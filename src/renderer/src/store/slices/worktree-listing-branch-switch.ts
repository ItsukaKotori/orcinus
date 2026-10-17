import type { Worktree } from '../../../../shared/worktree/types'

function indexUnambiguousWorktrees(
  worktrees: readonly Worktree[],
  include?: (worktree: Worktree) => boolean
): Map<string, Worktree | null> {
  const byId = new Map<string, Worktree | null>()
  for (const worktree of worktrees) {
    if (include && !include(worktree)) {
      continue
    }
    byId.set(worktree.id, byId.has(worktree.id) ? null : worktree)
  }
  return byId
}

/**
 * Preserve newer branch/head identities over a stale worktree-listing refresh.
 *
 * Why: a terminal branch switch is observed by two independent refresh paths —
 * the git-status identity path and the worktree-listing path. If a listing
 * response started before the switch lands after it, the plain merge would roll
 * the row back to the pre-switch branch; keeping the newer row preserves the
 * identity the git-status path already committed.
 */
export function preserveNewerListingBranchIdentities(args: {
  requestStarted: readonly Worktree[] | undefined
  current: readonly Worktree[] | undefined
  incoming: Worktree[]
  matchesRefreshHost: (worktree: Worktree) => boolean
}): Worktree[] {
  const { requestStarted, current, incoming, matchesRefreshHost } = args
  if (!requestStarted?.length || !current?.length) {
    return incoming
  }

  const startedById = indexUnambiguousWorktrees(requestStarted, matchesRefreshHost)
  const latestById = indexUnambiguousWorktrees(current, matchesRefreshHost)
  let reconciled: Worktree[] | null = null
  for (const [index, worktree] of incoming.entries()) {
    const requestStartedWorktree = startedById.get(worktree.id)
    const existing = latestById.get(worktree.id)
    if (
      requestStartedWorktree &&
      existing &&
      (existing.branch !== requestStartedWorktree.branch ||
        existing.head !== requestStartedWorktree.head)
    ) {
      // Why: preserve the newer row so the subsequent listing merge cannot
      // roll its branch and head back.
      reconciled ??= [...incoming]
      reconciled[index] = existing
    }
  }
  return reconciled ?? incoming
}
