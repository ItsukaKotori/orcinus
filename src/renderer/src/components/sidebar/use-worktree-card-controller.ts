import { canShowWorkspaceDeleteQuickAction } from './workspace-delete-quick-action'
import { useWorktreeCardDetailsHoverControl } from './worktree-card-details-hover-state'
import type { ResolvedWorktreeCardProps } from './worktree-card-model'
import { useWorktreeCardActivationActions } from './use-worktree-card-activation-actions'
import { useWorktreeCardFoundation } from './use-worktree-card-foundation'
import { useWorktreeCardIdentityDetails } from './use-worktree-card-identity-details'
import { useWorktreeCardLinkedDetails } from './use-worktree-card-linked-details'
import { useWorktreeCardSecondaryDetails } from './use-worktree-card-secondary-details'
import { useWorktreeCardWorkspaceActions } from './use-worktree-card-workspace-actions'

export function useWorktreeCardController(props: ResolvedWorktreeCardProps) {
  const { worktree, repo } = props
  const foundation = useWorktreeCardFoundation({ worktree, repo })
  const identity = useWorktreeCardIdentityDetails({
    worktree,
    repo,
    projectGroups: foundation.projectGroups,
    cardProps: foundation.cardProps,
    newCardStyle: foundation.newCardStyle
  })
  const linked = useWorktreeCardLinkedDetails({
    worktree,
    newCardStyle: foundation.newCardStyle,
    deleteState: foundation.deleteState,
    branch: identity.branch
  })

  const showStatus = foundation.cardProps.includes('status')
  const showAutomation = foundation.cardProps.includes('automation')
  const showCli = foundation.cardProps.includes('cli')
  const showComment = foundation.cardProps.includes('comment')
  const detailsHoverControl = useWorktreeCardDetailsHoverControl()

  const activation = useWorktreeCardActivationActions({
    worktree,
    repo,
    affiliateListMode: props.affiliateListMode,
    onSelectionGesture: props.onSelectionGesture,
    isActive: props.isActive,
    activationRowKey: props.activationRowKey,
    onActivate: props.onActivate,
    onImmediateActivate: props.onImmediateActivate,
    isDeleting: linked.isDeleting,
    isSshDisconnected: foundation.isSshDisconnected,
    updateWorktreeMeta: foundation.updateWorktreeMeta,
    openModal: foundation.openModal
  })

  // Why: delete is destructive, so it only appears while holding Option/Alt, not in the ordinary hover chrome.
  const showDeleteQuickAction =
    !props.affiliateListMode &&
    canShowWorkspaceDeleteQuickAction({
      deleteModifierPressed: linked.deleteModifierPressed,
      isDeleting: linked.isDeleting,
      isMainWorktree: worktree.isMainWorktree
    })
  const workspaceActions = useWorktreeCardWorkspaceActions({
    worktree,
    lineageChildCount: props.lineageChildCount,
    lineageCollapsed: props.lineageCollapsed,
    onLineageToggle: props.onLineageToggle,
    isMultiSelected: props.isMultiSelected,
    selectedWorktrees: props.selectedWorktrees,
    onCardDragStart: props.onCardDragStart,
    onCardDragEnd: props.onCardDragEnd,
    onContextMenuSelect: props.onContextMenuSelect,
    folderWorkspaceId: identity.folderWorkspaceId,
    deleteFolderWorkspace: foundation.deleteFolderWorkspace,
    setActiveWorktree: foundation.setActiveWorktree,
    setShowRenameErrorDialog: foundation.setShowRenameErrorDialog,
    isDeleting: linked.isDeleting,
    showDeleteQuickAction
  })

  const secondary = useWorktreeCardSecondaryDetails({
    worktree,
    showStatus,
    showAutomation,
    showCli,
    showComment,
    cardProps: foundation.cardProps,
    newCardStyle: foundation.newCardStyle,
    compactCards: foundation.compactCards,
    agentActivityDisplayMode: foundation.agentActivityDisplayMode,
    settings: foundation.settings
  })

  return {
    ...props,
    ...foundation,
    ...identity,
    ...linked,
    detailsHoverControl,
    showStatus,
    showAutomation,
    showCli,
    showComment,
    ...activation,
    showDeleteQuickAction,
    ...workspaceActions,
    ...secondary
  }
}

export type WorktreeCardController = ReturnType<typeof useWorktreeCardController>
