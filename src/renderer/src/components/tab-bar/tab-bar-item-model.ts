import type { Tab, WorkspaceVisibleTabType } from '../../../../shared/tab-types'
import type { TerminalTab } from '../../../../shared/terminal-tab-types'
import { resolveTerminalTabTitle } from '../../../../shared/tab-title-resolution'
import type { OpenFile } from '../../store/slices/editor'
import { getEditorDisplayLabel } from '@/components/editor/editor-labels'
import { normalizeRelativePath } from '@/lib/path'
import type { DropIndicator } from './drop-indicator'
import { reconcileTabOrder } from './reconcile-order'
import { resolveTabIndicatorEdges } from '../tab-group/tab-insertion'
import type { HoveredTabInsertion } from '../tab-group/useTabDragSplit'

export type TabBarItem =
  | {
      type: 'terminal'
      id: string
      unifiedTabId: string
      isPinned: boolean
      data: TerminalTab & { unifiedTabId?: string }
    }
  | {
      type: 'editor'
      id: string
      unifiedTabId: string
      isPinned: boolean
      data: OpenFile & { tabId?: string }
    }
  | {
      type: 'agent-session'
      id: string
      unifiedTabId: string
      isPinned: boolean
      data: Tab & { contentType: 'agent-session' }
    }
  | {
      type: 'plugin'
      id: string
      unifiedTabId: string
      isPinned: boolean
      data: Tab & { contentType: 'plugin' }
    }
  | {
      type: 'git-log'
      id: string
      unifiedTabId: string
      isPinned: boolean
      data: Tab & { contentType: 'git-log' }
    }

export function getTabDragLabel(item: TabBarItem, generatedTitlesEnabled: boolean): string {
  if (item.type === 'terminal') {
    return resolveTerminalTabTitle(item.data, generatedTitlesEnabled, item.data.title)
  }
  if (item.type === 'agent-session') {
    return item.data.label
  }
  if (item.type === 'plugin') {
    return item.data.customLabel ?? item.data.label
  }
  if (item.type === 'git-log') {
    return item.data.customLabel ?? item.data.label
  }
  return getEditorDisplayLabel(item.data)
}

export function getTabLayoutSignature(
  item: TabBarItem,
  {
    generatedTitlesEnabled,
    isExpanded,
    status
  }: {
    generatedTitlesEnabled: boolean
    isExpanded: boolean
    status?: string | null
  }
): string {
  const label = getTabDragLabel(item, generatedTitlesEnabled)
  if (item.type === 'terminal') {
    return `${item.type}:${item.id}:${item.isPinned}:${isExpanded}:${Boolean(item.data.color)}:${label}`
  }
  if (item.type === 'editor') {
    return `${item.type}:${item.id}:${item.isPinned}:${item.data.isDirty}:${item.data.isPreview}:${item.data.externalMutation ?? ''}:${status ?? ''}:${label}`
  }
  return `${item.type}:${item.id}:${item.isPinned}:${label}`
}

export function createUnifiedTabLookup(tabs: readonly Tab[], groupId: string): Map<string, Tab> {
  const lookup = new Map<string, Tab>()
  for (const tab of tabs) {
    if (tab.groupId !== groupId) {
      continue
    }
    lookup.set(tab.id, tab)
    if (tab.contentType === 'terminal') {
      lookup.set(tab.entityId, tab)
    }
  }
  return lookup
}

export function buildOrderedTabItems({
  tabBarOrder,
  terminalIds,
  editorFileIds,
  agentSessionTabIds,
  pluginTabIds,
  gitLogTabIds,
  terminalMap,
  editorMap,
  agentSessionMap,
  pluginTabMap,
  gitLogTabMap,
  unifiedTabByVisibleId
}: {
  tabBarOrder?: string[]
  terminalIds: string[]
  editorFileIds: string[]
  agentSessionTabIds: string[]
  pluginTabIds: string[]
  gitLogTabIds: string[]
  terminalMap: Map<string, TerminalTab & { unifiedTabId?: string }>
  editorMap: Map<string, OpenFile & { tabId?: string }>
  agentSessionMap: Map<string, Tab & { contentType: 'agent-session' }>
  pluginTabMap: Map<string, Tab & { contentType: 'plugin' }>
  gitLogTabMap: Map<string, Tab & { contentType: 'git-log' }>
  unifiedTabByVisibleId: Map<string, Tab>
}): TabBarItem[] {
  const ids = reconcileTabOrder(
    tabBarOrder,
    terminalIds,
    editorFileIds,
    agentSessionTabIds,
    pluginTabIds,
    gitLogTabIds
  )
  const items: TabBarItem[] = []
  for (const id of ids) {
    const terminal = terminalMap.get(id)
    if (terminal) {
      const unifiedTab = unifiedTabByVisibleId.get(id)
      items.push({
        type: 'terminal',
        id,
        unifiedTabId: terminal.unifiedTabId ?? unifiedTab?.id ?? terminal.id,
        isPinned: unifiedTab?.isPinned === true,
        data: terminal
      })
      continue
    }
    const file = editorMap.get(id)
    if (file) {
      const unifiedTab = unifiedTabByVisibleId.get(id) ?? unifiedTabByVisibleId.get(file.id)
      items.push({
        type: 'editor',
        id,
        unifiedTabId: file.tabId ?? unifiedTab?.id ?? file.id,
        isPinned: unifiedTab?.isPinned === true,
        data: file
      })
      continue
    }
    const agentSession = agentSessionMap.get(id)
    if (agentSession) {
      items.push({
        type: 'agent-session',
        id,
        unifiedTabId: agentSession.id,
        isPinned: agentSession.isPinned === true,
        data: agentSession
      })
      continue
    }
    const pluginTab = pluginTabMap.get(id)
    if (pluginTab) {
      const unifiedTab = unifiedTabByVisibleId.get(id)
      items.push({
        type: 'plugin',
        id,
        unifiedTabId: pluginTab.id,
        isPinned: unifiedTab?.isPinned === true,
        data: pluginTab
      })
      continue
    }
    const gitLogTab = gitLogTabMap.get(id)
    if (gitLogTab) {
      const unifiedTab = unifiedTabByVisibleId.get(id)
      items.push({
        type: 'git-log',
        id,
        unifiedTabId: gitLogTab.id,
        isPinned: unifiedTab?.isPinned === true,
        data: gitLogTab
      })
    }
  }
  return items
}

export function buildTabDropIndicators(
  items: readonly TabBarItem[],
  insertion: HoveredTabInsertion | null
): Map<string, DropIndicator> {
  const indicators = new Map<string, DropIndicator>()
  for (const edge of resolveTabIndicatorEdges(
    items.map((item) => item.id),
    insertion
  )) {
    indicators.set(edge.visibleTabId, edge.side)
  }
  return indicators
}

export function findActiveVisibleTabId(
  items: readonly TabBarItem[],
  active: {
    activeTabId: string | null
    activeFileId?: string | null
    activeTabType?: WorkspaceVisibleTabType
  }
): string | null {
  const activeItem = items.find((item) => {
    if (item.type === 'terminal') {
      return active.activeTabType === 'terminal' && item.id === active.activeTabId
    }
    if (item.type === 'agent-session') {
      // Reachable only from TabGroupPanel, which passes the structured tab's own id; the store's
      // `activeTabId` names a background terminal here.
      return active.activeTabType === 'agent-session' && item.id === active.activeTabId
    }
    if (item.type === 'plugin') {
      // Plugin rows are self-backed: the visible id is the unified tab id.
      return active.activeTabType === 'plugin' && item.id === active.activeTabId
    }
    if (item.type === 'git-log') {
      // Git Log rows are self-backed like plugin rows.
      return active.activeTabType === 'git-log' && item.id === active.activeTabId
    }
    return active.activeTabType === 'editor' && active.activeFileId === item.id
  })
  return activeItem?.id ?? null
}

export function buildTabStripLayoutKey(
  items: readonly TabBarItem[],
  generatedTitlesEnabled: boolean,
  expandedPaneByTabId: Record<string, boolean>,
  statusByRelativePath: Map<string, string>
): string {
  return items
    .map((item) =>
      getTabLayoutSignature(item, {
        generatedTitlesEnabled,
        isExpanded: expandedPaneByTabId[item.id] === true,
        status:
          item.type === 'editor'
            ? (statusByRelativePath.get(normalizeRelativePath(item.data.relativePath)) ?? null)
            : null
      })
    )
    .join('\u001f')
}
