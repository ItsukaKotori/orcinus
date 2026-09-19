import type { TabsSlice, TabsSliceGet, TabsSliceSet } from './tabs-slice-contract'
import { findTabAndWorktree, patchTab, updateGroup, dedupeTabOrder } from '../tab-group-state'
import { applyTabOrderSortValues, partitionPinnedTabOrder } from './tabs-tab-order'
import { mirrorTabPinnedToHost, patchTerminalTabRow } from './tabs-host-mirroring'

export function createTabsLabelActions(
  set: TabsSliceSet,
  get: TabsSliceGet
): Pick<
  TabsSlice,
  | 'reorderUnifiedTabs'
  | 'setTabLabel'
  | 'setTabCustomLabel'
  | 'setUnifiedTabColor'
  | 'pinTab'
  | 'unpinTab'
> {
  return {
    reorderUnifiedTabs: (groupId, tabIds, opts) => {
      let reordered = false
      set((state) => {
        for (const [worktreeId, groups] of Object.entries(state.groupsByWorktree)) {
          const group = groups.find((candidate) => candidate.id === groupId)
          if (!group) {
            continue
          }
          // Why: dedupe at the store boundary so each tab keeps one canonical position and later group ops don't branch on duplicate ids.
          const nextTabOrder = dedupeTabOrder(tabIds)
          reordered = true
          const orderMap = new Map(nextTabOrder.map((id, index) => [id, index]))
          return {
            groupsByWorktree: {
              ...state.groupsByWorktree,
              [worktreeId]: updateGroup(groups, { ...group, tabOrder: nextTabOrder })
            },
            unifiedTabsByWorktree: {
              ...state.unifiedTabsByWorktree,
              [worktreeId]: (state.unifiedTabsByWorktree[worktreeId] ?? []).map((tab) => {
                const sortOrder = orderMap.get(tab.id)
                return sortOrder === undefined ? tab : { ...tab, sortOrder }
              })
            }
          }
        }
        return {}
      })
      if (reordered && opts?.recordInteraction !== false) {
        get().recordFeatureInteraction?.('terminal-tabs')
      }
    },

    setTabLabel: (tabId, label) => {
      set((state) => patchTab(state.unifiedTabsByWorktree, tabId, { label }) ?? {})
    },

    setTabCustomLabel: (tabId, label, opts) => {
      const exists = get().getTab(tabId) !== null
      set((state) => patchTab(state.unifiedTabsByWorktree, tabId, { customLabel: label }) ?? {})
      if (exists && opts?.recordInteraction !== false) {
        get().recordFeatureInteraction?.('terminal-tabs')
      }
    },

    setUnifiedTabColor: (tabId, color) => {
      const exists = get().getTab(tabId) !== null
      set((state) => patchTab(state.unifiedTabsByWorktree, tabId, { color }) ?? {})
      if (exists) {
        get().recordFeatureInteraction?.('terminal-tabs')
      }
    },

    pinTab: (tabId) => {
      const exists = get().getTab(tabId) !== null
      set((state) => {
        const found = findTabAndWorktree(state.unifiedTabsByWorktree, tabId)
        if (!found) {
          return {}
        }
        const { tab, worktreeId } = found
        const tabs = (state.unifiedTabsByWorktree[worktreeId] ?? []).map((candidate) =>
          candidate.id === tabId ? { ...candidate, isPinned: true, isPreview: false } : candidate
        )
        const groups = state.groupsByWorktree[worktreeId] ?? []
        const group = groups.find((candidate) => candidate.id === tab.groupId)
        if (!group) {
          return {
            unifiedTabsByWorktree: { ...state.unifiedTabsByWorktree, [worktreeId]: tabs }
          }
        }
        const tabOrder = partitionPinnedTabOrder(group.tabOrder, tabs, tabId)
        return {
          unifiedTabsByWorktree: {
            ...state.unifiedTabsByWorktree,
            [worktreeId]: applyTabOrderSortValues(tabs, tabOrder)
          },
          // Why: reconcile derives pin from the TerminalTab, so mirror it there too or a host snapshot recomputes isPinned:false and un-pins during the echo window.
          ...patchTerminalTabRow(state.tabsByWorktree, tabId, { isPinned: true }),
          groupsByWorktree: {
            ...state.groupsByWorktree,
            [worktreeId]: updateGroup(groups, { ...group, tabOrder })
          }
        }
      })
      mirrorTabPinnedToHost(get(), tabId, true)
      if (exists) {
        get().recordFeatureInteraction?.('terminal-tabs')
      }
    },

    unpinTab: (tabId) => {
      const exists = get().getTab(tabId) !== null
      set((state) => {
        const found = findTabAndWorktree(state.unifiedTabsByWorktree, tabId)
        if (!found) {
          return {}
        }
        const { tab, worktreeId } = found
        const tabs = (state.unifiedTabsByWorktree[worktreeId] ?? []).map((candidate) =>
          candidate.id === tabId ? { ...candidate, isPinned: false } : candidate
        )
        const groups = state.groupsByWorktree[worktreeId] ?? []
        const group = groups.find((candidate) => candidate.id === tab.groupId)
        if (!group) {
          return {
            unifiedTabsByWorktree: { ...state.unifiedTabsByWorktree, [worktreeId]: tabs }
          }
        }
        const tabOrder = partitionPinnedTabOrder(group.tabOrder, tabs, tabId)
        return {
          unifiedTabsByWorktree: {
            ...state.unifiedTabsByWorktree,
            [worktreeId]: applyTabOrderSortValues(tabs, tabOrder)
          },
          ...patchTerminalTabRow(state.tabsByWorktree, tabId, { isPinned: false }),
          groupsByWorktree: {
            ...state.groupsByWorktree,
            [worktreeId]: updateGroup(groups, { ...group, tabOrder })
          }
        }
      })
      mirrorTabPinnedToHost(get(), tabId, false)
      if (exists) {
        get().recordFeatureInteraction?.('terminal-tabs')
      }
    }
  }
}
