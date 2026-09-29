import type { PreloadApi } from '../../shared/preload-api/api-types'
import { withMethodFallback } from '../unimplemented-fallback'
import { invokeCommand } from './invoke'

/**
 * Real `git` adapter (spec §5.2). The 17 `git_*` commands are thin wrappers
 * over the Rust surface: every payload goes out as a single `{ args }` object
 * and the command payload comes back untouched — the TS contract governs the
 * types, so admission tiers and diff binary flags are never reshaped at
 * runtime. Git has no `on*` events; every method outside the map below
 * (`setStatusUpstreamRefWatch`, the commit-message helpers, fetch/push/pull,
 * …) stays on the rejecting `withMethodFallback` surface.
 */
export function createGitRealApi(): PreloadApi['git'] {
  return withMethodFallback<PreloadApi['git']>('git', {
    status: (args) => invokeCommand('git_status', { args }),
    cancelStatus: (args) => invokeCommand('git_cancel_status', { args }),
    diff: (args) => invokeCommand('git_diff', { args }),
    stage: (args) => invokeCommand('git_stage', { args }),
    bulkStage: (args) => invokeCommand('git_bulk_stage', { args }),
    unstage: (args) => invokeCommand('git_unstage', { args }),
    bulkUnstage: (args) => invokeCommand('git_bulk_unstage', { args }),
    discard: (args) => invokeCommand('git_discard', { args }),
    bulkDiscard: (args) => invokeCommand('git_bulk_discard', { args }),
    commit: (args) => invokeCommand('git_commit', { args }),
    upstreamStatus: (args) => invokeCommand('git_upstream_status', { args }),
    conflictOperation: (args) => invokeCommand('git_conflict_operation', { args }),
    branchCompare: (args) => invokeCommand('git_branch_compare', { args }),
    commitCompare: (args) => invokeCommand('git_commit_compare', { args }),
    branchDiff: (args) => invokeCommand('git_branch_diff', { args }),
    commitDiff: (args) => invokeCommand('git_commit_diff', { args }),
    history: (args) => invokeCommand('git_history', { args })
  })
}
