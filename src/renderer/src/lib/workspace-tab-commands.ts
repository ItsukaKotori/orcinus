import type { KeybindingContext } from '../../../shared/keybindings'
import { toVisibleTabType, type Tab } from '../../../shared/tab-types'
import { useAppStore } from '@/store'
import { guardPinnedTabClose, resolvePinnedTabLabel } from '@/store/pinned-tab-close-guard'
import { createWorkspaceTabCloseCommands } from '@/components/tab-group/workspace-tab-close-commands'
import {
  handleSwitchRecentTab,
  handleSwitchTab,
  handleSwitchTabAcrossAllTypes,
  handleSwitchTerminalTab
} from '@/hooks/ipc-tab-switch'
import {
  isEmptyFloatingWorkspacePanelVisible,
  isFloatingWorkspacePanelFocused,
  switchFloatingWorkspaceTab
} from './floating-workspace-terminal-actions'
import { TOGGLE_FLOATING_TERMINAL_EVENT } from './floating-terminal'

export type WorkspaceTabTarget = { kind: 'tab'; worktreeId: string; tabId: string }

export type WorkspaceTabCommand =
  | {
      type: 'close'
      target?: WorkspaceTabTarget
      context?: KeybindingContext
      skipEmptyCheck?: boolean
      bulk?: boolean
    }
  | { type: 'switch'; direction: number; scope: 'same-type' | 'all-types' | 'terminal' }
  | { type: 'previous-recent' }

type TabState = ReturnType<typeof useAppStore.getState>

function hasFocusedGroup(state: TabState, worktreeId: string): boolean {
  const groupId = state.activeGroupIdByWorktree?.[worktreeId]
  return (state.groupsByWorktree?.[worktreeId] ?? []).some((group) => group.id === groupId)
}

function resolveActiveTab(state: TabState, worktreeId: string): Tab | null {
  const activeTab = state.getActiveTab(worktreeId)
  if (activeTab) {
    return activeTab
  }
  if (hasFocusedGroup(state, worktreeId)) {
    return null
  }
  // Hydration can publish the backing selection before its group selection.
  const entityId =
    state.activeTabType === 'editor' ? state.activeFileId : state.activeTabId
  return (
    (state.unifiedTabsByWorktree[worktreeId] ?? []).find(
      (tab) =>
        toVisibleTabType(tab.contentType) === state.activeTabType && tab.entityId === entityId
    ) ?? null
  )
}

function resolveCloseTarget(
  state: TabState,
  target?: WorkspaceTabTarget
): { worktreeId: string; tab: Tab | null } | null {
  if (target?.kind === 'tab') {
    const tab = (state.unifiedTabsByWorktree[target.worktreeId] ?? []).find(
      (tab) => tab.id === target.tabId
    )
    return tab ? { worktreeId: target.worktreeId, tab } : null
  }
  if (!state.activeWorktreeId) {
    return null
  }
  const tab = resolveActiveTab(state, state.activeWorktreeId)
  if (tab) {
    return { worktreeId: state.activeWorktreeId, tab }
  }
  return null
}

/** Input adapters describe intent; targeting and tab operations live here. */
export function dispatchWorkspaceTabCommand(command: WorkspaceTabCommand): boolean {
  const state = useAppStore.getState()
  if (command.type === 'close') {
    if (!command.target) {
      if (isEmptyFloatingWorkspacePanelVisible()) {
        window.dispatchEvent(new Event(TOGGLE_FLOATING_TERMINAL_EVENT))
        return true
      }
      if (isFloatingWorkspacePanelFocused()) {
        return false
      }
    }
    const target = resolveCloseTarget(state, command.target)
    if (!target || !target.tab) {
      return false
    }
    const tab = target.tab
    if (command.context === 'terminal' && tab.contentType === 'terminal') {
      return false
    }
    const commands = createWorkspaceTabCloseCommands({
      worktreeId: target.worktreeId,
      groupTabs: state.unifiedTabsByWorktree[target.worktreeId] ?? []
    })
    if ((command.bulk || command.skipEmptyCheck) && tab.isPinned) {
      return true
    }
    const close = () =>
      commands.closeItem(tab.id, {
        skipEmptyCheck: command.bulk || command.skipEmptyCheck,
        skipRunningProcessConfirm: command.bulk
      })
    if (tab.contentType === 'terminal' || command.bulk) {
      close()
    } else {
      guardPinnedTabClose({
        isPinned: tab.isPinned === true,
        tabLabel: resolvePinnedTabLabel(state, target.worktreeId, tab.id),
        onClose: close
      })
    }
    return true
  }
  if (command.type === 'previous-recent') {
    if (isFloatingWorkspacePanelFocused()) {
      return false
    }
    return handleSwitchRecentTab()
  }
  if (isFloatingWorkspacePanelFocused()) {
    switchFloatingWorkspaceTab(state, command.direction, command.scope)
    return true
  }
  switch (command.scope) {
    case 'same-type':
      return handleSwitchTab(command.direction)
    case 'all-types':
      return handleSwitchTabAcrossAllTypes(command.direction)
    case 'terminal':
      return handleSwitchTerminalTab(command.direction)
  }
}
