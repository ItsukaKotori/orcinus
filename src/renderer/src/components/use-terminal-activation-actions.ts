import { useCallback } from 'react'
import { TOGGLE_TERMINAL_PANE_EXPAND_EVENT } from '@/constants/terminal'
import {
  activateWebRuntimeSessionTab,
  isWebRuntimeSessionActive
} from '@/runtime/web-runtime-session'
import { getActiveWorktreeRuntimeEnvironmentId } from './terminal-workspace-model'
import type { TerminalBulkCloseController } from './use-terminal-bulk-close-actions'

export function useTerminalActivationActions(controller: TerminalBulkCloseController) {
  const { activeWorktreeId, setActiveTab, setActiveTabType } = controller
  const handleActivateTab = useCallback(
    (tabId: string) => {
      const runtimeEnvironmentId = getActiveWorktreeRuntimeEnvironmentId(activeWorktreeId)
      if (activeWorktreeId && isWebRuntimeSessionActive(runtimeEnvironmentId)) {
        void activateWebRuntimeSessionTab({
          worktreeId: activeWorktreeId,
          tabId,
          environmentId: runtimeEnvironmentId
        })
      }
      setActiveTab(tabId)
      setActiveTabType('terminal')
    },
    [activeWorktreeId, setActiveTab, setActiveTabType]
  )

  const handleTogglePaneExpand = useCallback(
    (tabId: string) => {
      setActiveTab(tabId)
      requestAnimationFrame(() => {
        window.dispatchEvent(
          new CustomEvent(TOGGLE_TERMINAL_PANE_EXPAND_EVENT, { detail: { tabId } })
        )
      })
    },
    [setActiveTab]
  )

  return { handleActivateTab, handleTogglePaneExpand }
}

export type TerminalActivationController = TerminalBulkCloseController &
  ReturnType<typeof useTerminalActivationActions>
