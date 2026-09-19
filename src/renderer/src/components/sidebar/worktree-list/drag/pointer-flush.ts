import type React from 'react'
import type { WorkspaceStatus } from '../../../../../../shared/worktree/types'
import { updateSidebarDragPreviewPosition } from '../../worktree-sidebar-pointer-drag-dom'
import { getPointerDropStatusTarget, shouldPreferSidebarStatusDropTarget } from './status-target'
import type { WorktreeDropCommitContext } from './drop-commit-context'
import {
  applyWorktreeDropPreview,
  applyWorktreeLineageDropPreview,
  clearWorktreeDropPreview,
  NO_WORKTREE_SIDEBAR_DROP_TARGET,
  updateLatestWorktreeStatusDropTarget,
  type WorktreePointerDrag,
  type WorktreeRowDragState,
  type WorktreeSidebarLineageDropTarget
} from './row-state'

const REORDER_INTENT_DELAY_MS = 160

export type WorktreePointerDragFrameArgs = {
  drag: WorktreePointerDrag
  ctx: WorktreeDropCommitContext
  setWorktreeDragState: React.Dispatch<React.SetStateAction<WorktreeRowDragState>>
  setDragOverStatus: (status: WorkspaceStatus | null) => void
  setPinDragOver: (pinDragOver: boolean) => void
}

// Reflect a status/pin hover that has no insertion line of its own.
function showStatusHoverWithoutInsertionLine(
  args: WorktreePointerDragFrameArgs,
  target: WorktreeSidebarLineageDropTarget
): void {
  const { drag, ctx } = args
  const statusDrop = target.status
    ? ctx.computeWorktreeStatusDrop({
        pointerY: drag.currentY,
        status: target.status,
        draggedIds: drag.reorderDraggedIds
      })
    : null
  updateLatestWorktreeStatusDropTarget(drag, target, statusDrop)
  if (statusDrop) {
    args.setDragOverStatus(null)
    args.setPinDragOver(false)
    args.setWorktreeDragState((prev) =>
      applyWorktreeDropPreview(prev, statusDrop, {
        pointerY: drag.currentY,
        matchPointerY: true
      })
    )
    return
  }
  args.setDragOverStatus(target.status)
  args.setPinDragOver(target.isPinDrop)
  args.setWorktreeDragState((prev) =>
    clearWorktreeDropPreview(prev, { pointerY: drag.currentY, matchPointerY: true })
  )
}

// One animation frame of an in-flight pointer drag: move the floating preview, then decide
// whether the pointer is over a status/pin section or a reorder slot.
export function flushWorktreePointerDragFrame(args: WorktreePointerDragFrameArgs): void {
  const { drag, ctx } = args
  drag.frameId = null
  if (!drag.active || !drag.preview) {
    return
  }
  delete drag.preview.dataset.worktreeSidebarNesting
  updateSidebarDragPreviewPosition({
    preview: drag.preview,
    pointerX: drag.currentX,
    pointerY: drag.currentY,
    offsetX: drag.previewOffsetX,
    offsetY: drag.previewOffsetY
  })
  if (!ctx.refreshWorktreeDragSession()) {
    ctx.clearWorktreeDrag()
    return
  }

  const sidebarContainer = ctx.scrollRef.current
  const preferredStatusTarget = ctx.getEligibleLineageDropTarget(
    sidebarContainer
      ? getPointerDropStatusTarget({
          container: sidebarContainer,
          x: drag.currentX,
          y: drag.currentY
        })
      : NO_WORKTREE_SIDEBAR_DROP_TARGET,
    drag.draggedIds
  )
  const lineageParentId = preferredStatusTarget.lineageParentId
  if (lineageParentId) {
    drag.reorderIntent = null
    updateLatestWorktreeStatusDropTarget(drag, preferredStatusTarget, null)
    drag.preview.dataset.worktreeSidebarNesting = 'true'
    updateSidebarDragPreviewPosition({
      preview: drag.preview,
      pointerX: drag.currentX,
      pointerY: drag.currentY,
      offsetX: drag.previewOffsetX,
      offsetY: drag.previewOffsetY
    })
    args.setDragOverStatus(null)
    args.setPinDragOver(false)
    args.setWorktreeDragState((prev) =>
      applyWorktreeLineageDropPreview(prev, lineageParentId, drag.currentY)
    )
    return
  }
  if (
    shouldPreferSidebarStatusDropTarget({
      sourceGroupKey: drag.sourceGroupKey,
      target: preferredStatusTarget,
      workspaceStatuses: ctx.workspaceStatuses
    })
  ) {
    drag.reorderIntent = null
    showStatusHoverWithoutInsertionLine(args, preferredStatusTarget)
    return
  }

  const drop = ctx.computeWorktreeDrop(drag.currentY)
  if (!drop) {
    drag.reorderIntent = null
    showStatusHoverWithoutInsertionLine(args, preferredStatusTarget)
    return
  }
  // Let the pointer cross a reorder gutter into the card before moving its target.
  let intent = drag.reorderIntent
  if (!intent || (intent.dropIndex !== drop.dropIndex && intent.pointerY !== drag.currentY)) {
    intent = {
      dropIndex: drop.dropIndex,
      pointerY: drag.currentY,
      startedAt: performance.now()
    }
  } else {
    // Autoscroll changes slots beneath a stationary pointer without renewing intent.
    intent.dropIndex = drop.dropIndex
    intent.pointerY = drag.currentY
  }
  drag.reorderIntent = intent
  if (performance.now() - intent.startedAt < REORDER_INTENT_DELAY_MS) {
    drag.latestStatusDropTarget = null
    args.setWorktreeDragState((prev) =>
      clearWorktreeDropPreview(prev, {
        pointerY: drag.currentY,
        preserveOffsets: true
      })
    )
    drag.frameId = window.requestAnimationFrame(() => flushWorktreePointerDragFrame(args))
    return
  }
  drag.latestStatusDropTarget = null
  args.setDragOverStatus(null)
  args.setPinDragOver(false)
  args.setWorktreeDragState((prev) =>
    applyWorktreeDropPreview(prev, drop, { pointerY: drag.currentY, matchPointerY: true })
  )
}
