import { useCallback } from 'react'
import { toast } from 'sonner'
import { activateWorkspaceTabPaletteResult } from '@/lib/workspace-tab-palette-activation'
import { activateAndRevealWorktree } from '@/lib/worktree-activation'
import { queueWorkspaceActivationTerminalFocus } from '@/lib/workspace-activation-terminal-focus'
import type { WorkspaceTabPaletteSearchResult } from '@/lib/workspace-tab-palette-search'
import type { CmdJActionResult, CmdJSettingsResult } from '@/components/cmd-j/palette-results'
import type { CmdJProjectSearchResult } from '@/components/cmd-j/palette-project-results'
import { getUnavailableQuickActionMessage } from './use-worktree-jump-palette-quick-actions'
import type { SettingsNavTarget } from '@/lib/settings-navigation-types'
import type { Worktree } from '../../../shared/worktree/types'
import { useAppStore } from '@/store'
import { getPaletteWorktreeExecutionHostId } from '@/lib/palette-repo-resolution'
import { translate } from '@/i18n/i18n'
import type { PaletteItem } from './worktree-jump-palette-model'
import type { WorktreeJumpPaletteLocalState } from './use-worktree-jump-palette-local-state'
import type { WorktreeJumpPaletteQuickActions } from './use-worktree-jump-palette-quick-actions'
import type { WorktreeJumpPaletteSelectionLifecycle } from './use-worktree-jump-palette-selection-lifecycle'
import type { WorktreeJumpPaletteStoreState } from './use-worktree-jump-palette-store-state'

function getSettingsTargetFromSectionId(sectionId: string): {
  pane: SettingsNavTarget
  repoId: string | null
  sectionId?: string
} {
  if (sectionId.startsWith('repo-')) {
    return { pane: 'repo', repoId: sectionId.slice('repo-'.length) }
  }
  return { pane: sectionId as SettingsNavTarget, repoId: null }
}

type WorktreeJumpPaletteSelectionActionsInput = WorktreeJumpPaletteStoreState &
  WorktreeJumpPaletteLocalState &
  Pick<WorktreeJumpPaletteQuickActions, 'buildQuickActionContext'> &
  Pick<WorktreeJumpPaletteSelectionLifecycle, 'focusFallbackSurface'>

export function useWorktreeJumpPaletteSelectionActions({
  closeModal,
  recordFeatureInteraction,
  skipRestoreFocusRef,
  setSelectedItemId,
  focusFallbackSurface,
  openSettingsTarget,
  openSettingsPage,
  buildQuickActionContext,
  revealSidebarRow,
  previousWorktreeIdRef,
  previousFocusElementRef
}: WorktreeJumpPaletteSelectionActionsInput) {
  const handleSelectWorktree = useCallback(
    (worktree: Worktree) => {
      const executionHostId = getPaletteWorktreeExecutionHostId(worktree)
      const current = useAppStore.getState().getKnownWorktreeById(worktree.id, executionHostId)
      if (!current) {
        toast.error(
          translate('auto.components.WorktreeJumpPalette.2c38630a01', 'Workspace no longer exists')
        )
        return
      }
      const activation = activateAndRevealWorktree(
        worktree.id,
        executionHostId ? { executionHostId } : {}
      )
      recordFeatureInteraction('cmd-j-workspace-open')
      skipRestoreFocusRef.current = true
      closeModal()
      setSelectedItemId('')
      if (!queueWorkspaceActivationTerminalFocus(worktree.id, activation)) {
        focusFallbackSurface()
      }
    },
    // oxlint-disable-next-line react-hooks/exhaustive-deps -- controller refs and setters preserve their original stable identities.
    [closeModal, focusFallbackSurface, recordFeatureInteraction]
  )
  const handleSelectWorkspaceTab = useCallback(
    (result: WorkspaceTabPaletteSearchResult) => {
      const activation = activateWorkspaceTabPaletteResult(result)
      if (activation.status === 'failed') {
        toast.error(
          activation.reason === 'missing-worktree'
            ? translate(
                'auto.components.WorktreeJumpPalette.2c38630a01',
                'Workspace no longer exists'
              )
            : translate(
                'auto.components.WorktreeJumpPalette.workspaceTabMissing',
                'Tab no longer exists'
              )
        )
        return
      }
      skipRestoreFocusRef.current = true
      closeModal()
      setSelectedItemId('')
    },
    // oxlint-disable-next-line react-hooks/exhaustive-deps -- controller refs and setters preserve their original stable identities.
    [closeModal]
  )
  const handleSelectSettings = useCallback(
    (result: CmdJSettingsResult) => {
      const target = getSettingsTargetFromSectionId(result.sectionId)
      if (result.targetSectionId) {
        target.sectionId = result.targetSectionId
      }
      skipRestoreFocusRef.current = true
      closeModal()
      setSelectedItemId('')
      openSettingsTarget(target)
      openSettingsPage()
      recordFeatureInteraction('cmd-j-settings-open')
    },
    // oxlint-disable-next-line react-hooks/exhaustive-deps -- controller refs and setters preserve their original stable identities.
    [closeModal, openSettingsPage, openSettingsTarget, recordFeatureInteraction]
  )
  const handleSelectQuickAction = useCallback(
    (action: CmdJActionResult) => {
      skipRestoreFocusRef.current = true
      closeModal()
      setSelectedItemId('')
      const context = buildQuickActionContext()
      void action
        .run(context)
        .then((result) => {
          if (result.status === 'unavailable') {
            toast.error(getUnavailableQuickActionMessage(action.title, result.reason))
            return
          }
          if (action.id === 'create-workspace') {
            recordFeatureInteraction('cmd-j-create-workspace')
            return
          }
          recordFeatureInteraction('cmd-j-quick-action')
        })
    },
    // oxlint-disable-next-line react-hooks/exhaustive-deps -- controller refs and setters preserve their original stable identities.
    [buildQuickActionContext, closeModal, recordFeatureInteraction]
  )
  const handleSelectProjectTarget = useCallback(
    (result: CmdJProjectSearchResult) => {
      skipRestoreFocusRef.current = true
      revealSidebarRow(result.rowKey, { behavior: 'smooth', highlight: true })
      recordFeatureInteraction('cmd-j')
      closeModal()
      setSelectedItemId('')
      if (previousWorktreeIdRef.current) {
        focusFallbackSurface(previousFocusElementRef.current)
      }
    },
    // oxlint-disable-next-line react-hooks/exhaustive-deps -- controller refs and setters preserve their original stable identities.
    [closeModal, focusFallbackSurface, recordFeatureInteraction, revealSidebarRow]
  )
  const handleSelectItem = useCallback(
    (item: PaletteItem) => {
      if (item.type === 'worktree') {
        handleSelectWorktree(item.worktree)
      } else if (item.type === 'project-target') {
        handleSelectProjectTarget(item.result)
      } else if (item.type === 'workspace-tab') {
        handleSelectWorkspaceTab(item.result)
      } else if (item.type === 'settings') {
        handleSelectSettings(item.result)
      } else {
        handleSelectQuickAction(item.result)
      }
    },
    [
      handleSelectProjectTarget,
      handleSelectQuickAction,
      handleSelectSettings,
      handleSelectWorkspaceTab,
      handleSelectWorktree
    ]
  )
  return { handleSelectItem }
}

export type WorktreeJumpPaletteSelectionActions = ReturnType<
  typeof useWorktreeJumpPaletteSelectionActions
>
