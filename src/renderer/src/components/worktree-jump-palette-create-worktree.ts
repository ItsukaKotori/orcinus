import { getRepoMapFromState } from '@/store/selectors'
import { useAppStore } from '@/store'
import { isGitRepoKind } from '../../../shared/repo-kind'
import { isWorktreePaletteCreateActivationAllowed } from '@/lib/worktree-palette-create-action'
import type { WorktreeJumpPaletteFilter } from './use-worktree-jump-palette-filter'
import type { WorktreeJumpPaletteLocalState } from './use-worktree-jump-palette-local-state'
import type { WorktreeJumpPaletteQuickActions } from './use-worktree-jump-palette-quick-actions'
import type { WorktreeJumpPaletteStoreState } from './use-worktree-jump-palette-store-state'

type WorktreeJumpPaletteCreateWorktreeInput = Pick<
  WorktreeJumpPaletteStoreState,
  'closeModal' | 'openModal' | 'recordFeatureInteraction'
> &
  Pick<WorktreeJumpPaletteFilter, 'repoMap'> &
  Pick<
    WorktreeJumpPaletteLocalState,
    | 'createLookupGuard'
    | 'createWorktreeName'
    | 'liveQueryRef'
    | 'preserveCreateLookupOnCloseRef'
    | 'selectionMovedByUserRef'
    | 'skipRestoreFocusRef'
  > &
  Pick<WorktreeJumpPaletteQuickActions, 'prefetchCreateWorkspaceBaseForComposer'>

export function createWorktreeJumpPaletteWorktreeHandler({
  closeModal,
  createWorktreeName,
  liveQueryRef,
  openModal,
  prefetchCreateWorkspaceBaseForComposer,
  recordFeatureInteraction,
  repoMap,
  selectionMovedByUserRef,
  skipRestoreFocusRef
}: WorktreeJumpPaletteCreateWorktreeInput): () => void {
  return () => {
    const trimmed = createWorktreeName.trim()
    if (liveQueryRef.current.trim() !== trimmed) {
      return
    }
    if (
      !isWorktreePaletteCreateActivationAllowed({
        hasCreateName: trimmed.length > 0,
        selectionMovedByUser: selectionMovedByUserRef.current
      })
    ) {
      return
    }
    const state = useAppStore.getState()
    const repo =
      (state.activeRepoId ? (repoMap.get(state.activeRepoId) ?? null) : null) ||
      [...getRepoMapFromState(state).values()].find((candidate) => isGitRepoKind(candidate))
    const openComposer = (data: Record<string, unknown>): void => {
      skipRestoreFocusRef.current = true
      prefetchCreateWorkspaceBaseForComposer(
        typeof data.initialRepoId === 'string' ? data.initialRepoId : undefined
      )
      closeModal()
      recordFeatureInteraction('cmd-j-create-workspace')
      queueMicrotask(() =>
        openModal('new-workspace-composer', { ...data, telemetrySource: 'command_palette' })
      )
    }
    openComposer({
      ...(trimmed ? { prefilledName: trimmed } : {}),
      ...(repo ? { initialRepoId: repo.id } : {})
    })
  }
}

export type { WorktreeJumpPaletteCreateWorktreeInput }
