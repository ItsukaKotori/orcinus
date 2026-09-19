import { useCallback } from 'react'
import { useAppStore } from '@/store'
import type {
  WorkspaceStatus,
  WorkspaceStatusDefinition,
  Worktree
} from '../../../../../../shared/worktree/types'
import type { WorktreeMeta } from '../../../../../../shared/worktree/meta-types'
import type { WorktreeMetaBatchUpdate } from '../../../../store/slices/worktree-helpers'
import { getWorkspaceStatus, getWorkspaceStatusGroupKey } from '../../workspace-status'
import {
  buildManualOrderUpdatesForGroupDrop,
  buildManualOrderUpdatesForVisibleGroups,
  type WorktreeDragGroup
} from '../../worktree-manual-order'
import type { WorktreeStatusDropAtIndexArgs } from './drop-commit-context'
import type { WorktreeManualOrderCatalog } from '../../worktree-manual-order-catalog'

// Every write a sidebar drop can make: status changes, pin, and manual order.
export function useWorktreeStatusMutations(args: {
  worktreeMap: Map<string, Worktree>
  manualOrderCatalog: WorktreeManualOrderCatalog
  workspaceStatuses: readonly WorkspaceStatusDefinition[]
}) {
  const { manualOrderCatalog, worktreeMap, workspaceStatuses } = args
  const updateWorktreeMeta = useAppStore((s) => s.updateWorktreeMeta)
  const updateWorktreesMeta = useAppStore((s) => s.updateWorktreesMeta)
  const setSortBy = useAppStore((s) => s.setSortBy)
  const setWorktreesPinnedAndReveal = useAppStore((s) => s.setWorktreesPinnedAndReveal)

  const moveWorktreeToStatus = useCallback(
    (worktreeId: string, status: WorkspaceStatus) => {
      const current = worktreeMap.get(worktreeId)
      if (!current || getWorkspaceStatus(current, workspaceStatuses) === status) {
        return
      }
      void updateWorktreeMeta(
        worktreeId,
        { workspaceStatus: status },
        { executionHostId: current.hostId ?? 'local' }
      )
    },
    [updateWorktreeMeta, worktreeMap, workspaceStatuses]
  )

  const moveWorktreesToStatus = useCallback(
    (worktreeIds: readonly string[], status: WorkspaceStatus) => {
      const updates: WorktreeMetaBatchUpdate[] = []
      for (const worktreeId of worktreeIds) {
        const current = worktreeMap.get(worktreeId)
        if (!current || getWorkspaceStatus(current, workspaceStatuses) === status) {
          continue
        }
        updates.push({
          worktreeId,
          updates: { workspaceStatus: status },
          executionHostId: current.hostId ?? 'local'
        })
      }
      if (updates.length > 0) {
        void updateWorktreesMeta(updates)
      }
    },
    [updateWorktreesMeta, worktreeMap, workspaceStatuses]
  )

  const moveWorktreesToStatusAtIndex = useCallback(
    (dropArgs: WorktreeStatusDropAtIndexArgs) => {
      const order = buildManualOrderUpdatesForGroupDrop({
        groups: dropArgs.groups,
        targetGroupKey: getWorkspaceStatusGroupKey(dropArgs.status),
        draggedIds: dropArgs.worktreeIds,
        dropIndex: dropArgs.dropIndex,
        now: Date.now(),
        rankByWorktreeId: manualOrderCatalog.rankByWorktreeId,
        allWorktreeIds: manualOrderCatalog.orderedIds
      })
      const updates = new Map<string, WorktreeMetaBatchUpdate>()
      for (const worktreeId of dropArgs.worktreeIds) {
        const current = worktreeMap.get(worktreeId)
        if (!current) {
          continue
        }
        const next: Partial<WorktreeMeta> = {}
        if (getWorkspaceStatus(current, workspaceStatuses) !== dropArgs.status) {
          next.workspaceStatus = dropArgs.status
        }
        updates.set(worktreeId, {
          worktreeId,
          updates: next,
          executionHostId: current.hostId ?? 'local'
        })
      }
      for (const [worktreeId, manualOrder] of order.updates) {
        const entry = updates.get(worktreeId)
        if (entry) {
          entry.updates = { ...entry.updates, ...manualOrder }
        }
      }
      for (const [worktreeId, entry] of updates) {
        if (Object.keys(entry.updates).length === 0) {
          updates.delete(worktreeId)
        }
      }
      if (updates.size === 0) {
        return
      }
      // Why: the insertion line promises exact placement, so persist manual order on a cross-status drop.
      if (order.changed) {
        setSortBy('manual')
      }
      void updateWorktreesMeta([...updates.values()])
    },
    [manualOrderCatalog, setSortBy, updateWorktreesMeta, worktreeMap, workspaceStatuses]
  )

  const pinWorktree = useCallback(
    (worktreeId: string) => {
      setWorktreesPinnedAndReveal([worktreeId], true)
    },
    [setWorktreesPinnedAndReveal]
  )

  const pinWorktrees = useCallback(
    (worktreeIds: readonly string[]) => {
      setWorktreesPinnedAndReveal(worktreeIds, true)
    },
    [setWorktreesPinnedAndReveal]
  )

  const reorderWorktrees = useCallback(
    (reorderArgs: {
      groups: readonly WorktreeDragGroup[]
      sourceGroupKey: string
      draggedIds: readonly string[]
      dropIndex: number
    }) => {
      const result = buildManualOrderUpdatesForVisibleGroups({
        ...reorderArgs,
        now: Date.now(),
        rankByWorktreeId: manualOrderCatalog.rankByWorktreeId,
        allWorktreeIds: manualOrderCatalog.orderedIds
      })
      if (result.changed) {
        setSortBy('manual')
      }
      void updateWorktreesMeta(
        [...result.updates].map(([worktreeId, updates]) => ({
          worktreeId,
          updates,
          executionHostId: worktreeMap.get(worktreeId)?.hostId ?? 'local'
        }))
      )
    },
    [manualOrderCatalog, setSortBy, updateWorktreesMeta, worktreeMap]
  )

  return {
    moveWorktreeToStatus,
    moveWorktreesToStatus,
    moveWorktreesToStatusAtIndex,
    pinWorktree,
    pinWorktrees,
    reorderWorktrees
  }
}
