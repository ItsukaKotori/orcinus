import { describe, expect, it } from 'vitest'
import type { WorktreeMeta } from '../../../../shared/worktree/meta-types'
import {
  buildWorktreeMetaUpdates,
  type WorktreeMetaDraft,
  type WorktreeMetaLiveLinks,
  type WorktreeMetaSnapshot
} from './worktree-meta-updates'

function makeDraft(overrides: Partial<WorktreeMetaDraft> = {}): WorktreeMetaDraft {
  return {
    displayNameInput: 'Workspace',
    issueInput: '',
    issueProvider: 'github',
    reviewInput: '',
    commentInput: '',
    ...overrides
  }
}

function makeSnapshot(overrides: Partial<WorktreeMetaSnapshot> = {}): WorktreeMetaSnapshot {
  return {
    displayName: 'Workspace',
    comment: '',
    issueInput: '',
    issueProvider: 'github',
    prInput: '',
    ...overrides
  }
}

/** Persistence raw-spreads updates, so a present-but-undefined key erases the
 *  stored value — the invariant is asserted on every build in this suite. */
function buildUpdates(
  draft: Partial<WorktreeMetaDraft>,
  snapshot: Partial<WorktreeMetaSnapshot> = {},
  live: WorktreeMetaLiveLinks = {}
): Partial<WorktreeMeta> {
  const updates = buildWorktreeMetaUpdates(makeDraft(draft), makeSnapshot(snapshot), live)
  const undefinedKeys = Object.keys(updates).filter(
    (key) => updates[key as keyof WorktreeMeta] === undefined
  )
  expect(undefinedKeys).toEqual([])
  return updates
}

describe('buildWorktreeMetaUpdates', () => {
  // The dialog opens focused on Comment, so this is the common save path; a
  // regression here silently destroys the user's existing link.
  it('emits no link keys when the issue field is untouched', () => {
    const updates = buildUpdates(
      { issueInput: '42', commentInput: 'shipping today' },
      { issueInput: '42' }
    )

    expect(updates.comment).toBe('shipping today')
    expect(updates).not.toHaveProperty('linkedPR')
    expect(updates).not.toHaveProperty('linkedIssue')
  })

  it('writes a GitHub issue number', () => {
    expect(buildUpdates({ issueInput: '12' }, {})).toEqual({ linkedIssue: 12 })
  })

  it('clears the issue slot when the field is emptied', () => {
    expect(buildUpdates({ issueInput: '  ' }, { issueInput: '42' }, { linkedIssue: 42 })).toEqual({
      linkedIssue: null
    })
  })

  it('displaces a linked work item when the issue field changes', () => {
    const updates = buildUpdates(
      { issueInput: '12' },
      {},
      {
        linkedIssue: 42,
        linkedWorkItemProvider: 'github',
        linkedWorkItemType: 'issue'
      }
    )

    expect(updates).toHaveProperty('linkedWorkItem', null)
    expect(updates).toHaveProperty('linkedTaskSourceContext', null)
  })

  // linkedWorkItem also records the PR a workspace was created from, which the
  // Issue row does not own and must not drop.
  it('leaves a PR-typed work item alone', () => {
    const updates = buildUpdates(
      { issueInput: '12' },
      {},
      { linkedWorkItemProvider: 'github', linkedWorkItemType: 'pr' }
    )

    expect(updates).not.toHaveProperty('linkedWorkItem')
    expect(updates).not.toHaveProperty('linkedTaskSourceContext')
  })

  // The row cannot render a Jira or GitLab issue, so clearing one would destroy a
  // link the user was never shown — and neither has another editor to restore it.
  it('leaves work items owned by legacy providers alone', () => {
    for (const provider of ['jira', 'gitlab', 'linear'] as const) {
      const updates = buildUpdates(
        { issueInput: '12' },
        {},
        { linkedWorkItemProvider: provider, linkedWorkItemType: 'issue' }
      )

      expect(updates).not.toHaveProperty('linkedWorkItem')
      expect(updates).not.toHaveProperty('linkedTaskSourceContext')
    }
  })

  // Only the spelling changed, so the field names the same issue it already
  // holds — treating that as an edit would clear the title and source context.
  it('emits nothing when a GitHub number is respelled with a hash', () => {
    const updates = buildUpdates(
      { issueInput: '#42' },
      { issueInput: '42' },
      { linkedIssue: 42, linkedWorkItemProvider: 'github', linkedWorkItemType: 'issue' }
    )

    expect(updates).toEqual({})
  })

  it('leaves links untouched for unparseable issue input', () => {
    const updates = buildUpdates(
      { issueInput: 'not an issue', displayNameInput: 'Renamed', commentInput: 'note' },
      { displayName: 'Workspace' }
    )

    expect(updates).toEqual({
      comment: 'note',
      displayName: 'Renamed'
    })
  })

  it('clears a display name with empty string, never a present-undefined key', () => {
    const updates = buildUpdates({ displayNameInput: '   ' }, { displayName: 'Custom Name' })

    expect(updates.displayName).toBe('')
  })

  it('rejects issue URLs in the PR input', () => {
    expect(buildUpdates({ reviewInput: 'https://github.com/stablyai/orca/issues/6933' })).toEqual(
      {}
    )
  })

  it('accepts PR URLs in the PR input', () => {
    expect(buildUpdates({ reviewInput: 'https://github.com/stablyai/orca/pull/6934' })).toEqual({
      linkedPR: 6934
    })
  })

  it('records suppression when the user clears an explicit PR link', () => {
    expect(buildUpdates({ reviewInput: '' }, { prInput: '6934' }, { linkedPR: 6934 })).toEqual({
      linkedPR: null,
      suppressedGitHubPR: 6934
    })
  })

  it('does not invent suppression for an already-unlinked PR field', () => {
    expect(buildUpdates({ reviewInput: '' }, {}, { linkedPR: null })).toEqual({})
  })

  it('does not suppress a PR linked in the background when the PR field is untouched', () => {
    expect(buildUpdates({ commentInput: 'note' }, {}, { linkedPR: 6934 })).toEqual({
      comment: 'note'
    })
  })

  it('accepts issue URLs in the issue input', () => {
    expect(buildUpdates({ issueInput: 'https://github.com/stablyai/orca/issues/6933' })).toEqual({
      linkedIssue: 6933
    })
  })

  it('rejects PR URLs in the issue input', () => {
    expect(buildUpdates({ issueInput: 'https://github.com/stablyai/orca/pull/6934' })).toEqual({})
  })

  // Persistence stamps lastActivityAt on any comment write, so re-emitting an
  // unchanged note reorders the workspace under the time-decay sidebar sort.
  it('emits no comment when the note is unchanged', () => {
    const updates = buildUpdates(
      { issueInput: '12', commentInput: 'shipping today' },
      { comment: 'shipping today' }
    )

    expect(updates).not.toHaveProperty('comment')
    expect(updates.linkedIssue).toBe(12)
  })

  it('clears a comment with empty string, never a present-undefined key', () => {
    expect(buildUpdates({ commentInput: '  ' }, { comment: 'old note' }).comment).toBe('')
  })
})
