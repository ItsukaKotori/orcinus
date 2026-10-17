import { useCallback } from 'react'
import { toast } from 'sonner'
import type { TuiAgent } from '../../../shared/tui-agent'
import { useAppStore } from '../store'
import { focusTerminalTabSurface } from '@/lib/focus-terminal-tab-surface'
import {
  createWebRuntimeSessionTerminal,
  isWebRuntimeSessionActive
} from '@/runtime/web-runtime-session'
import { launchAgentInNewTab } from '@/lib/launch-agent-in-new-tab'
import { openTabBarEntry, type TabCreateEntryArgs } from './tab-bar/tab-create-entry-action'
import { translate } from '@/i18n/i18n'
import { getActiveWorktreeRuntimeEnvironmentId } from './terminal-workspace-model'
import type { TerminalColdActivationController } from './terminal-cold-activation'

export function useTerminalCreateActions(controller: TerminalColdActivationController) {
  const {
    activeWorktreeId,
    createTab,
    openNewMarkdownInActiveWorkspace,
    openNewTerminalTabInActiveWorkspace,
    setActiveTabType,
    setTabBarOrder
  } = controller
  const handleNewTab = useCallback(
    (shellOverride?: string) => {
      if (!activeWorktreeId) {
        return
      }
      const targetGroupId =
        useAppStore.getState().activeGroupIdByWorktree[activeWorktreeId] ??
        useAppStore.getState().groupsByWorktree[activeWorktreeId]?.[0]?.id
      const runtimeEnvironmentId = getActiveWorktreeRuntimeEnvironmentId(activeWorktreeId)
      if (isWebRuntimeSessionActive(runtimeEnvironmentId)) {
        void createWebRuntimeSessionTerminal({
          worktreeId: activeWorktreeId,
          environmentId: runtimeEnvironmentId,
          targetGroupId,
          command: shellOverride,
          activate: true
        })
        return
      }
      if (!shellOverride && targetGroupId) {
        void openNewTerminalTabInActiveWorkspace(targetGroupId)
        return
      }
      const newTab = createTab(activeWorktreeId, undefined, shellOverride)
      setActiveTabType('terminal')
      const state = useAppStore.getState()
      const currentTerminals = state.tabsByWorktree[activeWorktreeId] ?? []
      const currentEditors = state.openFiles.filter((file) => file.worktreeId === activeWorktreeId)
      const stored = state.tabBarOrderByWorktree[activeWorktreeId]
      const termIds = currentTerminals.map((tab) => tab.id)
      const editorIds = currentEditors.map((file) => file.id)
      const validIds = new Set([...termIds, ...editorIds])
      const base = (stored ?? []).filter((id) => validIds.has(id))
      const inBase = new Set(base)
      for (const id of [...termIds, ...editorIds]) {
        if (!inBase.has(id)) {
          base.push(id)
          inBase.add(id)
        }
      }
      const order = base.filter((id) => id !== newTab.id)
      order.push(newTab.id)
      setTabBarOrder(activeWorktreeId, order)
      focusTerminalTabSurface(newTab.id)
    },
    [
      activeWorktreeId,
      createTab,
      openNewTerminalTabInActiveWorkspace,
      setActiveTabType,
      setTabBarOrder
    ]
  )

  const handleNewAgentTab = useCallback(
    (agent: TuiAgent) => {
      if (!activeWorktreeId) {
        return
      }
      const state = useAppStore.getState()
      const targetGroupId =
        state.activeGroupIdByWorktree[activeWorktreeId] ??
        state.groupsByWorktree[activeWorktreeId]?.[0]?.id
      const result = launchAgentInNewTab({
        agent,
        worktreeId: activeWorktreeId,
        groupId: targetGroupId,
        launchSource: 'shortcut'
      })
      if (!result) {
        toast.error(
          translate(
            'auto.components.Terminal.e57db40c11',
            'Could not build launch command for {{value0}}.',
            { value0: agent }
          )
        )
      }
    },
    [activeWorktreeId]
  )

  const handleOpenEntry = useCallback(async (args: TabCreateEntryArgs) => {
    await openTabBarEntry(args)
  }, [])

  const handleNewFile = useCallback(async () => {
    if (!activeWorktreeId) {
      return
    }
    const targetGroupId =
      useAppStore.getState().activeGroupIdByWorktree[activeWorktreeId] ??
      useAppStore.getState().groupsByWorktree[activeWorktreeId]?.[0]?.id
    if (!targetGroupId) {
      return
    }
    await openNewMarkdownInActiveWorkspace(targetGroupId)
  }, [activeWorktreeId, openNewMarkdownInActiveWorkspace])

  return {
    handleNewTab,
    handleNewAgentTab,
    handleOpenEntry,
    handleNewFile
  }
}

export type TerminalCreateController = TerminalColdActivationController &
  ReturnType<typeof useTerminalCreateActions>
