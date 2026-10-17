import { translate } from '@/i18n/i18n'
import {
  coerceWorktreeCardVisibleTitle,
  getWorktreeCardTitleDisplay
} from './worktree-card-title-display'
import { useWorkspaceDeleteModifierPressed } from './workspace-delete-quick-action'
import type { WorktreeCardProps } from './worktree-card-model'
import type { useWorktreeCardFoundation } from './use-worktree-card-foundation'
import type { useWorktreeCardIdentityDetails } from './use-worktree-card-identity-details'

type Foundation = ReturnType<typeof useWorktreeCardFoundation>
type IdentityDetails = ReturnType<typeof useWorktreeCardIdentityDetails>

export function useWorktreeCardLinkedDetails({
  worktree,
  newCardStyle,
  deleteState,
  branch
}: Pick<WorktreeCardProps, 'worktree'> &
  Pick<Foundation, 'newCardStyle' | 'deleteState'> &
  Pick<IdentityDetails, 'branch'>) {
  const cardTitleDisplay = getWorktreeCardTitleDisplay({
    storedDisplayName: worktree.displayName,
    branchName: branch
  })
  const legacyCardTitleDisplay = coerceWorktreeCardVisibleTitle(worktree.displayName)
  const visibleCardTitle = newCardStyle ? cardTitleDisplay : legacyCardTitleDisplay
  const isDeleting = deleteState?.isDeleting ?? false
  const isQueuedForDeletion = deleteState?.phase === 'queued'
  const deleteLabel = isQueuedForDeletion
    ? translate('auto.components.sidebar.WorktreeCard.ef18787206', 'Queued for deletion')
    : translate('auto.components.sidebar.WorktreeCard.691ccfd622', 'Deleting…')
  const deleteModifierPressed = useWorkspaceDeleteModifierPressed()

  return {
    visibleCardTitle,
    isDeleting,
    isQueuedForDeletion,
    deleteLabel,
    deleteModifierPressed
  }
}
