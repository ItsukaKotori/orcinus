import { describe, expect, it } from 'vitest'
import type { WorktreeMeta } from '../../../../shared/worktree/meta-types'
import {
  buildWorktreeMetaUpdates,
  type WorktreeMetaDraft,
  type WorktreeMetaSnapshot
} from './worktree-meta-updates'

function makeDraft(overrides: Partial<WorktreeMetaDraft> = {}): WorktreeMetaDraft {
  return {
    displayNameInput: 'Workspace',
    commentInput: '',
    ...overrides
  }
}

function makeSnapshot(overrides: Partial<WorktreeMetaSnapshot> = {}): WorktreeMetaSnapshot {
  return {
    displayName: 'Workspace',
    comment: '',
    ...overrides
  }
}

/** Persistence raw-spreads updates, so a present-but-undefined key erases the
 *  stored value — the invariant is asserted on every build in this suite. */
function buildUpdates(
  draft: Partial<WorktreeMetaDraft>,
  snapshot: Partial<WorktreeMetaSnapshot> = {}
): Partial<WorktreeMeta> {
  const updates = buildWorktreeMetaUpdates(makeDraft(draft), makeSnapshot(snapshot))
  const undefinedKeys = Object.keys(updates).filter(
    (key) => updates[key as keyof WorktreeMeta] === undefined
  )
  expect(undefinedKeys).toEqual([])
  return updates
}

describe('buildWorktreeMetaUpdates', () => {
  it('writes an edited comment and nothing else', () => {
    const updates = buildUpdates({ commentInput: 'shipping today' })

    expect(updates).toEqual({ comment: 'shipping today' })
  })

  it('clears a display name with empty string, never a present-undefined key', () => {
    const updates = buildUpdates({ displayNameInput: '   ' }, { displayName: 'Custom Name' })

    expect(updates.displayName).toBe('')
  })

  // Persistence stamps lastActivityAt on any comment write, so re-emitting an
  // unchanged note reorders the workspace under the time-decay sidebar sort.
  it('emits no comment when the note is unchanged', () => {
    const updates = buildUpdates(
      { displayNameInput: 'Workspace', commentInput: 'shipping today' },
      { comment: 'shipping today' }
    )

    expect(updates).not.toHaveProperty('comment')
    expect(updates).not.toHaveProperty('displayName')
  })

  it('clears a comment with empty string, never a present-undefined key', () => {
    expect(buildUpdates({ commentInput: '  ' }, { comment: 'old note' }).comment).toBe('')
  })

  it('emits wait-free empty updates for an untouched draft', () => {
    expect(buildUpdates({}, {})).toEqual({})
  })
})
