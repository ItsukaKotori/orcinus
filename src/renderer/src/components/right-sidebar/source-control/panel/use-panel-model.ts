import { useSourceControlCommitFlows } from '../commit/use-commit-flows'
import { useSourceControlDiscardConfirmation } from '../commit/use-discard-confirmation'
import { useSourceControlEntryMutations } from '../commit/use-entry-mutations'
import { useSourceControlNoteOpening } from '../notes/use-note-opening'
import { useGitHistoryCommitActions } from '../sync/use-git-history-commit-actions'
import { useSourceControlUpstreamStatusFetch } from '../sync/use-upstream-status-fetch'
import { useSourceControlActionDispatch } from './use-action-dispatch'
import { useSourceControlActionModel } from './use-action-model'
import { useSourceControlPanelFoundation } from './use-panel-foundation'

/**
 * The panel's single entry point: foundation, then the flows that act on it, then the action model
 * the chrome renders from. The rendered tree reads only this.
 */
export function useSourceControlPanelModel() {
  const foundation = useSourceControlPanelFoundation()
  const commitFlows = useSourceControlCommitFlows(foundation)
  const {
    activeRepoSettings,
    activeWorktree,
    activeWorktreeId,
    branchEntries,
    branchName,
    branchSummary,
    clearSelection,
    commitMessage,
    conflictOperation,
    effectiveBaseRef,
    entries,
    fetchUpstreamStatus,
    grouped,
    handleOpenDiff,
    handleStageAllPrimary,
    inFlightRemoteOpKind,
    isAbortingOperation,
    isBranchVisible,
    isCommitting,
    isExecutingBulk,
    isFolder,
    isRemoteOperationActive,
    openCommittedDiff,
    refreshActiveGitStatusAfterMutation,
    remoteStatus,
    resolveSplitTargetGroupId,
    setIsExecutingBulk,
    sourceControlRef,
    unresolvedConflicts,
    worktreePath
  } = foundation
  const { handleAbortMerge, handleAbortRebase, handleCommit, runCompoundCommitAction, runRemoteAction } =
    commitFlows

  const actionModel = useSourceControlActionModel({
    grouped,
    commitMessage,
    unresolvedConflictCount: unresolvedConflicts.length,
    isCommitting,
    isRemoteOperationActive,
    isAbortingOperation,
    remoteStatus,
    inFlightRemoteOpKind,
    branchSummary,
    branchName,
    conflictOperation,
    effectiveBaseRef
  })
  const actionDispatch = useSourceControlActionDispatch({
    handleAbortMerge,
    handleAbortRebase,
    handleCommit,
    handleStageAllPrimary,
    primaryAction: actionModel.primaryAction,
    remoteStatus,
    runCompoundCommitAction,
    runRemoteAction
  })
  useSourceControlUpstreamStatusFetch({
    activeRepoSettings,
    activeWorktree,
    activeWorktreeId,
    fetchUpstreamStatus,
    isBranchVisible,
    isFolder,
    worktreePath
  })
  const gitHistoryCommitActions = useGitHistoryCommitActions({
    activeWorktreeId,
    worktreePath,
    activeRepoSettings,
    resolveSplitTargetGroupId
  })
  const noteOpening = useSourceControlNoteOpening({
    activeWorktreeId,
    worktreePath,
    entries,
    branchEntries,
    branchSummary,
    handleOpenDiff,
    openCommittedDiff,
    sourceControlRef
  })
  const entryMutations = useSourceControlEntryMutations({
    activeRepoSettings,
    activeWorktreeId,
    worktreePath,
    refreshActiveGitStatusAfterMutation
  })
  const discardConfirmation = useSourceControlDiscardConfirmation({
    activeRepoSettings,
    activeWorktreeId,
    worktreePath,
    grouped,
    isExecutingBulk,
    setIsExecutingBulk,
    clearSelection,
    discardMany: entryMutations.discardMany,
    discardSingle: entryMutations.discardSingle,
    refreshActiveGitStatusAfterMutation
  })

  return {
    ...foundation,
    ...commitFlows,
    ...actionModel,
    ...actionDispatch,
    ...gitHistoryCommitActions,
    ...noteOpening,
    ...entryMutations,
    ...discardConfirmation
  }
}

export type SourceControlPanelModel = ReturnType<typeof useSourceControlPanelModel>
