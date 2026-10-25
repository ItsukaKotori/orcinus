import type {
  GitCheckoutOutcome,
  GitCheckoutTarget
} from '../../../shared/git-checkout-types'
import { resolveLocalWorktreePath, type RuntimeGitContext } from './runtime-git-client-context'

/**
 * 原地检出分支的 runtime 包装。
 *
 * **只有本地一条路**：`runtime` / `runtimeEnvironments` 在本仓库没有 real 实现
 * （`create-api.ts` 的 real 域清单里没有它们，只有 mock + `withMethodFallback`），
 * 所以照 push/pull 的样子加一条 `callRuntimeRpc('git.checkout', …)` 只会得到一个
 * unimplemented。等远程 runtime 面真落地时再一起补，别提前铺一条没人能走的路。
 *
 * 为什么自己挡空路径：`resolveLocalWorktreePath` **不抛错** —— 它回落到
 * `context.worktreePath`，可能是空串。checkout 是写操作，把空路径下发给
 * `require_authorized_worktree` 去报错，不如在这层给出可读的失败。
 */
export async function checkoutRuntimeGit(
  context: RuntimeGitContext,
  target: GitCheckoutTarget
): Promise<GitCheckoutOutcome> {
  const worktreePath = resolveLocalWorktreePath(context)
  if (!worktreePath) {
    throw new Error('checkoutRuntimeGit: missing worktree path')
  }
  return window.api.git.checkout({ worktreePath, target })
}
