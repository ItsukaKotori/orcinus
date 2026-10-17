import { describe, it, expect, vi, beforeEach } from 'vitest'
import type * as AgentStatusModule from '@/lib/agent-status'
import {
  createTestStore,
  makeOpenFile,
  makeTab,
  makeTabGroup,
  makeUnifiedTab,
  makeWorktree,
  seedStore
} from './store-test-helpers'
import { createStoreCascadesMockApi } from './store-cascades-test-harness'

const mockUnregisterPtyDataHandlers = vi.hoisted(() => vi.fn<() => unknown[]>(() => []))
const mockRestorePtyDataHandlersAfterFailedShutdown = vi.hoisted(() => vi.fn())

// Mock sonner (imported by repos.ts)
vi.mock('sonner', () => ({
  toast: { info: vi.fn(), success: vi.fn(), error: vi.fn(), warning: vi.fn() }
}))

vi.mock('@/components/terminal-pane/pty-dispatcher', () => ({
  restorePtyDataHandlersAfterFailedShutdown: mockRestorePtyDataHandlersAfterFailedShutdown,
  unregisterPtyDataHandlers: mockUnregisterPtyDataHandlers
}))

// Mock agent-status (imported by terminal-helpers)
vi.mock('@/lib/agent-status', async (importOriginal) => {
  const actual = await importOriginal<typeof AgentStatusModule>()
  return {
    ...actual,
    detectAgentStatusFromTitle: vi.fn().mockReturnValue(null)
  }
})

const mockApi = createStoreCascadesMockApi()

describe('setActiveWorktree', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    mockApi.worktrees.updateMeta.mockResolvedValue({})
  })

  // Why: a stale unread flag on a closed tab renders a bell the user can never dismiss, since the tab is gone.
  it('drops unreadTerminalTabs for a closed tab', () => {
    const store = createTestStore()
    const wt = 'repo1::/path/wt1'

    seedStore(store, {
      worktreesByRepo: {
        repo1: [makeWorktree({ id: wt, repoId: 'repo1', path: '/path/wt1' })]
      }
    })

    const closing = store.getState().createTab(wt)
    const surviving = store.getState().createTab(wt)

    // Seed directly: mark actions refuse the active tab, but this test targets closeTab's cleanup, not the guards.
    store.setState({
      unreadTerminalTabs: {
        [closing.id]: true as const,
        [surviving.id]: true as const
      }
    })

    store.getState().closeTab(closing.id)

    const s = store.getState()
    expect(s.unreadTerminalTabs[closing.id]).toBeUndefined()
    // Siblings untouched.
    expect(s.unreadTerminalTabs[surviving.id]).toBe(true)
  })

  // Why: focus events that normally clear unread never arrive for dead PTYs, so the shutdown path must drop the flags itself.
  it('drops unread flags for every tab in a shutdown worktree', async () => {
    const store = createTestStore()
    const wt = 'repo1::/path/wt1'

    seedStore(store, {
      worktreesByRepo: {
        repo1: [makeWorktree({ id: wt, repoId: 'repo1', path: '/path/wt1' })]
      }
    })

    const tabA = store.getState().createTab(wt)
    const tabB = store.getState().createTab(wt)

    // Seed flags directly (see closeTab test for why).
    store.setState({
      unreadTerminalTabs: {
        [tabA.id]: true as const,
        [tabB.id]: true as const
      }
    })

    await store.getState().shutdownWorktreeTerminals(wt)

    const s = store.getState()
    expect(s.unreadTerminalTabs[tabA.id]).toBeUndefined()
    expect(s.unreadTerminalTabs[tabB.id]).toBeUndefined()
  })


  it('returns to the landing state when closing the last terminal tab in the active worktree', () => {
    const store = createTestStore()
    const wt = 'repo1::/path/wt1'
    const groupId = 'group-1'
    const tabId = 'tab-1'
    const unifiedTabId = 'unified-tab-1'

    seedStore(store, {
      worktreesByRepo: {
        repo1: [makeWorktree({ id: wt, repoId: 'repo1', path: '/path/wt1' })]
      },
      activeWorktreeId: wt,
      activeTabId: tabId,
      activeTabType: 'terminal',
      activeTabIdByWorktree: { [wt]: tabId },
      activeTabTypeByWorktree: { [wt]: 'terminal' },
      tabsByWorktree: {
        [wt]: [makeTab({ id: tabId, worktreeId: wt })]
      },
      unifiedTabsByWorktree: {
        [wt]: [
          makeUnifiedTab({
            id: unifiedTabId,
            entityId: tabId,
            worktreeId: wt,
            groupId,
            contentType: 'terminal',
            label: 'Terminal 1'
          })
        ]
      },
      groupsByWorktree: {
        [wt]: [
          makeTabGroup({
            id: groupId,
            worktreeId: wt,
            activeTabId: unifiedTabId,
            tabOrder: [unifiedTabId]
          })
        ]
      },
      activeGroupIdByWorktree: { [wt]: groupId },
      layoutByWorktree: {
        [wt]: { type: 'leaf', groupId }
      }
    })

    store.getState().closeTab(tabId)

    const s = store.getState()
    expect(s.activeWorktreeId).toBeNull()
    expect(s.activeTabId).toBeNull()
    expect(s.tabsByWorktree[wt]).toEqual([])
    expect(s.unifiedTabsByWorktree[wt]).toEqual([])
  })

  it('keeps terminal numbering stable when a live agent renames an existing tab', () => {
    const store = createTestStore()
    const wt = 'repo1::/path/wt1'

    seedStore(store, {
      worktreesByRepo: {
        repo1: [makeWorktree({ id: wt, repoId: 'repo1', path: '/path/wt1' })]
      }
    })

    const first = store.getState().createTab(wt)
    store.getState().updateTabTitle(first.id, 'Claude Code')

    const second = store.getState().createTab(wt)

    expect(store.getState().tabsByWorktree[wt]?.[0]).toMatchObject({
      id: first.id,
      title: 'Claude Code',
      defaultTitle: 'Terminal 1'
    })
    expect(second.title).toBe('Terminal 2')
    expect(second.defaultTitle).toBe('Terminal 2')
  })

  it('falls back to the stable terminal label when a live title clears', () => {
    const store = createTestStore()
    const wt = 'repo1::/path/wt1'

    seedStore(store, {
      worktreesByRepo: {
        repo1: [makeWorktree({ id: wt, repoId: 'repo1', path: '/path/wt1' })]
      }
    })

    const first = store.getState().createTab(wt)
    store.getState().updateTabTitle(first.id, 'Claude Code')
    store.getState().updateTabTitle(first.id, '')

    expect(store.getState().tabsByWorktree[wt]?.[0]).toMatchObject({
      id: first.id,
      title: 'Terminal 1',
      defaultTitle: 'Terminal 1'
    })
    expect(
      store
        .getState()
        .unifiedTabsByWorktree[wt]?.find(
          (tab) => tab.contentType === 'terminal' && tab.entityId === first.id
        )
    ).toMatchObject({
      label: 'Terminal 1'
    })
  })

  it('preserves terminal and unified tab map references when a live title repeats', () => {
    const store = createTestStore()
    const wt = 'repo1::/path/wt1'

    seedStore(store, {
      worktreesByRepo: {
        repo1: [makeWorktree({ id: wt, repoId: 'repo1', path: '/path/wt1' })]
      }
    })

    const first = store.getState().createTab(wt)
    store.getState().updateTabTitle(first.id, 'Claude Code')
    const tabsByWorktree = store.getState().tabsByWorktree
    const unifiedTabsByWorktree = store.getState().unifiedTabsByWorktree
    const sortEpoch = store.getState().sortEpoch

    store.getState().updateTabTitle(first.id, 'Claude Code')

    expect(store.getState().tabsByWorktree).toBe(tabsByWorktree)
    expect(store.getState().unifiedTabsByWorktree).toBe(unifiedTabsByWorktree)
    expect(store.getState().sortEpoch).toBe(sortEpoch)
  })

  it('repairs a stale unified tab label when a live title repeats', () => {
    const store = createTestStore()
    const wt = 'repo1::/path/wt1'

    seedStore(store, {
      worktreesByRepo: {
        repo1: [makeWorktree({ id: wt, repoId: 'repo1', path: '/path/wt1' })]
      }
    })

    const first = store.getState().createTab(wt)
    store.getState().updateTabTitle(first.id, 'Claude Code')
    const tabsByWorktree = store.getState().tabsByWorktree
    store.setState((state) => ({
      unifiedTabsByWorktree: {
        ...state.unifiedTabsByWorktree,
        [wt]: state.unifiedTabsByWorktree[wt].map((tab) =>
          tab.contentType === 'terminal' && tab.entityId === first.id
            ? { ...tab, label: 'stale' }
            : tab
        )
      }
    }))

    store.getState().updateTabTitle(first.id, 'Claude Code')

    expect(store.getState().tabsByWorktree).toBe(tabsByWorktree)
    expect(
      store
        .getState()
        .unifiedTabsByWorktree[wt]?.find(
          (tab) => tab.contentType === 'terminal' && tab.entityId === first.id
        )?.label
    ).toBe('Claude Code')
  })

  it('restores terminal surface when switching to a worktree that was last on a terminal tab with open files', () => {
    const store = createTestStore()
    const wt = 'repo1::/path/wt1'
    const fileId = '/path/wt1/src/index.ts'

    seedStore(store, {
      worktreesByRepo: {
        repo1: [makeWorktree({ id: wt, repoId: 'repo1', path: '/path/wt1' })]
      },
      activeWorktreeId: null,
      tabsByWorktree: {
        [wt]: [makeTab({ id: 'terminal-1', worktreeId: wt })]
      },
      openFiles: [makeOpenFile({ id: fileId, worktreeId: wt, filePath: fileId })],
      activeFileIdByWorktree: { [wt]: fileId },
      // User was on the terminal, not the editor
      activeTabTypeByWorktree: { [wt]: 'terminal' },
    })

    store.getState().setActiveWorktree(wt)

    const s = store.getState()
    expect(s.activeWorktreeId).toBe(wt)
    expect(s.activeTabType).toBe('terminal')
    // File ID should still be tracked for background state
    expect(s.activeFileId).toBe(fileId)
  })
})
