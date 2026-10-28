import type {
  GitBranchCreateInput,
  GitBranchCreateOutcome,
  GitBranchSetUpstreamInput,
  GitBranchSetUpstreamOutcome,
  GitBranchRenameInput,
  GitBranchRenameOutcome,
  GitBranchDeleteInput,
  GitBranchDeleteOutcome
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

/**
 * 给已存在的本地分支设 upstream 的 runtime 包装。同 `createRuntimeGitBranch`：**只支持本地**，
 * 非 local 目标显式拒绝；业务拒绝（UpstreamMissing）在 resolve 的 `outcome.status` 里，不 reject。
 */
export async function setRuntimeGitBranchUpstream(
  context: RuntimeGitContext,
  input: GitBranchSetUpstreamInput
): Promise<GitBranchSetUpstreamOutcome> {
  const runtimeTarget = getActiveRuntimeTarget(context.settings)
  if (runtimeTarget.kind !== 'local' && context.worktreeId) {
    throw new Error('setRuntimeGitBranchUpstream: remote runtime targets are not supported yet')
  }
  const worktreePath = resolveLocalWorktreePath(context)
  if (!worktreePath) {
    throw new Error('setRuntimeGitBranchUpstream: missing worktree path')
  }
  return window.api.git.branchSetUpstream({ worktreePath, args: input })
}

/** 重命名本地分支（仅未检出）的 runtime 包装。同 create：只本地、拒绝不 reject。 */
export async function renameRuntimeGitBranch(
  context: RuntimeGitContext,
  input: GitBranchRenameInput
): Promise<GitBranchRenameOutcome> {
  const runtimeTarget = getActiveRuntimeTarget(context.settings)
  if (runtimeTarget.kind !== 'local' && context.worktreeId) {
    throw new Error('renameRuntimeGitBranch: remote runtime targets are not supported yet')
  }
  const worktreePath = resolveLocalWorktreePath(context)
  if (!worktreePath) {
    throw new Error('renameRuntimeGitBranch: missing worktree path')
  }
  return window.api.git.branchRename({ worktreePath, args: input })
}

/** 删除本地分支（仅未检出）的 runtime 包装。NotMergeable/CheckedOut 在 resolve 的 status 里，不 reject。 */
export async function deleteRuntimeGitBranch(
  context: RuntimeGitContext,
  input: GitBranchDeleteInput
): Promise<GitBranchDeleteOutcome> {
  const runtimeTarget = getActiveRuntimeTarget(context.settings)
  if (runtimeTarget.kind !== 'local' && context.worktreeId) {
    throw new Error('deleteRuntimeGitBranch: remote runtime targets are not supported yet')
  }
  const worktreePath = resolveLocalWorktreePath(context)
  if (!worktreePath) {
    throw new Error('deleteRuntimeGitBranch: missing worktree path')
  }
  return window.api.git.branchDelete({ worktreePath, args: input })
}
