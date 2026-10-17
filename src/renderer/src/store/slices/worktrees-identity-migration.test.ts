import { beforeEach, describe, expect, it, vi } from 'vitest'
import type { AppState } from '../types'
import {
  beginHugeRepoWarningProbe,
  clearHugeRepoWarningDismissalsForTests,
  hasDismissedHugeRepoWarning,
  markHugeRepoWarningDismissed
} from '@/lib/source-control-huge-repo-warning-dismissals'
import {
} from './worktrees'
import { worktreeWorkspaceKey } from '../../../../shared/workspace-scope'
import {
  createTestStore,
  resetRemoteRuntimeMocks,
  resetWorktreeSliceModuleMemory
} from './worktrees-slice-test-harness'

const requestWorktreeBaseFallbackNotice = vi.hoisted(() => vi.fn())

vi.mock('sonner', () => ({
  toast: {
    warning: vi.fn(),
    info: vi.fn(),
    success: vi.fn(),
    error: vi.fn(),
    dismiss: vi.fn()
  }
}))

vi.mock('@/components/worktree-base-fallback-notice', () => ({
  requestWorktreeBaseFallbackNotice
}))

beforeEach(resetWorktreeSliceModuleMemory)

describe('migrateWorktreeIdentity', () => {
  const OLD = 'repo1::/ws/cunner'
  const NEW = 'repo1::/ws/worktree-creation-spinner'

  beforeEach(() => {
    vi.clearAllMocks()
    resetRemoteRuntimeMocks()
    clearHugeRepoWarningDismissalsForTests()
  })

  it('carries a dismissal across rename while invalidating the old-path probe', () => {
    const store = createTestStore()
    const staleProbe = beginHugeRepoWarningProbe({ id: OLD, instanceId: 'persisted-instance' })
    expect(markHugeRepoWarningDismissed(staleProbe)).toBe(true)

    store.getState().migrateWorktreeIdentity(OLD, NEW)

    const renamedProbe = beginHugeRepoWarningProbe({ id: NEW, instanceId: 'persisted-instance' })
    expect(hasDismissedHugeRepoWarning(staleProbe)).toBe(false)
    expect(markHugeRepoWarningDismissed(staleProbe)).toBe(false)
    expect(hasDismissedHugeRepoWarning(renamedProbe)).toBe(true)
  })

  it('re-keys worktree-scoped maps, pointers, the Set, and openFiles old->new', () => {
    const store = createTestStore()
    store.setState({
      activeWorktreeId: OLD,
      activeWorkspaceKey: worktreeWorkspaceKey(OLD),
      renamingWorktreeId: { worktreeId: OLD, rowKey: 'all:old' },
      tabsByWorktree: { [OLD]: [{ id: 'tab1', worktreeId: OLD }] },
      rightSidebarExplorerViewByWorktree: { [OLD]: 'search' },
      fileSearchStateByWorktree: {
        [OLD]: { resultOwner: { worktreeId: OLD, runtimeEnvironmentId: 'runtime-a' } }
      },
      activeTabIdByWorktree: { [OLD]: 'tab1' },
      unifiedTabsByWorktree: { [OLD]: [{ id: 'unified1', worktreeId: OLD }] },
      groupsByWorktree: { [OLD]: [{ id: 'group1', worktreeId: OLD }] },
      gitStatusByWorktree: { [OLD]: [{ path: 'a.ts' }] },
      gitStatusHeadByWorktree: { [OLD]: 'head-old' },
      gitBranchLineTotalByWorktree: { [OLD]: { added: 24, removed: 3, mergeBase: 'base-old' } },
      gitBranchCompareRequestStatusHeadByWorktree: { [OLD]: 'head-old' },
      lastVisitedAtByWorktreeId: { [OLD]: 123 },
      defaultTerminalTabsAppliedByWorktreeId: { [OLD]: true },
      recentlyClosedEditorTabsByWorktree: { [OLD]: [{ id: 'f1', worktreeId: OLD }] },
      recentlyClosedTerminalTabsByWorktree: {
        [OLD]: [{ startupCwd: '/ws/cunner/packages/app' }, { startupCwd: '/elsewhere/dir' }]
      },
      recentlyClosedTabKindsByWorktree: { [OLD]: ['terminal', 'editor'] },
      remoteStatusesByWorktree: { [OLD]: { ahead: 1 } },
      everActivatedWorktreeIds: new Set([OLD]),
      openFiles: [{ id: 'f1', worktreeId: OLD }],
      pendingReconnectWorktreeIds: [OLD],
      sleepingAgentSessionsByPaneKey: {
        'tab1:leaf': {
          paneKey: 'tab1:leaf',
          tabId: 'tab1',
          worktreeId: OLD,
          agent: 'codex',
          providerSession: { key: 'session_id', id: 'session-1' },
          prompt: 'Do work',
          state: 'done',
          capturedAt: 1,
          updatedAt: 1
        }
      },
      // Tab-id-keyed: must NOT be re-keyed (the tab keeps its id across rename).
      terminalLayoutsByTabId: { tab1: { root: { type: 'leaf', leafId: '0' } } }
    } as unknown as Partial<AppState>)

    store.getState().migrateWorktreeIdentity(OLD, NEW)
    const s = store.getState()

    expect(s.tabsByWorktree[OLD]).toBeUndefined()
    expect(s.tabsByWorktree[NEW]).toEqual([{ id: 'tab1', worktreeId: NEW }])
    expect(s.activeWorktreeId).toBe(NEW)
    expect(s.activeWorkspaceKey).toBe(worktreeWorkspaceKey(NEW))
    expect(s.renamingWorktreeId).toEqual({ worktreeId: NEW, rowKey: 'all:old' })
    expect(s.activeTabIdByWorktree[NEW]).toBe('tab1')
    expect(s.unifiedTabsByWorktree[NEW]?.[0]?.worktreeId).toBe(NEW)
    expect(s.groupsByWorktree[NEW]?.[0]?.worktreeId).toBe(NEW)
    expect(s.gitStatusByWorktree[NEW]).toEqual([{ path: 'a.ts' }])
    expect(s.gitStatusHeadByWorktree[NEW]).toBe('head-old')
    expect(s.gitBranchLineTotalByWorktree[OLD]).toBeUndefined()
    expect(s.gitBranchLineTotalByWorktree[NEW]).toEqual({
      added: 24,
      removed: 3,
      mergeBase: 'base-old'
    })
    expect(s.gitBranchCompareRequestStatusHeadByWorktree[NEW]).toBe('head-old')
    expect(s.rightSidebarExplorerViewByWorktree[OLD]).toBeUndefined()
    expect(s.rightSidebarExplorerViewByWorktree[NEW]).toBe('search')
    expect(s.fileSearchStateByWorktree[OLD]).toBeUndefined()
    expect(s.fileSearchStateByWorktree[NEW]?.resultOwner).toEqual({
      worktreeId: NEW,
      runtimeEnvironmentId: 'runtime-a'
    })
    expect(s.lastVisitedAtByWorktreeId[NEW]).toBe(123)
    expect(s.defaultTerminalTabsAppliedByWorktreeId[NEW]).toBe(true)
    // The two maps absent from the purge list are still re-keyed.
    expect(s.recentlyClosedEditorTabsByWorktree[NEW]).toEqual([{ id: 'f1', worktreeId: NEW }])
    // Terminal reopen snapshots re-key and remap startupCwd under the old path; outside paths stay as-is.
    expect(s.recentlyClosedTerminalTabsByWorktree[OLD]).toBeUndefined()
    expect(s.recentlyClosedTerminalTabsByWorktree[NEW]).toEqual([
      { startupCwd: '/ws/worktree-creation-spinner/packages/app' },
      { startupCwd: '/elsewhere/dir' }
    ])
    expect(s.recentlyClosedTabKindsByWorktree[OLD]).toBeUndefined()
    expect(s.recentlyClosedTabKindsByWorktree[NEW]).toEqual(['terminal', 'editor'])
    expect(s.remoteStatusesByWorktree[NEW]).toEqual({ ahead: 1 })
    expect(s.everActivatedWorktreeIds.has(NEW)).toBe(true)
    expect(s.everActivatedWorktreeIds.has(OLD)).toBe(false)
    expect(s.openFiles[0].worktreeId).toBe(NEW)
    expect(s.pendingReconnectWorktreeIds).toEqual([NEW])
    expect(s.sleepingAgentSessionsByPaneKey['tab1:leaf']?.worktreeId).toBe(NEW)
    // Tab-id-keyed state is untouched — the tab survives with the same id.
    expect(s.terminalLayoutsByTabId.tab1).toBeDefined()
  })

  it('is a no-op when the ids match', () => {
    const store = createTestStore()
    store.setState({
      activeWorktreeId: OLD,
      tabsByWorktree: { [OLD]: [{ id: 'tab1' }] }
    } as unknown as Partial<AppState>)
    store.getState().migrateWorktreeIdentity(OLD, OLD)
    expect(store.getState().tabsByWorktree[OLD]).toEqual([{ id: 'tab1' }])
    expect(store.getState().activeWorktreeId).toBe(OLD)
  })

  it('leaves an unrelated active worktree pointer alone', () => {
    const store = createTestStore()
    const OTHER = 'repo1::/ws/other'
    store.setState({
      activeWorktreeId: OTHER,
      tabsByWorktree: { [OLD]: [{ id: 'tab1' }] }
    } as unknown as Partial<AppState>)
    store.getState().migrateWorktreeIdentity(OLD, NEW)
    expect(store.getState().activeWorktreeId).toBe(OTHER)
    expect(store.getState().tabsByWorktree[NEW]).toEqual([{ id: 'tab1' }])
  })

})
