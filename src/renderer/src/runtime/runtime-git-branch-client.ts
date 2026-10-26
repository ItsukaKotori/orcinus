import type {
  GitBranchCreateInput,
  GitBranchCreateOutcome
} from '../../../shared/git-branch-ops-types'
import { getActiveRuntimeTarget } from './runtime-rpc-client'
import { resolveLocalWorktreePath, type RuntimeGitContext } from './runtime-git-client-context'

/**
 * 在 start-point 建分支（不检出）的 runtime 包装。
 *
 * **只支持本地**：远程 runtime 面无 real 实现，故非 local 目标**显式拒绝**（同
 * `checkoutRuntimeGit`），不静默按本地跑错主机写。空路径自己挡（resolveLocalWorktreePath
 * 不抛，回落到可能为空的 context.worktreePath）。业务拒绝（NameTaken）在 resolve 的
 * `outcome.status` 里；环境级失败才 reject。
 */
export async function createRuntimeGitBranch(
  context: RuntimeGitContext,
  input: GitBranchCreateInput
): Promise<GitBranchCreateOutcome> {
  const runtimeTarget = getActiveRuntimeTarget(context.settings)
  if (runtimeTarget.kind !== 'local' && context.worktreeId) {
    throw new Error('createRuntimeGitBranch: remote runtime targets are not supported yet')
  }
  const worktreePath = resolveLocalWorktreePath(context)
  if (!worktreePath) {
    throw new Error('createRuntimeGitBranch: missing worktree path')
  }
  return window.api.git.branchCreate({ worktreePath, args: input })
}
