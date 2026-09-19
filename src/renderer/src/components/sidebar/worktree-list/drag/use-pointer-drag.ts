import { useCallback, useEffect } from 'react'
import type React from 'react'
import type { Worktree } from '../../../../../../shared/worktree/types'
import { getWorktreeHostIdentity } from '../../../../../../shared/worktree/host-qualified-identity'
import {
  createSidebarDragPreview,
  isSidebarPointerDragBlocked,
  setSidebarPointerDragDocumentStyles
} from '../../worktree-sidebar-pointer-drag-dom'
import { getWorktreeSidebarDragRectsForGroup } from '../../worktree-sidebar-drag-autoscroll'
import { getWorktreeSidebarDragGrab } from '../../worktree-sidebar-drag-geometry'
import type { WorktreeDropCommitContext } from './drop-commit-context'
import type { WorktreeDragRuntime } from './use-runtime'
import type { WorktreeDragSession } from './use-session'
import { useWorktreePointerDragAutoscroll } from './use-pointer-autoscroll'
import { useWorktreePointerDragWindowEvents } from './use-pointer-window-events'
import { flushWorktreePointerDragFrame } from './pointer-flush'
import { EMPTY_WORKTREE_DRAG_PREVIEW_OFFSETS, type WorktreePointerDrag } from './row-state'

export function useWorktreePointerDrag(args: {
  ctx: WorktreeDropCommitContext
  session: WorktreeDragSession
  runtime: WorktreeDragRuntime
  scrollRef: React.RefObject<HTMLDivElement | null>
  markScrollMovement: () => void
  selectedWorktreeIds: ReadonlySet<string>
  selectedWorktrees: readonly Worktree[]
}) {
  const {
    ctx,
    session,
    runtime,
    scrollRef,
    markScrollMovement,
    selectedWorktreeIds,
    selectedWorktrees
  } = args
  const {
    worktreePointerDragRef,
    suppressWorktreeClickUntilRef,
    setWorktreeDragState,
    setDragOverStatus,
    setPinDragOver
  } = runtime

  const flushWorktreePointerDrag = useCallback(() => {
    const drag = worktreePointerDragRef.current
    if (!drag) {
      return
    }
    flushWorktreePointerDragFrame({
      drag,
      ctx,
      setWorktreeDragState,
      setDragOverStatus,
      setPinDragOver
    })
  }, [
    ctx,
    setDragOverStatus,
    setPinDragOver,
    setWorktreeDragState,
    worktreePointerDragRef
  ])

  const scheduleWorktreePointerDragFrame = useCallback(
    (drag: WorktreePointerDrag) => {
      if (drag.frameId !== null) {
        return
      }
      drag.frameId = window.requestAnimationFrame(flushWorktreePointerDrag)
    },
    [flushWorktreePointerDrag]
  )

  const startWorktreePointerAutoscroll = useWorktreePointerDragAutoscroll({
    session,
    runtime,
    scrollRef,
    markScrollMovement,
    scheduleWorktreePointerDragFrame
  })

  const beginWorktreePointerDrag = useCallback(
    (drag: WorktreePointerDrag) => {
      const { preview, offsetX, offsetY, height } = createSidebarDragPreview({
        sourceRow: drag.sourceRow,
        pointerX: drag.currentX,
        pointerY: drag.currentY,
        draggedCount: drag.draggedIds.length
      })
      drag.active = true
      drag.preview = preview
      drag.previewOffsetX = offsetX
      drag.previewOffsetY = offsetY
      suppressWorktreeClickUntilRef.current = window.performance.now() + 500
      setSidebarPointerDragDocumentStyles(true)
      session.worktreeDragSessionRef.current = {
        draggingWorktreeId: drag.worktreeId,
        sourceGroupKey: drag.sourceGroupKey,
        draggedIds: drag.draggedIds,
        reorderDraggedIds: drag.reorderDraggedIds,
        reorderUnitDraggedIds: drag.reorderUnitDraggedIds,
        rects: drag.rects,
        // Why: reuse the floating preview's own offset so the hit test tracks the
        // card the user sees, not the raw pointer.
        grab: getWorktreeSidebarDragGrab({ offsetY, height }),
        anchor: null
      }
      setWorktreeDragState({
        draggingWorktreeId: drag.worktreeId,
        sourceGroupKey: drag.sourceGroupKey,
        lineageDropTargetId: null,
        dropIndex: null,
        dropIndicatorY: null,
        previewOffsetsByWorktreeId: EMPTY_WORKTREE_DRAG_PREVIEW_OFFSETS,
        pointerY: drag.currentY
      })
      startWorktreePointerAutoscroll()
      scheduleWorktreePointerDragFrame(drag)
    },
    [
      scheduleWorktreePointerDragFrame,
      session,
      setWorktreeDragState,
      startWorktreePointerAutoscroll,
      suppressWorktreeClickUntilRef
    ]
  )

  const handleWorktreeRowPointerDown = useCallback(
    (event: React.PointerEvent<HTMLDivElement>, worktree: Worktree, rowKey: string) => {
      const worktreeId = worktree.id
      if (event.button !== 0 || event.pointerType === 'touch') {
        return
      }
      const sourceRow = event.currentTarget
      if (isSidebarPointerDragBlocked(event.target, sourceRow)) {
        return
      }
      const sourceGroupKey = session.groupKeyByRowKey.get(rowKey)
      const container = scrollRef.current
      if (!sourceGroupKey || !container) {
        return
      }
      const rects = getWorktreeSidebarDragRectsForGroup(container, sourceGroupKey)
      if (rects.length <= 1) {
        return
      }
      const draggedIds =
        selectedWorktreeIds.has(getWorktreeHostIdentity(worktree)) && selectedWorktrees.length > 1
          ? selectedWorktrees.map((worktree) => worktree.id)
          : [worktreeId]
      const reorderDraggedIds = session.getReorderDraggedIds(draggedIds)
      const reorderUnitDraggedIds = session.getReorderUnitDraggedIds(
        sourceGroupKey,
        reorderDraggedIds
      )
      worktreePointerDragRef.current = {
        pointerId: event.pointerId,
        sourceRow,
        startX: event.clientX,
        startY: event.clientY,
        currentX: event.clientX,
        currentY: event.clientY,
        worktreeId,
        draggedIds,
        reorderDraggedIds,
        reorderUnitDraggedIds,
        sourceGroupKey,
        rects,
        active: false,
        preview: null,
        previewOffsetX: 0,
        previewOffsetY: 0,
        frameId: null,
        reorderIntent: null,
        latestStatusDropTarget: null
      }
    },
    [
      scrollRef,
      selectedWorktreeIds,
      selectedWorktrees,
      session,
      worktreePointerDragRef
    ]
  )

  const handleWorktreeRowClickCapture = useCallback(
    (event: React.MouseEvent<HTMLDivElement>) => {
      if (window.performance.now() >= suppressWorktreeClickUntilRef.current) {
        return
      }
      event.preventDefault()
      event.stopPropagation()
    },
    [suppressWorktreeClickUntilRef]
  )

  useWorktreePointerDragWindowEvents({
    ctx,
    runtime,
    beginWorktreePointerDrag,
    scheduleWorktreePointerDragFrame
  })

  useEffect(() => {
    const handleClick = (event: MouseEvent): void => {
      if (window.performance.now() >= suppressWorktreeClickUntilRef.current) {
        return
      }
      event.preventDefault()
      event.stopPropagation()
      event.stopImmediatePropagation()
    }

    document.addEventListener('click', handleClick, true)
    return () => document.removeEventListener('click', handleClick, true)
  }, [suppressWorktreeClickUntilRef])

  return { handleWorktreeRowPointerDown, handleWorktreeRowClickCapture }
}
