import type { WorktreeSlice } from '../../worktree-helpers'
import type { WorktreeSliceGet, WorktreeSliceSet } from '../listing/worktree-slice-types'
import { translate } from '@/i18n/i18n'
import { displayNameUpdatePinsLabel } from '../../../../../../shared/worktree/display-name-provenance'
import { parseWorkspaceKey } from '../../../../../../shared/workspace-scope'
import { applyWorktreeUpdates, getRepoIdFromWorktreeId } from '../../worktree-helpers'
import {
  applyDetectedWorktreeUpdates,
  findKnownWorktreeById,
  getFolderWorkspaceMetaUpdates
} from '../listing/detected-worktree-meta'
import { persistWorktreeMeta } from './worktree-meta-persist'
import { isRuntimeSelectorNotFoundError } from '../listing/runtime-worktree-rpc-errors'
import { settingsForWorktreeOwner } from '../listing/worktree-owner-settings'

export function createUpdateWorktreeMeta(
  set: WorktreeSliceSet,
  get: WorktreeSliceGet
): WorktreeSlice['updateWorktreeMeta'] {
  return async (worktreeId, updates, options) => {
    const shouldApplyUpdate = options?.shouldApply
    const requestedHostId = options?.executionHostId
    const existingWorktree = findKnownWorktreeById(get(), worktreeId, requestedHostId)
    const executionHostId =
      requestedHostId ??
      existingWorktree?.hostId ??
      (get().settings?.activeRuntimeEnvironmentId ? undefined : 'local')
    if (shouldApplyUpdate && !shouldApplyUpdate(existingWorktree)) {
      return { ok: true }
    }
    const workspaceScope = parseWorkspaceKey(worktreeId)
    if (workspaceScope?.type === 'folder') {
      const folderUpdates = getFolderWorkspaceMetaUpdates(updates)
      if (Object.keys(folderUpdates).length === 0) {
        return { ok: true }
      }
      try {
        // Why: a rejected folder update reconciles the optimistic write away, so
        // reporting ok would show the dialog a save that silently undid itself.
        const updated = await get().updateFolderWorkspace(
          workspaceScope.folderWorkspaceId,
          folderUpdates
        )
        return updated
          ? { ok: true }
          : {
              ok: false,
              error: translate(
                'auto.store.slices.worktrees.a17f4d2e93',
                'Could not update this workspace.'
              )
            }
      } catch (err) {
        console.error('Failed to update folder workspace meta:', err)
        return { ok: false, error: err instanceof Error ? err.message : String(err) }
      }
    }
    const worktreeForUpdate = get().getKnownWorktreeById(worktreeId, executionHostId)
    if (shouldApplyUpdate && !shouldApplyUpdate(worktreeForUpdate)) {
      return { ok: true }
    }
    // Why: bump lastActivityAt on comment edits so the time-decay sort doesn't drop a just-touched worktree.
    const displayNameProvenance =
      'displayName' in updates
        ? { displayNameIsPinned: displayNameUpdatePinsLabel(updates.displayName) }
        : {}
    const targetEnriched = { ...updates, ...displayNameProvenance }
    const renameCleared =
      'displayName' in targetEnriched
        ? {
            ...targetEnriched,
            pendingFirstAgentMessageRename: false,
            firstAgentMessageRenameError: null
          }
        : targetEnriched
    const enriched =
      'comment' in renameCleared ? { ...renameCleared, lastActivityAt: Date.now() } : renameCleared

    let didApply = false
    set((s) => {
      if (
        shouldApplyUpdate &&
        !shouldApplyUpdate(findKnownWorktreeById(s, worktreeId, executionHostId))
      ) {
        return {}
      }
      didApply = true
      const nextWorktrees = applyWorktreeUpdates(
        s.worktreesByRepo,
        worktreeId,
        enriched,
        executionHostId
      )
      const nextDetectedWorktrees = applyDetectedWorktreeUpdates(
        s.detectedWorktreesByRepo,
        worktreeId,
        enriched,
        executionHostId
      )
      if (
        nextWorktrees === s.worktreesByRepo &&
        nextDetectedWorktrees === s.detectedWorktreesByRepo
      ) {
        return {}
      }

      return {
        ...(nextWorktrees !== s.worktreesByRepo
          ? { worktreesByRepo: nextWorktrees, sortEpoch: s.sortEpoch + 1 }
          : {}),
        ...(nextDetectedWorktrees !== s.detectedWorktreesByRepo
          ? { detectedWorktreesByRepo: nextDetectedWorktrees }
          : {})
      }
    })
    if (shouldApplyUpdate && !didApply) {
      return { ok: true }
    }

    try {
      await persistWorktreeMeta(
        settingsForWorktreeOwner(get(), worktreeId, executionHostId),
        worktreeId,
        enriched,
        executionHostId ?? existingWorktree?.hostId,
        worktreeForUpdate?.identity?.key
      )
    } catch (err) {
      if (isRuntimeSelectorNotFoundError(err)) {
        void get().fetchWorktrees(getRepoIdFromWorktreeId(worktreeId))
        return {
          ok: false,
          error: translate(
            'auto.store.slices.worktrees.c6cf133786',
            'This workspace is no longer available.'
          )
        }
      }
      console.error('Failed to update worktree meta:', err)
      void get().fetchWorktrees(getRepoIdFromWorktreeId(worktreeId))
      // Why: the refetch above reverts the optimistic write, so a caller that
      // closes its surface on this path shows the user a save that undid itself.
      return { ok: false, error: err instanceof Error ? err.message : String(err) }
    }
    return { ok: true }
  }
}
