import { describe, expect, it } from 'vitest'
import type { Worktree } from '../../../../shared/worktree/types'
import { preserveNewerListingBranchIdentities } from './worktree-listing-branch-switch'

function makeWorktree(overrides: Partial<Worktree> & { id: string }): Worktree {
  return {
    repoId: 'repo1',
    path: '/path/wt',
    head: 'abc123',
    branch: 'refs/heads/feature-one',
    isBare: false,
    isMainWorktree: false,
    displayName: 'feature-one',
    comment: '',
    linkedIssue: null,
    linkedPR: null,
    linkedLinearIssue: null,
    linkedGitLabMR: null,
    linkedGitLabIssue: null,
    isArchived: false,
    isUnread: false,
    isPinned: false,
    sortOrder: 0,
    lastActivityAt: 0,
    ...overrides
  }
}

const matchesAnyHost = (): boolean => true

describe('preserveNewerListingBranchIdentities', () => {
  it('preserves a newer branch/head over a stale listing response', () => {
    const requestStarted = [makeWorktree({ id: 'repo1::/path/wt1', head: 'abc123' })]
    const current = [
      makeWorktree({ id: 'repo1::/path/wt1', branch: 'refs/heads/feature-two', head: 'def456' })
    ]

    const reconciled = preserveNewerListingBranchIdentities({
      requestStarted,
      current,
      incoming: [
        makeWorktree({ id: 'repo1::/path/wt1', branch: 'refs/heads/feature-one', head: 'abc123' })
      ],
      matchesRefreshHost: matchesAnyHost
    })

    expect(reconciled).toEqual(current)
  })

  it('returns the incoming rows untouched when identities match', () => {
    const requestStarted = [makeWorktree({ id: 'repo1::/path/wt1' })]
    const incoming = [makeWorktree({ id: 'repo1::/path/wt1' })]

    const reconciled = preserveNewerListingBranchIdentities({
      requestStarted,
      current: requestStarted,
      incoming,
      matchesRefreshHost: matchesAnyHost
    })

    expect(reconciled).toBe(incoming)
  })

  it('returns the incoming rows untouched without request/current baselines', () => {
    const incoming = [makeWorktree({ id: 'repo1::/path/wt1' })]

    expect(
      preserveNewerListingBranchIdentities({
        requestStarted: undefined,
        current: undefined,
        incoming,
        matchesRefreshHost: matchesAnyHost
      })
    ).toBe(incoming)
    expect(
      preserveNewerListingBranchIdentities({
        requestStarted: [],
        current: [],
        incoming,
        matchesRefreshHost: matchesAnyHost
      })
    ).toBe(incoming)
  })

  it('leaves cold-hydration rows with no baseline entry alone', () => {
    const requestStarted = [makeWorktree({ id: 'repo1::/path/other' })]
    const incoming = [
      makeWorktree({ id: 'repo1::/path/wt1', branch: 'refs/heads/feature-two', head: 'def456' })
    ]

    const reconciled = preserveNewerListingBranchIdentities({
      requestStarted,
      current: requestStarted,
      incoming,
      matchesRefreshHost: matchesAnyHost
    })

    expect(reconciled).toBe(incoming)
  })

  it('fails closed when the same worktree id belongs to multiple hosts', () => {
    const duplicated = [
      makeWorktree({ id: 'repo1::/same/path', hostId: 'local' }),
      makeWorktree({ id: 'repo1::/same/path', hostId: 'ssh:ssh-1' })
    ]
    const incoming = [
      makeWorktree({ id: 'repo1::/same/path', branch: 'refs/heads/feature-two', head: 'def456' })
    ]

    const reconciled = preserveNewerListingBranchIdentities({
      requestStarted: duplicated,
      current: duplicated,
      incoming,
      matchesRefreshHost: matchesAnyHost
    })

    expect(reconciled).toBe(incoming)
  })

  it('does not preserve a response from a host the row no longer belongs to', () => {
    const incoming = [
      makeWorktree({
        id: 'repo1::/same/path',
        hostId: 'ssh:ssh-1',
        branch: 'refs/heads/feature-two',
        head: 'new-head'
      })
    ]

    const reconciled = preserveNewerListingBranchIdentities({
      requestStarted: [makeWorktree({ id: 'repo1::/same/path', hostId: 'ssh:ssh-1' })],
      current: [makeWorktree({ id: 'repo1::/same/path', hostId: 'ssh:ssh-2' })],
      incoming,
      matchesRefreshHost: (worktree) => worktree.hostId === 'ssh:ssh-2'
    })

    expect(reconciled).toBe(incoming)
  })
})
