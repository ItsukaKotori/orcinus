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
 * provider lease, the local create/remove/forget/meta mutations, and the
 * `worktrees:changed` event are real; the remote/runtime surfaces either answer
 * the provider contract empty/rejected or stay on `withMethodFallback` until
 * their subproject lands. Mutation payloads pass through untranslated — the
 * Rust commands answer the `{ worktree }`, `{ preservedBranch? }`, and
 * full-`Worktree` shapes the TS contract already declares (`warnings` stays
 * unemitted: it is a lineage shape B has no data for).
 */
export function createWorktreesRealApi(): WorktreeApi {
  return withMethodFallback<WorktreeApi>('worktrees', {
    list: (args) => invokeCommand('worktrees_list', { args }),
    listDetected,
    create: (args) => invokeCommand('worktrees_create', { args }),
    remove: (args) => invokeCommand('worktrees_remove', { args }),
    forgetLocal: (args) => invokeCommand('worktrees_forget_local', { args }),
    forceDeletePreservedBranch: (args) =>
      invokeCommand('worktrees_force_delete_preserved_branch', { args }),
    updateMeta: (args) => invokeCommand('worktrees_update_meta', { args }),
    persistSortOrder: (args) => invokeCommand('worktrees_persist_sort_order', { args }),
    listAll: () => invokeCommand('worktrees_list_all'),
    listKnownForExecutionHost: async ({ repoId, executionHostId }) => ({
      status: 'rejected',
      repoId,
      executionHostId
    }),
    forgetRemovedForExecutionHost: async () => ({ forgottenWorktreeIds: [] }),
    // Why: the renderer renders the lineage view at startup; A owns no lineage
    // metadata yet, so it answers the empty map instead of a fabricated rejection.
    listLineage: async () => ({ lineage: {} }),
    cancelListDetected: async () => {},
    onChanged: (callback) => subscribeToEvent('worktrees:changed', callback),
    onGitStatusMetadataChanged: noopSubscription,
    onHeadIdentitiesChanged: noopSubscription,
    onBaseStatus: noopSubscription,
    onRemoteBranchConflict: noopSubscription,
    onCreateProgress: noopSubscription
  })
}
