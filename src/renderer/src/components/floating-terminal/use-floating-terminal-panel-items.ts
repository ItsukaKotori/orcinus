import { useMemo } from 'react'
import { resolveGroupTabFromVisibleId } from '@/components/tab-group/tab-group-visible-id'
import { useTerminalTabColdParking } from '@/components/terminal-pane/use-terminal-tab-cold-parking'
import type { OpenFile } from '@/store/slices/editor'
import type { TerminalTab } from '../../../../shared/terminal-tab-types'
import { resolveUnifiedTabLabel } from '../../../../shared/tab-title-resolution'
import { FLOATING_TERMINAL_WORKTREE_ID } from '../../../../shared/constants'
import type { FloatingTerminalPanelStoreState } from './use-floating-terminal-panel-store-state'

const NO_ACTIVITY_TERMINAL_PORTALS = []

type FloatingTerminalPanelItemsInput = Pick<
  FloatingTerminalPanelStoreState,
  'tabs' | 'groups' | 'unifiedTabs' | 'floatingFiles' | 'generatedTabTitlesEnabled'
> & { open: boolean }

export function useFloatingTerminalPanelItems({
  tabs,
  groups,
  unifiedTabs,
  floatingFiles,
  generatedTabTitlesEnabled,
  open
}: FloatingTerminalPanelItemsInput) {
  const activeGroup = useMemo(
    () =>
      groups.find((group) => group.activeTabId != null) ??
      (unifiedTabs[0]
        ? (groups.find((group) => group.id === unifiedTabs[0].groupId) ?? null)
        : null),
    [groups, unifiedTabs]
  )
  const groupTabs = useMemo(
    () => (activeGroup ? unifiedTabs.filter((tab) => tab.groupId === activeGroup.id) : unifiedTabs),
    [activeGroup, unifiedTabs]
  )
  const activeTab = useMemo(
    () =>
      (activeGroup?.activeTabId
        ? groupTabs.find((tab) => tab.id === activeGroup.activeTabId)
        : null) ??
      groupTabs[0] ??
      null,
    [activeGroup, groupTabs]
  )
  const activeTerminalId = activeTab?.contentType === 'terminal' ? activeTab.entityId : null
  const activeEditorUnifiedId =
    activeTab && activeTab.contentType !== 'terminal' ? activeTab.id : null
  const activeEditorFileId =
    activeTab && activeTab.contentType !== 'terminal' ? activeTab.entityId : null
  const terminalTabById = useMemo(() => new Map(tabs.map((tab) => [tab.id, tab])), [tabs])
  const terminalAssignments = useMemo(() => {
    const assignments = new Map<string, { groupId: string; isActiveInGroup: boolean }>()
    for (const tab of unifiedTabs) {
      if (tab.contentType === 'terminal') {
        assignments.set(tab.entityId, {
          groupId: tab.groupId,
          isActiveInGroup: tab.entityId === activeTerminalId
        })
      }
    }
    return assignments
  }, [activeTerminalId, unifiedTabs])
  const parkedTerminalTabIds = useTerminalTabColdParking({
    worktreeId: FLOATING_TERMINAL_WORKTREE_ID,
    terminalTabs: tabs,
    assignments: terminalAssignments,
    activeTerminalTabId: activeTerminalId,
    isWorktreeActive: open,
    coldParkTerminalPanes: false,
    shouldMeasureHiddenWorktree: false,
    activityTerminalPortals: NO_ACTIVITY_TERMINAL_PORTALS
  })
  const terminalItems = useMemo<(TerminalTab & { unifiedTabId: string })[]>(
    () =>
      groupTabs
        .filter((tab) => tab.contentType === 'terminal')
        .flatMap((tab): (TerminalTab & { unifiedTabId: string })[] => {
          const terminalTab = terminalTabById.get(tab.entityId)
          if (!terminalTab) {
            return []
          }
          return [
            {
              ...terminalTab,
              unifiedTabId: tab.id,
              title: resolveUnifiedTabLabel(
                {
                  ...tab,
                  quickCommandLabel: tab.quickCommandLabel ?? terminalTab.quickCommandLabel,
                  generatedLabel: tab.generatedLabel ?? terminalTab.generatedTitle
                },
                generatedTabTitlesEnabled,
                tab.label
              ),
              generatedTitle: terminalTab.generatedTitle ?? tab.generatedLabel ?? null,
              quickCommandLabel: terminalTab.quickCommandLabel ?? tab.quickCommandLabel ?? null,
              customTitle: tab.customLabel ?? terminalTab.customTitle,
              color: tab.color ?? terminalTab.color
            }
          ]
        }),
    [generatedTabTitlesEnabled, groupTabs, terminalTabById]
  )
  const editorItems = useMemo(
    () =>
      groupTabs
        .filter((tab) => tab.contentType !== 'terminal')
        .map((tab) => {
          const file = floatingFiles.find((candidate) => candidate.id === tab.entityId)
          return file ? { ...file, tabId: tab.id } : null
        })
        .filter((file): file is OpenFile & { tabId: string } => file !== null),
    [floatingFiles, groupTabs]
  )
  const hasVisibleFloatingTabs = terminalItems.length > 0 || editorItems.length > 0
  const visibleFloatingItemCount = terminalItems.length + editorItems.length
  const activeClosableTab = hasVisibleFloatingTabs ? activeTab : null
  const tabBarOrder = useMemo(
    () =>
      (activeGroup?.tabOrder ?? []).map((tabId) => {
        const tab = groupTabs.find((candidate) => candidate.id === tabId)
        return tab?.contentType === 'terminal' ? tab.entityId : tabId
      }),
    [activeGroup, groupTabs]
  )
  const visibleFloatingTabOrder = useMemo(
    () =>
      tabBarOrder.filter((visibleId) => {
        const tab = resolveGroupTabFromVisibleId(groupTabs, visibleId)
        if (!tab) {
          return false
        }
        if (tab.contentType === 'terminal') {
          return terminalItems.some((item) => item.unifiedTabId === tab.id)
        }
        return editorItems.some((item) => item.tabId === tab.id)
      }),
    [editorItems, groupTabs, tabBarOrder, terminalItems]
  )
  const activeEditorFile = activeEditorFileId
    ? (floatingFiles.find((file) => file.id === activeEditorFileId) ?? null)
    : null
  const activeTabType: 'terminal' | 'editor' =
    activeTab?.contentType === 'terminal' ? 'terminal' : 'editor'

  return {
    activeGroup,
    groupTabs,
    activeTab,
    activeTerminalId,
    activeEditorUnifiedId,
    parkedTerminalTabIds,
    terminalItems,
    editorItems,
    hasVisibleFloatingTabs,
    visibleFloatingItemCount,
    activeClosableTab,
    tabBarOrder,
    visibleFloatingTabOrder,
    activeEditorFile,
    activeTabType
  }
}

export type FloatingTerminalPanelItems = ReturnType<typeof useFloatingTerminalPanelItems>
