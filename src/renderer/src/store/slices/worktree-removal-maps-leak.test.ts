/**
 * Memory-leak regression: per-worktree maps that the worktree-removal
 * paths previously missed must be evicted on removal.
 *
 * `remoteStatusesByWorktree`, `recentlyClosedEditorTabsByWorktree`
 * and `defaultTerminalTabsAppliedByWorktreeId` were re-keyed on rename but absent
 * from BOTH removal paths (the bulk `buildWorktreePurgeState` and the single
 * `removeWorktree` reducer), while their siblings (gitStatusByWorktree, …)
 * were purged. worktreeId is an unbounded, ephemeral key space (fresh UUIDs /
 * path-derived ids, never reused).
 */
import { describe, it, expect, vi, beforeEach } from 'vitest'
import type * as AgentStatusModule from '@/lib/agent-status'
import {
  getAgentHibernationPaneOutputEpoch,
  recordAgentHibernationPaneOutput,
  resetAgentHibernationOutputActivityForTests
} from '@/lib/agent-hibernation-output-activity'

vi.mock('sonner', () => ({
  toast: { info: vi.fn(), success: vi.fn(), error: vi.fn(), warning: vi.fn() }
}))

vi.mock('@/components/terminal-pane/pty-dispatcher', () => ({
  restorePtyDataHandlersAfterFailedShutdown: vi.fn(),
  unregisterPtyDataHandlers: vi.fn()
}))

vi.mock('@/lib/agent-status', async (importOriginal) => {
  const actual = await importOriginal<typeof AgentStatusModule>()
  return { ...actual, detectAgentStatusFromTitle: vi.fn().mockReturnValue(null) }
})

const mockApi = {
  worktrees: {
    list: vi.fn().mockResolvedValue([]),
    remove: vi.fn().mockResolvedValue(undefined),
    forceDeletePreservedBranch: vi.fn().mockResolvedValue({ deleted: true }),
    updateMeta: vi.fn().mockResolvedValue({})
  },
  pty: { kill: vi.fn().mockResolvedValue(undefined) },
  runtimeEnvironments: { call: vi.fn().mockResolvedValue({ ok: true, result: {} }) }
}

// @ts-expect-error -- minimal window.api stub for the store under test
globalThis.window = { api: mockApi }

import {
  createTestStore,
  seedStore,
  makeWorktree,
  makeOpenFile,
  makeTab
} from './store-test-helpers'

const WT1 = 'repo1::/path/wt1'
const WT2 = 'repo1::/path/wt2'

describe('worktree removal evicts the per-worktree + per-page maps it previously missed', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    mockApi.worktrees.remove.mockResolvedValue(undefined)
    resetAgentHibernationOutputActivityForTests()
  })

  function seedWorktreeKeyedMaps(store: ReturnType<typeof createTestStore>): void {
    seedStore(store, {
      worktreesByRepo: {
        repo1: [
          makeWorktree({ id: WT1, repoId: 'repo1', path: '/path/wt1' }),
          makeWorktree({ id: WT2, repoId: 'repo1', path: '/path/wt2' })
        ]
      },
      remoteStatusesByWorktree: {
        [WT1]: { hasUpstream: true, ahead: 1, behind: 0 },
        [WT2]: { hasUpstream: true, ahead: 0, behind: 2 }
      },
      recentlyClosedEditorTabsByWorktree: {
        [WT1]: [makeOpenFile({ id: '/path/wt1/closed.ts', worktreeId: WT1 })],
        [WT2]: [makeOpenFile({ id: '/path/wt2/closed.ts', worktreeId: WT2 })]
      },
      recentlyClosedTerminalTabsByWorktree: {
        [WT1]: [{ startupCwd: '/path/wt1' }],
        [WT2]: [{ startupCwd: '/path/wt2' }]
      },
      recentlyClosedTabKindsByWorktree: { [WT1]: ['terminal'], [WT2]: ['terminal'] },
      defaultTerminalTabsAppliedByWorktreeId: { [WT1]: true, [WT2]: true }
    })
  }

  it('bulk purgeWorktreeTerminalState drops worktree-keyed maps for the removed worktree only', () => {
    const store = createTestStore()
    seedWorktreeKeyedMaps(store)

    store.getState().purgeWorktreeTerminalState([WT1])

    const s = store.getState()
    // Evicted for the removed worktree.
    expect(s.remoteStatusesByWorktree[WT1]).toBeUndefined()
    expect(s.recentlyClosedEditorTabsByWorktree[WT1]).toBeUndefined()
    expect(s.recentlyClosedTerminalTabsByWorktree[WT1]).toBeUndefined()
    expect(s.recentlyClosedTabKindsByWorktree[WT1]).toBeUndefined()
    expect(s.defaultTerminalTabsAppliedByWorktreeId[WT1]).toBeUndefined()
    // Retained for the surviving worktree (guard over-eviction).
    expect(s.remoteStatusesByWorktree[WT2]).toBeDefined()
    expect(s.recentlyClosedEditorTabsByWorktree[WT2]).toBeDefined()
    expect(s.recentlyClosedTerminalTabsByWorktree[WT2]).toBeDefined()
    expect(s.recentlyClosedTabKindsByWorktree[WT2]).toBeDefined()
    expect(s.defaultTerminalTabsAppliedByWorktreeId[WT2]).toBe(true)
  })

  it('single removeWorktree drops worktree-keyed maps for the removed worktree only', async () => {
    const store = createTestStore()
    seedWorktreeKeyedMaps(store)

    const result = await store.getState().removeWorktree({ id: WT1, executionHostId: null })
    expect(result).toEqual({ ok: true })

    const s = store.getState()
    expect(s.remoteStatusesByWorktree[WT1]).toBeUndefined()
    expect(s.recentlyClosedEditorTabsByWorktree[WT1]).toBeUndefined()
    expect(s.recentlyClosedTerminalTabsByWorktree[WT1]).toBeUndefined()
    expect(s.recentlyClosedTabKindsByWorktree[WT1]).toBeUndefined()
    expect(s.defaultTerminalTabsAppliedByWorktreeId[WT1]).toBeUndefined()
    expect(s.remoteStatusesByWorktree[WT2]).toBeDefined()
    expect(s.recentlyClosedEditorTabsByWorktree[WT2]).toBeDefined()
    expect(s.recentlyClosedTerminalTabsByWorktree[WT2]).toBeDefined()
    expect(s.recentlyClosedTabKindsByWorktree[WT2]).toBeDefined()
    expect(s.defaultTerminalTabsAppliedByWorktreeId[WT2]).toBe(true)
  })

  it('worktree removal drops the hibernation output-epoch map for the removed worktree only', () => {
    const store = createTestStore()
    const LEAF = '11111111-1111-4111-8111-111111111111'
    const TAB1 = 'tab-wt1'
    const TAB2 = 'tab-wt2'
    seedStore(store, {
      worktreesByRepo: {
        repo1: [
          makeWorktree({ id: WT1, repoId: 'repo1', path: '/path/wt1' }),
          makeWorktree({ id: WT2, repoId: 'repo1', path: '/path/wt2' })
        ]
      },
      tabsByWorktree: {
        [WT1]: [makeTab({ id: TAB1, worktreeId: WT1 })],
        [WT2]: [makeTab({ id: TAB2, worktreeId: WT2 })]
      }
    })
    const removedPaneKey = `${TAB1}:${LEAF}`
    const survivingPaneKey = `${TAB2}:${LEAF}`
    recordAgentHibernationPaneOutput(removedPaneKey)
    recordAgentHibernationPaneOutput(survivingPaneKey)

    store.getState().purgeWorktreeTerminalState([WT1])

    // Removed worktree's pane reads back as never-seen (epoch 0).
    expect(getAgentHibernationPaneOutputEpoch(removedPaneKey)).toBe(0)
    // Surviving worktree's pane keeps its epoch (guard over-eviction).
    expect(getAgentHibernationPaneOutputEpoch(survivingPaneKey)).toBe(1)
  })

})
