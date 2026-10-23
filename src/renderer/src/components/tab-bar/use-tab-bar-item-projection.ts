import { useMemo } from 'react'
import type { GitFileStatus } from '../../../../shared/git-status-types'
import type { Tab } from '../../../../shared/tab-types'
import type { TabBarProps } from './tab-bar-props'
import {
  buildOrderedTabItems,
  buildTabDropIndicators,
  buildTabStripLayoutKey,
  findActiveVisibleTabId,
  type TabBarItem
} from './tab-bar-item-model'
import type { DropIndicator } from './drop-indicator'

export type TabBarItemProjection = {
  orderedItems: TabBarItem[]
  sortableIds: string[]
  dropIndicatorByVisibleId: Map<string, DropIndicator>
  activeVisibleTabId: string | null
  tabStripLayoutKey: string
}

export function useTabBarItemProjection({
  props,
  resolvedGroupId,
  unifiedTabByVisibleId,
  generatedTabTitlesEnabled,
  statusByRelativePath
}: {
  props: TabBarProps
  resolvedGroupId: string
  unifiedTabByVisibleId: Map<string, Tab>
  generatedTabTitlesEnabled: boolean
  statusByRelativePath: Map<string, GitFileStatus>
}): TabBarItemProjection {
  const {
    tabs,
    editorFiles,
    agentSessionTabs,
    gitLogTabs,
    tabBarOrder,
    hoveredTabInsertion,
    activeTabId,
    activeFileId,
    activeTabType,
    expandedPaneByTabId
  } = props
  const terminalMap = useMemo(() => new Map(tabs.map((tab) => [tab.id, tab])), [tabs])
  const editorMap = useMemo(
    () => new Map((editorFiles ?? []).map((file) => [file.tabId ?? file.id, file])),
    [editorFiles]
  )
  const agentSessionMap = useMemo(
    () => new Map((agentSessionTabs ?? []).map((tab) => [tab.id, tab])),
    [agentSessionTabs]
  )
  const gitLogTabMap = useMemo(
    () => new Map((gitLogTabs ?? []).map((tab) => [tab.id, tab])),
    [gitLogTabs]
  )
  const terminalIds = useMemo(() => tabs.map((tab) => tab.id), [tabs])
  const editorFileIds = useMemo(
    () => editorFiles?.map((file) => file.tabId ?? file.id) ?? [],
    [editorFiles]
  )
  const agentSessionTabIds = useMemo(
    () => agentSessionTabs?.map((tab) => tab.id) ?? [],
    [agentSessionTabs]
  )
  const gitLogTabIds = useMemo(() => gitLogTabs?.map((tab) => tab.id) ?? [], [gitLogTabs])
  const orderedItems = useMemo(
    () =>
      buildOrderedTabItems({
        tabBarOrder,
        terminalIds,
        editorFileIds,
        agentSessionTabIds,
        gitLogTabIds,
        terminalMap,
        editorMap,
        agentSessionMap,
        gitLogTabMap,
        unifiedTabByVisibleId
      }),
    [
      tabBarOrder,
      terminalIds,
      editorFileIds,
      agentSessionTabIds,
      gitLogTabIds,
      terminalMap,
      editorMap,
      agentSessionMap,
      gitLogTabMap,
      unifiedTabByVisibleId
    ]
  )
  const sortableIds = useMemo(() => orderedItems.map((item) => item.id), [orderedItems])
  const activeIndicator =
    hoveredTabInsertion?.groupId === resolvedGroupId ? hoveredTabInsertion : null
  const dropIndicatorByVisibleId = useMemo(
    () => buildTabDropIndicators(orderedItems, activeIndicator),
    [activeIndicator, orderedItems]
  )
  const activeVisibleTabId = useMemo(
    () =>
      findActiveVisibleTabId(orderedItems, {
        activeTabId,
        activeFileId,
        activeTabType
      }),
    [activeFileId, activeTabId, activeTabType, orderedItems]
  )
  const tabStripLayoutKey = useMemo(
    () =>
      buildTabStripLayoutKey(
        orderedItems,
        generatedTabTitlesEnabled,
        expandedPaneByTabId,
        statusByRelativePath
      ),
    [expandedPaneByTabId, generatedTabTitlesEnabled, orderedItems, statusByRelativePath]
  )

  return {
    orderedItems,
    sortableIds,
    dropIndicatorByVisibleId,
    activeVisibleTabId,
    tabStripLayoutKey
  }
}
