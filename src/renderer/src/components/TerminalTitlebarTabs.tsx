import { createPortal } from 'react-dom'
import type { TerminalTab } from '../../../shared/terminal-tab-types'
import { useAppStore } from '../store'
import TabBar from './tab-bar/TabBar'
import type { TerminalController } from './use-terminal-controller'

const EMPTY_TERMINAL_TABS: TerminalTab[] = []

// Why: keeps title-only tab updates a leaf subscription so the Terminal root,
// which reads the topology projection, does not re-render on every rename.
function LiveTerminalTabBar(
  props: Omit<React.ComponentProps<typeof TabBar>, 'tabs'>
): React.JSX.Element {
  const tabs = useAppStore((state) => state.tabsByWorktree[props.worktreeId] ?? EMPTY_TERMINAL_TABS)
  return <TabBar {...props} tabs={tabs} />
}

export function TerminalTitlebarTabs({
  controller
}: {
  controller: TerminalController
}): React.JSX.Element | null {
  const {
    activeFileId,
    activeTabId,
    activeTabType,
    effectiveActiveLayout,
    expandedPaneByTabId,
    handleActivateTab,
    handleCloseAllFiles,
    handleCloseFile,
    handleCloseOthers,
    handleCloseTab,
    handleCloseTabsToLeft,
    handleCloseTabsToRight,
    handleNewFile,
    handleNewTab,
    handleOpenEntry,
    handleTogglePaneExpand,
    makePreviewFilePermanent,
    pinFile,
    renderedActiveWorktreeId,
    setActiveFile,
    setActiveTabType,
    setTabColor,
    setTabCustomTitle,
    tabBarOrder,
    titlebarTabsTarget,
    worktreeFiles
  } = controller
  if (!renderedActiveWorktreeId || effectiveActiveLayout || !titlebarTabsTarget) {
    return null
  }
  return createPortal(
    <LiveTerminalTabBar
      activeTabId={activeTabId}
      worktreeId={renderedActiveWorktreeId}
      onActivate={handleActivateTab}
      onClose={handleCloseTab}
      onCloseOthers={handleCloseOthers}
      onCloseToRight={handleCloseTabsToRight}
      onCloseToLeft={handleCloseTabsToLeft}
      onNewTerminalTab={() => handleNewTab()}
      onNewTerminalWithShell={handleNewTab}
      onOpenEntry={handleOpenEntry}
      onNewFileTab={handleNewFile}
      onSetCustomTitle={setTabCustomTitle}
      onSetTabColor={setTabColor}
      expandedPaneByTabId={expandedPaneByTabId}
      onTogglePaneExpand={handleTogglePaneExpand}
      editorFiles={worktreeFiles}
      activeFileId={activeFileId}
      activeTabType={activeTabType}
      onActivateFile={(fileId) => {
        setActiveFile(fileId)
        setActiveTabType('editor')
      }}
      onCloseFile={handleCloseFile}
      onCloseAllFiles={handleCloseAllFiles}
      onMakePreviewFilePermanent={makePreviewFilePermanent}
      onPinFile={pinFile}
      tabBarOrder={tabBarOrder}
    />,
    titlebarTabsTarget
  )
}
