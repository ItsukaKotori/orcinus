import { beforeEach, describe, expect, it, vi } from 'vitest'
import type { AppState } from '@/store/types'
import type { AppShortcutState } from './app-command-handlers'

const mocks = vi.hoisted(() => ({
  store: {} as AppState,
  openGitLogTab: vi.fn()
}))

vi.mock('../store', () => ({
  useAppStore: Object.assign(vi.fn(), { getState: () => mocks.store })
}))

vi.mock('@/lib/floating-workspace-terminal-actions', () => ({
  isFloatingWorkspacePanelFocused: () => false
}))

vi.mock('@/lib/terminal-shortcut-capture-notification', () => ({
  showTerminalShortcutCaptureNotification: vi.fn()
}))

import { createAppCommandHandlers } from './app-command-handlers'

const WORKTREE_ID = 'repo::/feature'

function shortcutState(
  overrides: Partial<AppShortcutState> = {}
): AppShortcutState {
  return {
    activeView: 'terminal',
    activeWorktreeId: WORKTREE_ID,
    actions: {} as AppShortcutState['actions'],
    creationLayoutActive: false,
    floatingTerminalEnabled: false,
    floatingTerminalOpen: false,
    floatingVisibleTabCount: 0,
    keybindings: {},
    openFloatingWorkspaceMaximized: vi.fn(),
    pluginCommands: [],
    setFloatingTerminalOpen: vi.fn(),
    terminalShortcutPolicy: 'orca-first',
    workspaceChromeActive: true,
    ...overrides
  }
}

function runGitLogOpen(state: AppShortcutState = shortcutState()): boolean | undefined {
  return createAppCommandHandlers(state).get('git.log.open')?.()
}

describe('git.log.open command', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    mocks.store = { openGitLogTab: mocks.openGitLogTab } as unknown as AppState
  })

  it('opens the native Git Log tab for the active worktree', () => {
    // Palette/plugin dispatch entry (spec §3.5): the bottom-bar 「Log」 button
    // was the only way in until this action id existed.
    expect(runGitLogOpen()).toBe(true)
    expect(mocks.openGitLogTab).toHaveBeenCalledWith({ worktreeId: WORKTREE_ID })
  })

  it('does not claim the command without an active worktree', () => {
    expect(runGitLogOpen(shortcutState({ activeWorktreeId: null }))).toBe(false)
    expect(mocks.openGitLogTab).not.toHaveBeenCalled()
  })
})
