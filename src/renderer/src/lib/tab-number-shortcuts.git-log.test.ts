import { beforeEach, describe, expect, it, vi } from 'vitest'
import type { Tab, TabGroup } from '../../../shared/tab-types'

const mocks = vi.hoisted(() => ({
  store: {} as {
    activeView: string
    activeWorktreeId: string | null
    activeGroupIdByWorktree: Record<string, string>
    groupsByWorktree: Record<string, TabGroup[]>
    unifiedTabsByWorktree: Record<string, Tab[]>
    repos: unknown[]
    worktreesByRepo: unknown
    settings: unknown
    focusGroup: ReturnType<typeof vi.fn>
    activateTab: ReturnType<typeof vi.fn>
    setActiveTab: ReturnType<typeof vi.fn>
    setActiveTabType: ReturnType<typeof vi.fn>
    setActiveFile: ReturnType<typeof vi.fn>
  },
  runtimeOwner: { active: false },
  webRuntime: { active: false, activate: vi.fn() }
}))

vi.mock('@/store', () => ({
  useAppStore: { getState: () => mocks.store }
}))

vi.mock('@/lib/worktree-runtime-owner', () => ({
  getRuntimeEnvironmentIdForWorktree: () => (mocks.runtimeOwner.active ? 'env-1' : null)
}))

vi.mock('@/runtime/web-runtime-session', () => ({
  isWebRuntimeSessionActive: () => mocks.webRuntime.active,
  activateWebRuntimeSessionTab: mocks.webRuntime.activate
}))

vi.mock('@/lib/focus-terminal-tab-surface', () => ({
  focusTerminalTabSurface: vi.fn()
}))

import { focusTerminalTabSurface } from '@/lib/focus-terminal-tab-surface'
import { activateTabNumberShortcut } from './tab-number-shortcuts'

function tab(overrides: Partial<Tab> & Pick<Tab, 'id' | 'groupId'>): Tab {
  return {
    entityId: overrides.entityId ?? overrides.id,
    worktreeId: 'wt-1',
    contentType: 'terminal',
    label: overrides.id,
    customLabel: null,
    color: null,
    sortOrder: 0,
    createdAt: 0,
    ...overrides
  } as Tab
}

function seedStore(target: Tab): void {
  const group: TabGroup = {
    id: 'group-a',
    worktreeId: 'wt-1',
    activeTabId: null,
    tabOrder: [target.id]
  }
  mocks.store = {
    activeView: 'terminal',
    activeWorktreeId: 'wt-1',
    activeGroupIdByWorktree: { 'wt-1': 'group-a' },
    groupsByWorktree: { 'wt-1': [group] },
    unifiedTabsByWorktree: { 'wt-1': [target] },
    repos: [],
    worktreesByRepo: {},
    settings: {},
    focusGroup: vi.fn(),
    activateTab: vi.fn(),
    setActiveTab: vi.fn(),
    setActiveTabType: vi.fn(),
    setActiveFile: vi.fn()
  }
}

beforeEach(() => {
  vi.clearAllMocks()
  mocks.runtimeOwner.active = false
  mocks.webRuntime.active = false
})

describe('activateTabNumberShortcut', () => {
  it('activates a git-log tab with the git-log visible type, not the editor file stack', () => {
    // Why: mirrors the plugin branch — without it the digit chord sets the
    // active file to a git-log id, desyncing every editor surface.
    const logTab = tab({
      id: 'git-log/wt-1',
      groupId: 'group-a',
      contentType: 'git-log',
      gitLog: { worktreeId: 'wt-1' }
    })
    seedStore(logTab)

    expect(activateTabNumberShortcut(0)).toBe(true)
    expect(mocks.store.activateTab).toHaveBeenCalledWith('git-log/wt-1')
    expect(mocks.store.setActiveTabType).toHaveBeenCalledWith('git-log')
    expect(mocks.store.setActiveFile).not.toHaveBeenCalled()
    expect(focusTerminalTabSurface).not.toHaveBeenCalled()
  })
})
