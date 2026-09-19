import { getFullDropIndexForWorktreeDragUnit } from '../../worktree-drag-units'
import { resolveWorktreeSidebarStatusDropCommitTarget } from '../../worktree-sidebar-drop-preview'
import { getPointerDropStatusTarget, shouldPreferSidebarStatusDropTarget } from './status-target'
import type { WorktreeDropCommitContext } from './drop-commit-context'
import type { WorktreeSidebarStatusDropTarget } from '../../worktree-sidebar-drop-preview'
import { NO_WORKTREE_SIDEBAR_DROP_TARGET, type WorktreePointerDrag } from './row-state'

type PointerDropCommitArgs = {
  event: PointerEvent
  drag: WorktreePointerDrag
  ctx: WorktreeDropCommitContext
}

function commitStatusOrPinDrop(
  args: PointerDropCommitArgs,
  target: WorktreeSidebarStatusDropTarget & { lineageParentId?: string | null },
  dropIndex: number | null
): void {
  const { drag, ctx } = args
  if (target.isPinDrop) {
    ctx.onPinWorktrees(drag.draggedIds)
    return
  }
  if (!target.status) {
    return
  }
  if (dropIndex === null) {
    ctx.onMoveWorktreesToStatus(drag.reorderDraggedIds, target.status)
    return
  }
  ctx.onMoveWorktreesToStatusAtIndex({
    worktreeIds: drag.reorderDraggedIds,
    status: target.status,
    dropIndex,
    groups: ctx.worktreeDragGroups
  })
}

// Resolve where a released pointer drag lands: lineage parent, status/pin
// section, or a reorder slot inside the source group.
export function commitWorktreePointerDrop(args: PointerDropCommitArgs): void {
  const { event, drag, ctx } = args
  if (!ctx.refreshWorktreeDragSession()) {
    ctx.clearWorktreeDrag()
    return
  }
  const preferredStatusTarget = ctx.getEligibleLineageDropTarget(
    ctx.scrollRef.current
      ? getPointerDropStatusTarget({
          container: ctx.scrollRef.current,
          x: event.clientX,
          y: event.clientY
        })
      : NO_WORKTREE_SIDEBAR_DROP_TARGET,
    drag.draggedIds
  )
  if (preferredStatusTarget.lineageParentId) {
    ctx.commitWorktreeLineageParentDrop(drag.draggedIds, preferredStatusTarget.lineageParentId)
    ctx.clearWorktreeDrag()
    return
  }
  if (
    shouldPreferSidebarStatusDropTarget({
      sourceGroupKey: drag.sourceGroupKey,
      target: preferredStatusTarget,
      workspaceStatuses: ctx.workspaceStatuses
    })
  ) {
    const statusDrop = preferredStatusTarget.status
      ? ctx.computeWorktreeStatusDrop({
          pointerY: event.clientY,
          status: preferredStatusTarget.status,
          draggedIds: drag.reorderDraggedIds
        })
      : null
    commitStatusOrPinDrop(args, preferredStatusTarget, statusDrop?.dropIndex ?? null)
    ctx.clearWorktreeDrag()
    return
  }
  const drop = ctx.computeWorktreeDrop(event.clientY)
  if (drop) {
    ctx.onReorderWorktrees({
      groups: ctx.worktreeDragGroups,
      sourceGroupKey: drag.sourceGroupKey,
      draggedIds: drag.reorderDraggedIds,
      dropIndex: getFullDropIndexForWorktreeDragUnit({
        groups: ctx.worktreeDragUnitGroups,
        sourceGroupKey: drag.sourceGroupKey,
        dropIndex: drop.dropIndex
      })
    })
    ctx.clearReorderedWorktreeParents({
      draggedIds: drag.draggedIds,
      sourceGroupKey: drag.sourceGroupKey
    })
  } else if (ctx.scrollRef.current) {
    const currentPreview = preferredStatusTarget.status
      ? ctx.computeWorktreeStatusDrop({
          pointerY: event.clientY,
          status: preferredStatusTarget.status,
          draggedIds: drag.reorderDraggedIds
        })
      : null
    const { target, preview: statusDrop } = resolveWorktreeSidebarStatusDropCommitTarget({
      currentTarget: preferredStatusTarget,
      currentPreview,
      latestTrackedTarget: drag.latestStatusDropTarget,
      x: event.clientX,
      y: event.clientY
    })
    if (target.lineageParentId) {
      ctx.commitWorktreeLineageParentDrop(drag.draggedIds, target.lineageParentId)
    } else {
      commitStatusOrPinDrop(args, target, statusDrop?.dropIndex ?? null)
    }
  }
  ctx.clearWorktreeDrag()
}
