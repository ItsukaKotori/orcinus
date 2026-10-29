import { assertGitPushTargetShape } from '../../shared/git-push-target-validation'
import { resolveConfiguredGitPushTarget } from '../../shared/git-push-target-resolution'
import type { PreloadApi } from '../../shared/preload-api/api-types'
import { createRunGit, defaultGitReadExecutor } from '../../renderer/src/lib/git-read-client'
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
 * map below (`setStatusUpstreamRefWatch`, the commit-message helpers, …)
 * stays on the rejecting `withMethodFallback` surface.
 *
 * `checkout` / `fetch` / `pull` 在映射里：对应的 tauri 命令已真实注册。
 * 此前 `fetch` / `pull` 没有后端命令，而 `runtime-git-sync-client.ts` 的本地
 * 分支照旧调 `window.api.git.fetch/pull` → 右侧栏 pull/sync 按钮点了就是
 * unimplemented（spec §3.7）。
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
    logQuery: (args) => invokeCommand('git_log_query', { args }),
    refs: (args) => invokeCommand('git_refs', { args }),
    checkout: (args) => invokeCommand('git_checkout', { args }),
    branchCreate: (args) => invokeCommand('git_branch_create', { args }),
    branchSetUpstream: (args) => invokeCommand('git_branch_set_upstream', { args }),
    branchRename: (args) => invokeCommand('git_branch_rename', { args }),
    branchDelete: (args) => invokeCommand('git_branch_delete', { args }),
    // 与 push 同一分层：TS 侧把 `pushTarget` 解成 remote 名，Rust 不碰 GitPushTarget
    // （那是 TS-only 类型）。没 pushTarget 就交给 git 自己按 upstream/默认 remote 定。
    fetch: (args) =>
      // 注：只下发 worktreePath + remote。`connectionId` / `worktreeId` 是
      // runtime 客户端传过来的多余字段，Rust 侧 serde 默认忽略；
      // 不要以为它们到了后端。
      invokeCommand('git_fetch', {
        args: {
          worktreePath: args.worktreePath,
          ...(args.pushTarget ? { remote: args.pushTarget.remoteName } : {}),
        },
      }),
    pull: (args) =>
      invokeCommand('git_pull', { args: { worktreePath: args.worktreePath } }),
    // fast-forward = `git pull --ff-only`（Rust 侧），只下发 worktreePath；pushTarget/worktreeId/connectionId 为 runtime 侧多余字段，Rust 忽略。
    fastForward: (args) =>
      invokeCommand('git_fast_forward', { args: { worktreePath: args.worktreePath } }),
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
