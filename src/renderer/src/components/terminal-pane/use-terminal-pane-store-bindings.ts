import { useMemo, useRef, useState } from 'react'
import { useAppStore } from '../../store'
import { useLinkRoutingPreferenceDialog } from '@/components/link-routing-preference-dialog'
import { isWindowsUserAgent } from './pane-helpers'
import type { SessionRestoredBannerReason } from './session-restored-banner-pane-state'
import { useTerminalPaneStoreActions } from './use-terminal-pane-store-actions'
import { getCachedTerminalTabForWorktree } from './terminal-tab-lookup'
import { selectTerminalTabAgentTypesByLeaf } from './terminal-tab-agent-type-index'
import { collectLeafIdsInOrder, EMPTY_LAYOUT } from './layout-serialization'
import { sanitizeTerminalLayoutPaneTitles } from '@/lib/terminal-pane-title-sanitization'
import { getCachedUnifiedTerminalTabForWorktree } from './terminal-unified-tab-lookup'
import type { TerminalPaneTitleController } from './use-terminal-pane-title-state'

const EMPTY_RUNTIME_PANE_TITLES: Readonly<Record<number, string>> = Object.freeze({})

export function useTerminalPaneStoreBindings(controller: TerminalPaneTitleController) {
  const { isVisible, tabId, worktreeId } = controller
  const {
    clearCodexRestartNotice,
    clearRuntimePaneTitle,
    clearTabPtyId,
    clearTerminalPaneUnread,
    clearTerminalTabUnread,
    clearWorktreeUnread,
    consumePendingCodexPaneRestart,
    consumeTabIssueCommandSplit,
    consumeTabSetupSplit,
    consumeTabStartupCommand,
    markTerminalPaneUnread,
    markTerminalTabUnread,
    markWorktreeUnread,
    openSpacePage,
    refreshWorkspaceSpace,
    setRuntimePaneTitle,
    setTabCanExpandPane,
    setTabLayout,
    setTabPaneExpanded,
    suppressPtyExit,
    updateSettings,
    updateTabPtyId,
    updateTabTitle
  } = useTerminalPaneStoreActions()
  const pendingCodexPaneRestartIds = useAppStore((store) => store.pendingCodexPaneRestartIds)
  const runtimePaneTitlesByPaneId = useAppStore(
    (store) => store.runtimePaneTitlesByTabId[tabId] ?? EMPTY_RUNTIME_PANE_TITLES
  )
  const tabAgentTypeByLeaf = useAppStore((store) =>
    selectTerminalTabAgentTypesByLeaf(
      store.agentStatusByPaneKey,
      tabId,
      store.paneForegroundAgentByPaneKey
    )
  )
  const savedLayout = useAppStore((store) => store.terminalLayoutsByTabId[tabId] ?? EMPTY_LAYOUT)
  const terminalTab = useAppStore((store) =>
    getCachedTerminalTabForWorktree(store.tabsByWorktree, worktreeId, tabId)
  )
  const unifiedTabLabel = useAppStore(
    (store) =>
      getCachedUnifiedTerminalTabForWorktree(store.unifiedTabsByWorktree, worktreeId, tabId)?.label
  )
  const restoredLayout = useMemo(
    () => (terminalTab ? sanitizeTerminalLayoutPaneTitles(savedLayout, terminalTab) : savedLayout),
    [savedLayout, terminalTab]
  )
  const expectedLayoutLeafIds = useMemo(
    () => collectLeafIdsInOrder(restoredLayout.root),
    [restoredLayout.root]
  )
  const expectedLayoutLeafIdsAttr =
    expectedLayoutLeafIds.length > 0 ? expectedLayoutLeafIds.join(' ') : undefined
  const initialLayoutRef = useRef(restoredLayout)
  const settings = useAppStore((store) => store.settings)
  const requestLinkRoutingPreference = useLinkRoutingPreferenceDialog()
  const keybindings = useAppStore((store) => store.keybindings)
  const rightClickToPaste = settings?.terminalRightClickToPaste ?? isWindowsUserAgent()
  const forceBracketedMultilineTextPaste = isWindowsUserAgent()
  const [startup] = useState(() => useAppStore.getState().pendingStartupByTabId[tabId])
  const [shouldMeasureHiddenStartup, setShouldMeasureHiddenStartup] = useState(
    () => startup !== undefined && !isVisible
  )
  const [sessionRestoredBannerPaneIds, setSessionRestoredBannerPaneIds] = useState<
    Map<number, SessionRestoredBannerReason>
  >(() => new Map())
  const [setupSplit] = useState(() => useAppStore.getState().pendingSetupSplitByTabId[tabId])
  const [issueCommandSplit] = useState(
    () => useAppStore.getState().pendingIssueCommandSplitByTabId[tabId]
  )

  return {
    setTabLayout,
    setTabPaneExpanded,
    setTabCanExpandPane,
    suppressPtyExit,
    clearCodexRestartNotice,
    consumePendingCodexPaneRestart,
    pendingCodexPaneRestartIds,
    savedLayout,
    terminalTab,
    restoredLayout,
    expectedLayoutLeafIds,
    expectedLayoutLeafIdsAttr,
    runtimePaneTitlesByPaneId,
    tabAgentTypeByLeaf,
    unifiedTabLabel,
    initialLayoutRef,
    updateTabTitle,
    setRuntimePaneTitle,
    clearRuntimePaneTitle,
    updateTabPtyId,
    clearTabPtyId,
    markWorktreeUnread,
    markTerminalTabUnread,
    markTerminalPaneUnread,
    clearWorktreeUnread,
    clearTerminalTabUnread,
    clearTerminalPaneUnread,
    openSpacePage,
    refreshWorkspaceSpace,
    settings,
    updateSettings,
    requestLinkRoutingPreference,
    keybindings,
    rightClickToPaste,
    forceBracketedMultilineTextPaste,
    startup,
    shouldMeasureHiddenStartup,
    setShouldMeasureHiddenStartup,
    sessionRestoredBannerPaneIds,
    setSessionRestoredBannerPaneIds,
    consumeTabStartupCommand,
    setupSplit,
    consumeTabSetupSplit,
    issueCommandSplit,
    consumeTabIssueCommandSplit
  }
}

export type TerminalPaneStoreController = TerminalPaneTitleController &
  ReturnType<typeof useTerminalPaneStoreBindings>
