import { CommitArea } from '../commit/commit-area'
import { writeCommitDraftForWorktree } from '../commit/commit-drafts'
import type { SourceControlPanelReadyProps } from './panel-props'

type SourceControlCommitSurfaceProps = SourceControlPanelReadyProps & {
  showGenericEmptyState: boolean
}

/**
 * The one action surface below the file list: the review composer when Create review is the direct
 * next step, otherwise the commit box.
 */
export function SourceControlCommitSurface({
  activeRepo,
  model,
  showGenericEmptyState
}: SourceControlCommitSurfaceProps) {
  const {
    activeConnectionId,
    activeGroupId,
    activeSourceControlLaunchPlatform,
    activeWorktreeId,
    commitError,
    commitFailureRecoveryPrompt,
    commitMessage,
    dropdownItems,
    generateError,
    getLaunchActionRecipe,
    grouped,
    handleActionInvoke,
    handleCancelGenerate,
    handleFixCommitFailureWithAI,
    handleFixPushFailureWithAI,
    handleGenerateCommitMessageClick,
    handlePrimaryClick,
    hasPartiallyStagedChanges,
    inFlightRemoteOpKind,
    isAbortingOperation,
    isCommitting,
    isGenerating,
    isLaunchingCommitFailureAgent,
    isLaunchingPushFailureAgent,
    isRemoteOperationActive,
    openSourceControlAiSettings,
    primaryAction,
    pushRecovery,
    remoteActionError,
    resolvedCommitMessageAi,
    saveLaunchActionDefault,
    sourceControlAiActionsVisible,
    unresolvedConflicts,
    updateCommitDrafts
  } = model

  return (
    <CommitArea
      worktreeId={activeWorktreeId}
      connectionId={activeConnectionId}
      repoId={activeRepo.id}
      launchPlatform={activeSourceControlLaunchPlatform}
      commitMessage={commitMessage}
      commitError={commitError}
      commitFailureRecoveryPrompt={commitFailureRecoveryPrompt}
      pushRecovery={pushRecovery}
      remoteActionError={pushRecovery ? null : (remoteActionError?.message ?? null)}
      isCommitting={isCommitting}
      isFixingCommitFailureWithAI={isLaunchingCommitFailureAgent}
      isFixingPushFailureWithAI={isLaunchingPushFailureAgent}
      groupId={activeGroupId ?? activeWorktreeId}
      showComposer={!showGenericEmptyState}
      sourceControlAiActionsVisible={sourceControlAiActionsVisible}
      aiAgentConfigured={resolvedCommitMessageAi?.ok === true}
      isGenerating={isGenerating}
      generateError={generateError}
      stagedCount={grouped.staged.length}
      hasPartiallyStagedChanges={hasPartiallyStagedChanges}
      hasUnresolvedConflicts={unresolvedConflicts.length > 0}
      isRemoteOperationActive={isRemoteOperationActive || isAbortingOperation}
      inFlightRemoteOpKind={inFlightRemoteOpKind}
      primaryAction={primaryAction}
      dropdownItems={dropdownItems}
      fixCommitFailureRecipe={getLaunchActionRecipe('fixCommitFailure')}
      fixPushFailureRecipe={getLaunchActionRecipe('fixPushFailure')}
      onCommitMessageChange={(value) => {
        if (!activeWorktreeId) {
          return
        }
        updateCommitDrafts((prev) => writeCommitDraftForWorktree(prev, activeWorktreeId, value))
      }}
      onGenerate={handleGenerateCommitMessageClick}
      onCancelGenerate={handleCancelGenerate}
      onSaveLaunchActionDefault={saveLaunchActionDefault}
      onOpenSourceControlAiSettings={openSourceControlAiSettings}
      onFixCommitFailureWithAI={handleFixCommitFailureWithAI}
      onFixPushFailureWithAI={handleFixPushFailureWithAI}
      onPrimaryAction={handlePrimaryClick}
      onDropdownAction={handleActionInvoke}
    />
  )
}
