import { useAppStore } from '@/store'
import { selectFloatingTerminalPanelInputs } from './floating-terminal-panel-inputs'

export function useFloatingTerminalPanelStoreState() {
  const { tabs, groups, unifiedTabs, floatingFiles, expandedPaneByTabId } = useAppStore(
    selectFloatingTerminalPanelInputs
  )
  const createTab = useAppStore((state) => state.createTab)
  const closeTab = useAppStore((state) => state.closeTab)
  const closeFile = useAppStore((state) => state.closeFile)
  const closeUnifiedTab = useAppStore((state) => state.closeUnifiedTab)
  const markFileDirty = useAppStore((state) => state.markFileDirty)
  const activateTab = useAppStore((state) => state.activateTab)
  const setActiveTab = useAppStore((state) => state.setActiveTab)
  const setTabCustomTitle = useAppStore((state) => state.setTabCustomTitle)
  const setTabColor = useAppStore((state) => state.setTabColor)
  const setTabPaneExpanded = useAppStore((state) => state.setTabPaneExpanded)
  const makePreviewFilePermanent = useAppStore((state) => state.makePreviewFilePermanent)
  const pinFile = useAppStore((state) => state.pinFile)
  const openFile = useAppStore((state) => state.openFile)
  const floatingTerminalCwd = useAppStore((state) => state.settings?.floatingTerminalCwd ?? '')
  const generatedTabTitlesEnabled = useAppStore(
    (state) => state.settings?.tabAutoGenerateTitle === true
  )

  return {
    tabs,
    groups,
    unifiedTabs,
    floatingFiles,
    expandedPaneByTabId,
    createTab,
    closeTab,
    closeFile,
    closeUnifiedTab,
    markFileDirty,
    activateTab,
    setActiveTab,
    setTabCustomTitle,
    setTabColor,
    setTabPaneExpanded,
    makePreviewFilePermanent,
    pinFile,
    openFile,
    floatingTerminalCwd,
    generatedTabTitlesEnabled
  }
}

export type FloatingTerminalPanelStoreState = ReturnType<typeof useFloatingTerminalPanelStoreState>
