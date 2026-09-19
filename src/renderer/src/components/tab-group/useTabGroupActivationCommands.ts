import { useCallback } from 'react'
import type { Tab } from '../../../../shared/tab-types'
import { useAppStore } from '../../store'
import { focusTerminalTabSurface } from '../../lib/focus-terminal-tab-surface'
import { TOGGLE_TERMINAL_PANE_EXPAND_EVENT } from '@/constants/terminal'
import {
  activateWebRuntimeSessionTab,
  isWebRuntimeSessionActive
} from '../../runtime/web-runtime-session'
import { getRuntimeEnvironmentIdForWorktree } from '@/lib/worktree-runtime-owner'
import { browserWorkspaceHasRemoteOwner } from '@/runtime/remote-browser-tab-ownership'
import type { TabGroupWorktreeSnapshot } from './useTabGroupItemProjections'

export function useTabGroupActivationCommands({
  groupId,
  worktreeId,
  groupTabs,
  worktreeState
}: {
  groupId: string
  worktreeId: string
  groupTabs: Tab[]
  worktreeState: TabGroupWorktreeSnapshot
}) {
  const focusGroup = useAppStore((state) => state.focusGroup)
  const activateTab = useAppStore((state) => state.activateTab)
  const setActiveTab = useAppStore((state) => state.setActiveTab)
  const setActiveTabType = useAppStore((state) => state.setActiveTabType)
  const setActiveFile = useAppStore((state) => state.setActiveFile)
  const setActiveBrowserTab = useAppStore((state) => state.setActiveBrowserTab)

  const activateTerminal = useCallback(
    (terminalId: string) => {
      const item = groupTabs.find(
        (candidate) => candidate.entityId === terminalId && candidate.contentType === 'terminal'
      )
      if (!item) {
        return
      }
      focusGroup(worktreeId, groupId)
      activateTab(item.id)
      const runtimeEnvironmentId = getRuntimeEnvironmentIdForWorktree(
        useAppStore.getState(),
        worktreeId
      )
      if (isWebRuntimeSessionActive(runtimeEnvironmentId)) {
        void activateWebRuntimeSessionTab({
          worktreeId,
          tabId: terminalId,
          environmentId: runtimeEnvironmentId
        })
      }
      setActiveTab(terminalId)
      setActiveTabType('terminal')
      const activeLeafId = worktreeState.terminalLayoutsByTabId[terminalId]?.activeLeafId ?? null
      // Why: restore xterm focus to the store-active leaf so keyboard input can't drift to a sibling pane.
      focusTerminalTabSurface(terminalId, activeLeafId)
    },
    [
      activateTab,
      focusGroup,
      groupId,
      groupTabs,
      setActiveTab,
      setActiveTabType,
      worktreeState.terminalLayoutsByTabId,
      worktreeId
    ]
  )

  const toggleTerminalPaneExpand = useCallback(
    (terminalId: string) => {
      const item = groupTabs.find(
        (candidate) => candidate.entityId === terminalId && candidate.contentType === 'terminal'
      )
      if (!item) {
        return
      }
      // Why: the collapse icon stops pointer propagation, so activate here since the normal tab handler won't have run.
      activateTerminal(terminalId)
      requestAnimationFrame(() => {
        window.dispatchEvent(
          new CustomEvent(TOGGLE_TERMINAL_PANE_EXPAND_EVENT, {
            detail: { tabId: terminalId }
          })
        )
      })
    },
    [activateTerminal, groupTabs]
  )

  const activateEditor = useCallback(
    (tabId: string) => {
      const item = groupTabs.find((candidate) => candidate.id === tabId)
      if (!item) {
        return
      }
      focusGroup(worktreeId, groupId)
      activateTab(item.id)
      setActiveFile(item.entityId)
      setActiveTabType('editor')
    },
    [activateTab, focusGroup, groupId, groupTabs, setActiveFile, setActiveTabType, worktreeId]
  )

  const activateBrowser = useCallback(
    (browserTabId: string) => {
      const item = groupTabs.find(
        (candidate) => candidate.entityId === browserTabId && candidate.contentType === 'browser'
      )
      if (!item) {
        return
      }
      focusGroup(worktreeId, groupId)
      activateTab(item.id)
      const runtimeEnvironmentId = getRuntimeEnvironmentIdForWorktree(
        useAppStore.getState(),
        worktreeId
      )
      if (
        isWebRuntimeSessionActive(runtimeEnvironmentId) &&
        browserWorkspaceHasRemoteOwner(useAppStore.getState(), browserTabId, runtimeEnvironmentId)
      ) {
        void activateWebRuntimeSessionTab({
          worktreeId,
          tabId: item.id,
          environmentId: runtimeEnvironmentId
        })
      }
      setActiveBrowserTab(browserTabId)
      setActiveTabType('browser')
    },
    [activateTab, focusGroup, groupId, groupTabs, setActiveBrowserTab, setActiveTabType, worktreeId]
  )

  const activateAgentSession = useCallback(
    (tabId: string) => {
      // Why: agent-session rows are otherwise host-owned; local activation still has to
      // move focus and the visible-tab type so the row reads as the active tab.
      const tab = (useAppStore.getState().unifiedTabsByWorktree[worktreeId] ?? []).find(
        (candidate) => candidate.id === tabId && candidate.contentType === 'agent-session'
      )
      if (!tab) {
        return
      }
      focusGroup(worktreeId, tab.groupId)
      activateTab(tab.id, { worktreeId })
      setActiveTabType('agent-session', worktreeId)
    },
    [activateTab, focusGroup, setActiveTabType, worktreeId]
  )

  return {
    activateTerminal,
    toggleTerminalPaneExpand,
    activateEditor,
    activateBrowser,
    activateAgentSession
  }
}
