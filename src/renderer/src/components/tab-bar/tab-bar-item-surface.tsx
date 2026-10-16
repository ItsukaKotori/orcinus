import React from 'react'
import { resolveTerminalTabTitle } from '../../../../shared/tab-title-resolution'
import type { TerminalTab } from '../../../../shared/terminal-tab-types'
import type { TuiAgent } from '../../../../shared/tui-agent'
import { isAgentSessionHandleProvider } from '../../../../shared/agent-session-provider-handle'
import SortableTab from './SortableTab'
import EditorFileTab from './EditorFileTab'
import { PluginTabLeadingIcon } from './PluginTabLeadingIcon'
import type { DropIndicator } from './drop-indicator'
import type { TabDragItemData } from '../tab-group/useTabDragSplit'
import { getTabDragLabel, type TabBarItem } from './tab-bar-item-model'
import type { TabBarProps } from './tab-bar-props'
import type { TabBarRuntimeModel } from './use-tab-bar-runtime-model'

export function renderTabBarItems({
  items,
  props,
  runtime,
  dropIndicatorByVisibleId,
  includeTopTabBorder,
  togglePinned
}: {
  items: TabBarItem[]
  props: TabBarProps
  runtime: TabBarRuntimeModel
  dropIndicatorByVisibleId: Map<string, DropIndicator>
  includeTopTabBorder: boolean
  togglePinned: (item: TabBarItem) => void
}): React.ReactNode[] {
  const {
    worktreeId,
    activeTabId,
    activeFileId,
    activeTabType,
    expandedPaneByTabId,
    onActivate,
    onClose,
    onCloseOthers,
    onCloseToRight,
    onCloseToLeft,
    onSetCustomTitle,
    onSetTabColor,
    onTogglePaneExpand,
    onActivateFile,
    onCloseFile,
    onActivateAgentSession,
    onActivatePlugin,
    onCloseAllFiles,
    onMakePreviewFilePermanent
  } = props
  const { resolvedGroupId, generatedTabTitlesEnabled, statusByRelativePath } = runtime

  // Why: this is the strip's single activation fan-out for every row kind.
  function activateRealTab<TArg>(activate: ((arg: TArg) => void) | undefined): (arg: TArg) => void {
    return (arg) => {
      activate?.(arg)
    }
  }

  return items.map((item, index) => {
    const dragData: TabDragItemData = {
      kind: 'tab',
      worktreeId,
      groupId: resolvedGroupId,
      unifiedTabId: item.unifiedTabId,
      visibleTabId: item.id,
      tabType: item.type,
      label: getTabDragLabel(item, generatedTabTitlesEnabled),
      iconPath: item.type === 'editor' ? item.data.filePath : undefined,
      color:
        item.type === 'terminal' || item.type === 'plugin' ? (item.data.color ?? null) : null
    }
    if (item.type === 'terminal') {
      const terminalTab = {
        ...item.data,
        title: resolveTerminalTabTitle(item.data, generatedTabTitlesEnabled, item.data.title)
      }
      return (
        <SortableTab
          key={item.id}
          tab={terminalTab}
          unifiedTabId={item.unifiedTabId}
          groupId={resolvedGroupId}
          tabCount={items.length}
          hasTabsToRight={index < items.length - 1}
          hasTabsToLeft={index > 0}
          isActive={activeTabType === 'terminal' && item.id === activeTabId}
          isPinned={item.isPinned}
          isExpanded={expandedPaneByTabId[item.id] === true}
          onActivate={activateRealTab(onActivate)}
          onClose={onClose}
          onCloseOthers={onCloseOthers}
          onCloseToRight={onCloseToRight}
          onCloseToLeft={onCloseToLeft}
          onSetCustomTitle={onSetCustomTitle}
          onSetTabColor={onSetTabColor}
          onTogglePin={() => togglePinned(item)}
          onToggleExpand={onTogglePaneExpand}
          dragData={dragData}
          dropIndicator={dropIndicatorByVisibleId.get(item.id) ?? null}
          includeTopTabBorder={includeTopTabBorder}
        />
      )
    }
    if (item.type === 'agent-session') {
      const structuredTab: TerminalTab = {
        id: item.id,
        ptyId: null,
        worktreeId,
        title: item.data.label,
        customTitle: item.data.customLabel,
        color: item.data.color,
        sortOrder: item.data.sortOrder,
        createdAt: item.data.createdAt,
        ...(isAgentSessionHandleProvider(item.data.agentSessionAgent)
          ? { launchAgent: item.data.agentSessionAgent as TuiAgent }
          : {})
      }
      return (
        <SortableTab
          key={item.id}
          tab={structuredTab}
          unifiedTabId={item.unifiedTabId}
          groupId={resolvedGroupId}
          tabCount={items.length}
          hasTabsToRight={index < items.length - 1}
          hasTabsToLeft={index > 0}
          isActive={activeTabType === 'agent-session' && item.id === activeTabId}
          isPinned={item.isPinned}
          isExpanded={false}
          onActivate={() => activateRealTab(onActivateAgentSession)(item.id)}
          onClose={() => onClose(item.id)}
          onCloseOthers={() => onCloseOthers(item.id)}
          onCloseToRight={() => onCloseToRight(item.id)}
          onCloseToLeft={() => onCloseToLeft(item.id)}
          onSetCustomTitle={onSetCustomTitle}
          onSetTabColor={onSetTabColor}
          onTogglePin={() => togglePinned(item)}
          onToggleExpand={() => {}}
          canSplitTerminal={false}
          dragData={dragData}
          dropIndicator={dropIndicatorByVisibleId.get(item.id) ?? null}
          includeTopTabBorder={includeTopTabBorder}
        />
      )
    }
    if (item.type === 'plugin') {
      // Plugin tabs reuse the terminal row chrome (drag/close/pin/context menu)
      // through a synthesized runtime tab; the unified id is the visible id.
      const pluginRowTab: TerminalTab = {
        id: item.id,
        ptyId: null,
        worktreeId,
        title: item.data.label,
        customTitle: item.data.customLabel,
        color: item.data.color,
        sortOrder: item.data.sortOrder,
        createdAt: item.data.createdAt
      }
      return (
        <SortableTab
          key={item.id}
          tab={pluginRowTab}
          unifiedTabId={item.unifiedTabId}
          groupId={resolvedGroupId}
          tabCount={items.length}
          hasTabsToRight={index < items.length - 1}
          hasTabsToLeft={index > 0}
          isActive={activeTabType === 'plugin' && item.id === activeTabId}
          isPinned={item.isPinned}
          isExpanded={false}
          leadingIcon={<PluginTabLeadingIcon binding={item.data.pluginTab} />}
          onActivate={() => activateRealTab(onActivatePlugin)(item.id)}
          onClose={() => onCloseFile?.(item.id)}
          onCloseOthers={() => onCloseOthers(item.id)}
          onCloseToRight={() => onCloseToRight(item.id)}
          onCloseToLeft={() => onCloseToLeft(item.id)}
          onSetCustomTitle={onSetCustomTitle}
          onSetTabColor={onSetTabColor}
          onTogglePin={() => togglePinned(item)}
          onToggleExpand={() => {}}
          canSplitTerminal={false}
          dragData={dragData}
          dropIndicator={dropIndicatorByVisibleId.get(item.id) ?? null}
          includeTopTabBorder={includeTopTabBorder}
        />
      )
    }
    return (
      <EditorFileTab
        key={item.id}
        file={item.data}
        isActive={
          activeTabType === 'editor' && activeFileId === item.id
        }
        isPinned={item.isPinned}
        hasTabsToRight={index < items.length - 1}
        hasTabsToLeft={index > 0}
        tabCount={items.length}
        statusByRelativePath={statusByRelativePath}
        onActivate={() => activateRealTab(onActivateFile)(item.id)}
        onClose={() => onCloseFile?.(item.id)}
        onCloseOthers={() => onCloseOthers(item.id)}
        onCloseToRight={() => onCloseToRight(item.id)}
        onCloseToLeft={() => onCloseToLeft(item.id)}
        onCloseAll={() => onCloseAllFiles?.()}
        onMakePermanent={() => onMakePreviewFilePermanent?.(item.data.id, item.data.tabId)}
        onTogglePin={() => togglePinned(item)}
        dragData={dragData}
        dropIndicator={dropIndicatorByVisibleId.get(item.id) ?? null}
        includeTopTabBorder={includeTopTabBorder}
      />
    )
  })
}
