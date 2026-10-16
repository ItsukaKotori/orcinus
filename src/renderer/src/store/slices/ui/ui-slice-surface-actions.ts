import type { UISlice, UISliceGet, UISliceSet } from './ui-slice-contract'

export function createUiSurfaceActions(set: UISliceSet, _get: UISliceGet): Partial<UISlice> {
  return {
    pendingRevealWorktree: null,
    pendingRevealSidebarRow: null,
    revealWorktreeInSidebar: (worktreeId, options) =>
      set({
        pendingRevealWorktree: {
          worktreeId,
          ...(options?.executionHostId ? { executionHostId: options.executionHostId } : {}),
          behavior: options?.behavior ?? 'smooth',
          ...(options?.highlight ? { highlight: true } : {}),
          ...(options?.beginRename ? { beginRename: true } : {})
        }
      }),
    revealSidebarRow: (rowKey, options) =>
      set({
        pendingRevealSidebarRow: {
          rowKey,
          behavior: options?.behavior ?? 'smooth',
          ...(options?.highlight === false ? {} : { highlight: true })
        }
      }),
    clearPendingRevealWorktreeId: () => set({ pendingRevealWorktree: null }),
    clearPendingRevealSidebarRow: () => set({ pendingRevealSidebarRow: null }),
    scrollToDiffCommentId: null,
    setScrollToDiffCommentId: (id) => set({ scrollToDiffCommentId: id })
  }
}
