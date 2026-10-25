import type {
  GitCheckoutOutcome,
  GitCheckoutTarget
} from '../../../shared/git-checkout-types'
import { getActiveRuntimeTarget } from './runtime-rpc-client'
import { resolveLocalWorktreePath, type RuntimeGitContext } from './runtime-git-client-context'

/**
 * 原地检出分支的 runtime 包装。
 *
 * **只支持本地目标**。远程 runtime 面在本仓库没有 real 实现
 * （`create-api.ts` 的 real 域清单里没有 `runtime` / `runtimeEnvironments`），
 * 所以不像 push/pull 那样开一条 `callRuntimeRpc` 分支 —— 但**必须显式拒绝**
 * 非 local 目标，而不是悄悄按本地做：否则用户选中远程 runtime 时，
 * 会拿远端机器的路径在**本机**跑 `git checkout`；只要同拼写路径在本机已授权，
 * 就是一次静默的错主机写操作。
 *
 * 为什么自己挡空路径：`resolveLocalWorktreePath` **不抛错** —— 它回落到
 * `context.worktreePath`，可能是空串。checkout 是写操作，把空路径下发给
 * `require_authorized_worktree` 去报错，不如在这层给出可读的失败。
 */
export async function checkoutRuntimeGit(
  context: RuntimeGitContext,
  target: GitCheckoutTarget
): Promise<GitCheckoutOutcome> {
  const runtimeTarget = getActiveRuntimeTarget(context.settings)
  if (runtimeTarget.kind !== 'local' && context.worktreeId) {
    throw new Error('checkoutRuntimeGit: remote runtime targets are not supported yet')
  }
  const worktreePath = resolveLocalWorktreePath(context)
  if (!worktreePath) {
    throw new Error('checkoutRuntimeGit: missing worktree path')
  }
  return window.api.git.checkout({ worktreePath, target })
}
