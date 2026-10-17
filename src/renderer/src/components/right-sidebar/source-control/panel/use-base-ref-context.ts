import type { SourceControlPanelState } from './use-panel-state'
import { useSourceControlBaseRefs } from '../sync/use-base-refs'
import { useSourceControlBranchCompare } from '../sync/use-branch-compare'

/**
 * Resolves the base refs and branch compare the panel works against — the
 * worktree/repo pins, the repo default, and the compare view — without any
 * hosted-review coupling.
 */
export function useSourceControlBaseRefContext(panelState: SourceControlPanelState) {
  const {
    activeGitStatusHead,
    activeRepo,
    activeRepoConnectionId,
    activeRepoExecutionHostId,
    activeRepoId,
    activeRepoRuntimeEnvironmentId,
    activeRepoSettings,
    activeWorktree,
    activeWorktreeId,
    branchName,
    isBranchVisible,
    isFolder,
    remoteStatus,
    settings,
    worktreePath
  } = panelState

  const baseRefs = useSourceControlBaseRefs({
    activeRepoConnectionId,
    activeRepoExecutionHostId,
    activeRepoId,
    activeRepoRuntimeEnvironmentId,
    activeRepoWorktreeBaseRef: activeRepo?.worktreeBaseRef,
    activeWorktreeBaseRef: activeWorktree?.baseRef,
    isBranchVisible,
    isFolder,
    remoteStatus,
    settings
  })
  const { compareBaseRef } = baseRefs
  const branchCompare = useSourceControlBranchCompare({
    activeRepoSettings,
    activeWorktreeId,
    worktreePath,
    compareBaseRef,
    isFolder,
    branchName,
    isBranchVisible,
    activeGitStatusHead,
    remoteStatus
  })

  return { ...baseRefs, ...branchCompare }
}

export type SourceControlBaseRefContext = ReturnType<typeof useSourceControlBaseRefContext>
