import { useEffect } from 'react'
import { useSourceControlAi } from '../ai/use-ai'
import { useSourceControlFileListing } from '../listing/use-file-listing'
import { useSourceControlBaseRefContext } from './use-base-ref-context'
import { useSourceControlPanelState } from './use-panel-state'

/**
 * Everything the panel knows before any action runs: state, base refs, the projected file
 * listing and the AI surface. Split from the flows so no action hook can be read as a dependency
 * of the data it acts on.
 */
export function useSourceControlPanelFoundation() {
  const panelState = useSourceControlPanelState()
  const baseRefContext = useSourceControlBaseRefContext(panelState)
  const {
    activeConnectionId,
    activeGroupId,
    activeRemoteActionSequence,
    activeRepo,
    activeRepoSettings,
    activeSourceControlLaunchPlatform,
    activeWorktreeId,
    branchEntries,
    branchName,
    branchSummary,
    collapsedSections,
    collapsedTreeDirs,
    commitError,
    commitMessage,
    conflictOperation,
    entries,
    filterQuery,
    isBranchVisible,
    isFolder,
    isGitHistoryExpanded,
    isMac,
    openSettingsPage,
    openSettingsTarget,
    refreshActiveGitStatusAfterMutation,
    remoteActionError,
    rightSidebarTab,
    sourceControlGroupOrder,
    sourceControlRef,
    sourceControlViewMode,
    updateRepo,
    updateSettings,
    worktreeMap,
    worktreePath
  } = panelState

  const listing = useSourceControlFileListing({
    activeRemoteActionSequence,
    activeRepoSettings,
    activeWorktreeId,
    branchEntries,
    branchName,
    branchSummary,
    collapsedSections,
    collapsedTreeDirs,
    compareBaseRef: baseRefContext.compareBaseRef,
    entries,
    filterQuery,
    isBranchVisible,
    isFolder,
    isGitHistoryExpanded,
    isMac,
    refreshActiveGitStatusAfterMutation,
    remoteActionError,
    rightSidebarTab,
    sourceControlGroupOrder,
    sourceControlRef,
    sourceControlViewMode,
    worktreeMap,
    worktreePath
  })
  const ai = useSourceControlAi({
    settings: activeRepoSettings,
    activeRepo: activeRepo ?? null,
    activeWorktreeId,
    activeConnectionId,
    activeGroupId,
    activeSourceControlLaunchPlatform,
    conflictOperation,
    unresolvedConflicts: listing.unresolvedConflicts,
    stagedEntries: listing.grouped.staged,
    worktreePath,
    commitMessage,
    commitError,
    pushRecoveryPrompt: listing.pushRecovery?.prompt ?? null,
    updateSettings,
    updateRepo,
    openSettingsTarget,
    openSettingsPage
  })
  const { setCommitGenerationDialogOpen, setResolveConflictsComposerOpen, sourceControlAiActionsVisible } =
    ai

  useEffect(() => {
    if (sourceControlAiActionsVisible) {
      return
    }
    setResolveConflictsComposerOpen(false)
    setCommitGenerationDialogOpen(false)
  }, [setCommitGenerationDialogOpen, setResolveConflictsComposerOpen, sourceControlAiActionsVisible])

  return { ...panelState, ...baseRefContext, ...listing, ...ai }
}

export type SourceControlPanelFoundation = ReturnType<typeof useSourceControlPanelFoundation>
