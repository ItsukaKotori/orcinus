import { assertGitPushTargetShape } from '../../shared/git-push-target-validation'
import { resolveConfiguredGitPushTarget } from '../../shared/git-push-target-resolution'
import type { PreloadApi } from '../../shared/preload-api/api-types'
import { createRunGit, defaultGitReadExecutor } from '../../renderer/src/lib/github/git-read-client'
import { withMethodFallback } from '../unimplemented-fallback'
import { invokeCommand } from './invoke'

/**
 * Real `git` adapter (spec §5.2). The `git_*` commands are thin wrappers over
 * the Rust surface: every payload goes out as a single `{ args }` object and
 * the command payload comes back untouched — the TS contract governs the
 * types, so admission tiers and diff binary flags are never reshaped at
 * runtime. `push` resolves its destination first (`pushTarget`, else the
 * branch's configured push target, else `origin HEAD`) and then delegates to
 * `git_push`; Git has no `on*` events, so every remaining method outside the
 * map below (`setStatusUpstreamRefWatch`, the commit-message helpers,
 * fetch/pull, …) stays on the rejecting `withMethodFallback` surface.
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
    history: (args) => invokeCommand('git_history', { args }),
    push: async (args) => {
      const runGit = createRunGit(defaultGitReadExecutor(args.worktreePath))
      const forceWithLease = args.forceWithLease === true
      let remote = 'origin'
      let refspec = 'HEAD'
      if (args.pushTarget) {
        if (args.pushTarget.remoteUrl) {
          throw new Error('Push targets with a remote URL are not supported yet.')
        }
        assertGitPushTargetShape(args.pushTarget)
        await runGit(['check-ref-format', '--branch', args.pushTarget.branchName])
        remote = args.pushTarget.remoteName
        refspec = `HEAD:${args.pushTarget.branchName}`
      } else {
        const resolved = await resolveConfiguredGitPushTarget(runGit)
        if (resolved) {
          remote = resolved.remote
          refspec = resolved.refspec
        }
      }
      // `remote`/`refspec` stay explicit even for the `origin HEAD` fallback:
      // the Rust side accepts `null` (Option) with the same default, but
      // naming the fallback here keeps the wire call self-describing.
      await invokeCommand('git_push', {
        args: { worktreePath: args.worktreePath, remote, refspec, forceWithLease }
      })
    }
  })
}
