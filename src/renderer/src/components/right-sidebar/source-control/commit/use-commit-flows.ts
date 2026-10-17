import type { SourceControlPanelFoundation } from '../panel/use-panel-foundation'
import { useSourceControlConflictAbort } from '../sync/use-conflict-abort'
import { useSourceControlRemoteActionRunner } from '../sync/use-remote-action-runner'
import { useSourceControlCommitAction } from './use-commit-action'
import { useSourceControlCommitMessageGeneration } from './use-commit-message-generation'

/**
 * Everything that writes to the local branch: the commit itself, the message it gets, the remote
 * ops that follow it, and the escape hatch out of a conflicted merge or rebase.
 */
export function useSourceControlCommitFlows(foundation: SourceControlPanelFoundation) {
  const {
    activeRepo,
    activeRepoSettings,
    activeWorktree,
    activeWorktreeId,
    allocateCommitMessageGenerationRequestId,
    beginGitBranchCompareRequest,
    branchName,
    commitInFlightRef,
    commitMessage,
    commitMessageGenerationRecords,
    compareBaseRef,
    conflictOperation,
    effectiveBaseRef,
    fastForwardBranch,
    fetchBranch,
    generateErrors,
    generateInFlightByWorktree,
    generateInFlightRef,
    grouped,
    isAbortingOperation,
    openCommitGenerationDialog,
    pullBranch,
    pushBranch,
    rebaseFromBase,
    refreshActiveGitStatusAfterMutation,
    refreshBranchCompareRef,
    refreshGitHistoryRef,
    remoteActionErrorSequenceByWorktreeRef,
    remoteStatus,
    resolvedCommitMessageAi,
    setAbortOperationInFlightByWorktree,
    setCommitErrorForWorktree,
    setCommitInFlightByWorktree,
    setCommitMessageGenerationRecord,
    setGenerateErrors,
    setGenerateInFlightByWorktree,
    setRemoteActionErrors,
    settings,
    sourceControlAiActionsVisible,
    syncBranch,
    unresolvedConflicts,
    updateCommitDrafts,
    updateCommitMessageGenerationRecord,
    worktreePath
  } = foundation

  const commitAction = useSourceControlCommitAction({
    activeRepoSettings,
    activeWorktree,
    activeWorktreeId,
    beginGitBranchCompareRequest,
    commitInFlightRef,
    commitMessage,
    compareBaseRef,
    refreshActiveGitStatusAfterMutation,
    refreshBranchCompareRef,
    refreshGitHistoryRef,
    setCommitErrorForWorktree,
    setCommitInFlightByWorktree,
    stagedCount: grouped.staged.length,
    unresolvedConflictCount: unresolvedConflicts.length,
    updateCommitDrafts,
    worktreePath
  })
  const commitMessageGeneration = useSourceControlCommitMessageGeneration({
    activeRepo,
    activeRepoSettings,
    activeWorktreeId,
    allocateCommitMessageGenerationRequestId,
    commitMessageGenerationRecords,
    generateErrors,
    generateInFlightByWorktree,
    generateInFlightRef,
    openCommitGenerationDialog,
    resolvedCommitMessageAi,
    setCommitMessageGenerationRecord,
    setGenerateErrors,
    setGenerateInFlightByWorktree,
    settings,
    sourceControlAiActionsVisible,
    updateCommitDrafts,
    updateCommitMessageGenerationRecord,
    worktreePath
  })
  const remoteActionRunner = useSourceControlRemoteActionRunner({
    activeRepoSettings,
    activeWorktree,
    activeWorktreeId,
    branchName,
    effectiveBaseRef,
    fastForwardBranch,
    fetchBranch,
    grouped,
    handleCommit: commitAction.handleCommit,
    pullBranch,
    pushBranch,
    rebaseFromBase,
    refreshActiveGitStatusAfterMutation,
    refreshBranchCompareRef,
    refreshGitHistoryRef,
    remoteActionErrorSequenceByWorktreeRef,
    remoteStatus,
    setRemoteActionErrors,
    syncBranch,
    worktreePath
  })
  const conflictAbort = useSourceControlConflictAbort({
    activeRepoSettings,
    activeWorktreeId,
    conflictOperation,
    isAbortingOperation,
    refreshActiveGitStatusAfterMutation,
    refreshBranchCompareRef,
    refreshGitHistoryRef,
    setAbortOperationInFlightByWorktree,
    setRemoteActionErrors,
    worktreePath
  })

  return {
    ...commitAction,
    ...commitMessageGeneration,
    ...remoteActionRunner,
    ...conflictAbort
  }
}

export type SourceControlCommitFlows = ReturnType<typeof useSourceControlCommitFlows>
