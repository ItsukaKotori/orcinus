import type { WorktreeApi } from '../../shared/preload-api/api/worktree-api'
import type {
  LegacyDetectedWorktreeRequest,
  ListDetectedWorktreesArgs
} from '../../shared/detected-worktree-provider-contract'
import type { DetectedWorktreeListResult, Worktree } from '../../shared/worktree/types'
import { withMethodFallback } from '../unimplemented-fallback'
import { invokeCommand, subscribeToEvent } from './invoke'

const noopUnsubscribe = (): void => {}
const noopSubscription = (): (() => void) => noopUnsubscribe

/**
 * A's `worktrees_list` is a real scan (git porcelain or folder-workspace
 * projection) but carries no ownership metadata, so every row projects to the
 * legacy authoritative shape the renderer admits (spec §5.3).
 */
function toLegacyDetectedWorktreeResult(
  repoId: string,
  worktrees: readonly Worktree[]
): DetectedWorktreeListResult {
  return {
    repoId,
    authoritative: true,
    source: 'git',
    worktrees: worktrees.map((worktree) => ({
      ...worktree,
      ownership: 'orca-managed',
      selectedCheckout: worktree.isMainWorktree,
      visible: true
    }))
  }
}

async function listDetected(
  args: ListDetectedWorktreesArgs | LegacyDetectedWorktreeRequest
): Promise<DetectedWorktreeListResult> {
  const worktrees = await invokeCommand<Worktree[]>('worktrees_list', {
    args: { repoId: args.repoId }
  })
  return toLegacyDetectedWorktreeResult(args.repoId, worktrees)
}

/**
 * Real `worktrees` adapter (spec §5.3). The registry projection, the detected
 * provider lease, and the `worktrees:changed` event are real; every other
 * surface either answers the provider contract empty/rejected or stays on
 * `withMethodFallback` until its subproject lands.
 */
export function createWorktreesRealApi(): WorktreeApi {
  return withMethodFallback<WorktreeApi>('worktrees', {
    list: (args) => invokeCommand('worktrees_list', { args }),
    listDetected,
    listAll: () => invokeCommand('worktrees_list_all'),
    listKnownForExecutionHost: async ({ repoId, executionHostId }) => ({
      status: 'rejected',
      repoId,
      executionHostId
    }),
    forgetRemovedForExecutionHost: async () => ({ forgottenWorktreeIds: [] }),
    cancelListDetected: async () => {},
    onChanged: (callback) => subscribeToEvent('worktrees:changed', callback),
    onGitStatusMetadataChanged: noopSubscription,
    onHeadIdentitiesChanged: noopSubscription,
    onBaseStatus: noopSubscription,
    onRemoteBranchConflict: noopSubscription,
    onCreateProgress: noopSubscription
  })
}
