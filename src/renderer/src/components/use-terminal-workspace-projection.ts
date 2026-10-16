import { useCallback, useEffect, useMemo } from 'react'
import { setForegroundTerminalTabIds } from '@/lib/foreground-terminal-tabs'
import { useTerminalProviderSnapshotCapability } from './terminal/use-terminal-provider-snapshot-capability'
import { getEffectiveLayoutForWorktree as getEffectiveLayout } from './terminal/split-group-mount'
import type { TerminalWorkspaceStoreController } from './use-terminal-workspace-store-bindings'
import { useWorktreeFiles } from './terminal/use-worktree-files'

export function useTerminalWorkspaceProjection(controller: TerminalWorkspaceStoreController) {
  const {
    activeGroupIdByWorktree,
    activeTabId,
    activeTabType,
    activeView,
    activeWorktreeId,
    activityTerminalPortals,
    ensureWorktreeRootGroup,
    groupsByWorktree,
    hydrationSucceeded,
    layoutByWorktree,
    openFiles,
    renderedActiveWorktreeId,
    tabsByWorktree,
    workspaceSessionReady
  } = controller
  const foregroundTerminalTabIds = useMemo(() => {
    const ids = new Set<string>()
    if (activeView === 'terminal' && activeTabType === 'terminal' && activeTabId) {
      ids.add(activeTabId)
    }
    for (const portal of activityTerminalPortals) {
      ids.add(portal.tabId)
    }
    return Array.from(ids)
  }, [activeTabId, activeTabType, activeView, activityTerminalPortals])

  useEffect(() => {
    setForegroundTerminalTabIds(foregroundTerminalTabIds)
    return () => setForegroundTerminalTabIds([])
  }, [foregroundTerminalTabIds])

  const tabs = useMemo(
    () =>
      renderedActiveWorktreeId !== null && Object.hasOwn(tabsByWorktree, renderedActiveWorktreeId)
        ? tabsByWorktree[renderedActiveWorktreeId]
        : [],
    [renderedActiveWorktreeId, tabsByWorktree]
  )
  const terminalProviderSnapshotCapabilityRevision = useTerminalProviderSnapshotCapability(
    workspaceSessionReady && hydrationSucceeded
  )
  const titlebarTabsTarget = document.getElementById('titlebar-tabs')

  useEffect(() => {
    if (!activeWorktreeId) {
      return
    }
    ensureWorktreeRootGroup(activeWorktreeId)
  }, [activeWorktreeId, ensureWorktreeRootGroup])

  const worktreeFiles = useWorktreeFiles(openFiles, renderedActiveWorktreeId)
  const getEffectiveLayoutForWorktree = useCallback(
    (worktreeId: string) =>
      getEffectiveLayout(worktreeId, layoutByWorktree, groupsByWorktree, activeGroupIdByWorktree),
    [activeGroupIdByWorktree, groupsByWorktree, layoutByWorktree]
  )
  const effectiveActiveLayout = renderedActiveWorktreeId
    ? getEffectiveLayoutForWorktree(renderedActiveWorktreeId)
    : undefined
  return {
    foregroundTerminalTabIds,
    tabs,
    titlebarTabsTarget,
    worktreeFiles,
    getEffectiveLayoutForWorktree,
    effectiveActiveLayout,
    terminalProviderSnapshotCapabilityRevision
  }
}

export type TerminalWorkspaceProjectionController = TerminalWorkspaceStoreController &
  ReturnType<typeof useTerminalWorkspaceProjection>
