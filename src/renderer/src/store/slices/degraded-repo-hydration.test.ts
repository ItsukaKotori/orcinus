import { expect, it, vi } from 'vitest'
import type * as AgentStatusModule from '@/lib/agent-status'
import type { WorkspaceSessionState } from '../../../../shared/workspace-session-state-types'
import { getDefaultWorkspaceSession } from '../../../../shared/constants'

vi.mock('sonner', () => ({ toast: { info: vi.fn(), success: vi.fn(), error: vi.fn() } }))
vi.mock('@/lib/agent-status', async (importOriginal) => {
  const actual = await importOriginal<typeof AgentStatusModule>()
  return { ...actual, detectAgentStatusFromTitle: vi.fn().mockReturnValue(null) }
})

// @ts-expect-error -- mocked browser preload API
globalThis.window = { api: {} }

import {
  buildWorkspaceSessionPayload,
  shouldPersistWorkspaceSession
} from '@/lib/workspace-session'
import { createTestStore, makeTab, makeWorktree } from './store-test-helpers'

const WORKTREE_ID = 'repo1::/path/degraded'
const TERMINAL_ID = 'terminal-degraded'
const EDITOR_FILE_ID = '/path/degraded/src/App.tsx'
const GROUP_ID = 'group-degraded'

function makeDegradedRepoSession(): WorkspaceSessionState {
  return {
    ...getDefaultWorkspaceSession(),
    activeRepoId: 'repo1',
    activeWorktreeId: WORKTREE_ID,
    activeTabId: TERMINAL_ID,
    tabsByWorktree: {
      [WORKTREE_ID]: [makeTab({ id: TERMINAL_ID, worktreeId: WORKTREE_ID })]
    },
    openFilesByWorktree: {
      [WORKTREE_ID]: [
        {
          filePath: EDITOR_FILE_ID,
          relativePath: 'src/App.tsx',
          worktreeId: WORKTREE_ID,
          language: 'typescript'
        }
      ]
    },
    activeFileIdByWorktree: { [WORKTREE_ID]: EDITOR_FILE_ID },
    unifiedTabs: {
      [WORKTREE_ID]: [
        {
          id: TERMINAL_ID,
          entityId: TERMINAL_ID,
          groupId: GROUP_ID,
          worktreeId: WORKTREE_ID,
          contentType: 'terminal',
          label: 'Terminal',
          customLabel: null,
          color: null,
          sortOrder: 0,
          createdAt: 1
        },
        {
          id: EDITOR_FILE_ID,
          entityId: EDITOR_FILE_ID,
          groupId: GROUP_ID,
          worktreeId: WORKTREE_ID,
          contentType: 'editor',
          label: 'App.tsx',
          customLabel: null,
          color: null,
          sortOrder: 1,
          createdAt: 2
        }
      ]
    },
    tabGroups: {
      [WORKTREE_ID]: [
        {
          id: GROUP_ID,
          worktreeId: WORKTREE_ID,
          activeTabId: EDITOR_FILE_ID,
          tabOrder: [TERMINAL_ID, EDITOR_FILE_ID],
          recentTabIds: [TERMINAL_ID, EDITOR_FILE_ID]
        }
      ]
    },
    activeGroupIdByWorktree: { [WORKTREE_ID]: GROUP_ID }
  }
}

function makeTerminalFreeDegradedRepoSession(): WorkspaceSessionState {
  const session = makeDegradedRepoSession()
  session.activeTabId = null
  session.tabsByWorktree = {}
  session.activeTabTypeByWorktree = { [WORKTREE_ID]: 'editor' }
  session.unifiedTabs![WORKTREE_ID] = session.unifiedTabs![WORKTREE_ID].filter(
    (tab) => tab.contentType !== 'terminal'
  )
  session.tabGroups![WORKTREE_ID] = session.tabGroups![WORKTREE_ID].map((group) => ({
    ...group,
    tabOrder: [EDITOR_FILE_ID],
    recentTabIds: [EDITOR_FILE_ID]
  }))
  return session
}

function hydrateWithRepoScan(
  store: ReturnType<typeof createTestStore>,
  session: WorkspaceSessionState,
  authoritative = false
): void {
  store.setState({
    repos: [{ id: 'repo1', path: '/repo1', displayName: 'Repo 1', badgeColor: '#000', addedAt: 0 }],
    worktreesByRepo: { repo1: [] },
    detectedWorktreesByRepo: {
      repo1: {
        repoId: 'repo1',
        authoritative,
        source: authoritative ? 'git' : 'metadata-fallback',
        worktrees: []
      }
    }
  })
  store.getState().hydrateWorkspaceSession(session)
  store.getState().hydrateTabsSession(session)
  store.getState().hydrateEditorSession(session)
}

it('keeps tab and editor chrome through degraded hydration and persistence', () => {
  const firstStore = createTestStore()
  hydrateWithRepoScan(firstStore, makeDegradedRepoSession())
  firstStore.setState({ workspaceSessionReady: true })
  firstStore.getState().setHydrationSucceeded(true)
  expect(shouldPersistWorkspaceSession(firstStore.getState())).toBe(true)

  const persisted = buildWorkspaceSessionPayload(firstStore.getState())
  const restoredStore = createTestStore()
  hydrateWithRepoScan(restoredStore, persisted)

  const restored = restoredStore.getState()
  expect(restored.unifiedTabsByWorktree[WORKTREE_ID]?.map((tab) => tab.id)).toEqual([
    TERMINAL_ID,
    EDITOR_FILE_ID
  ])
  expect(restored.openFiles.map((file) => file.id)).toEqual([EDITOR_FILE_ID])
})

it('keeps a terminal-free degraded workspace selected through hydration and persistence', () => {
  const firstStore = createTestStore()
  hydrateWithRepoScan(firstStore, makeTerminalFreeDegradedRepoSession())

  const first = firstStore.getState()
  expect(first.activeWorktreeId).toBe(WORKTREE_ID)
  expect(first.activeTabType).toBe('editor')

  firstStore.setState({ workspaceSessionReady: true })
  firstStore.getState().setHydrationSucceeded(true)
  const persisted = buildWorkspaceSessionPayload(firstStore.getState())
  const restoredStore = createTestStore()
  hydrateWithRepoScan(restoredStore, persisted)

  const restored = restoredStore.getState()
  expect(restored.activeWorktreeId).toBe(WORKTREE_ID)
  expect(restored.unifiedTabsByWorktree[WORKTREE_ID]?.map((tab) => tab.id)).toEqual([
    EDITOR_FILE_ID
  ])
  expect(restored.activeTabType).toBe('editor')
})

it('keeps chrome when a non-authoritative fallback lists only some of the repo worktrees', () => {
  const store = createTestStore()
  // Why: an SSH metadata fallback can be non-empty yet partial (host-less metas are skipped on multi-owner
  // repos, agent-scratch stays hidden). A partial list must not read as proof the missing worktree was deleted.
  const sibling = makeWorktree({
    id: 'repo1::/path/sibling',
    repoId: 'repo1',
    path: '/path/sibling'
  })
  store.setState({
    repos: [{ id: 'repo1', path: '/repo1', displayName: 'Repo 1', badgeColor: '#000', addedAt: 0 }],
    worktreesByRepo: { repo1: [sibling] },
    detectedWorktreesByRepo: {
      repo1: {
        repoId: 'repo1',
        authoritative: false,
        source: 'metadata-fallback',
        worktrees: []
      }
    }
  })
  const session = makeDegradedRepoSession()
  store.getState().hydrateWorkspaceSession(session)
  store.getState().hydrateTabsSession(session)
  store.getState().hydrateEditorSession(session)

  const state = store.getState()
  expect(state.tabsByWorktree[WORKTREE_ID]?.map((tab) => tab.id)).toEqual([TERMINAL_ID])
  expect(state.openFiles.map((file) => file.id)).toEqual([EDITOR_FILE_ID])
})

it('drops terminal-free chrome when an authoritative scan proves deletion', () => {
  const store = createTestStore()
  hydrateWithRepoScan(store, makeTerminalFreeDegradedRepoSession(), true)

  const state = store.getState()
  expect(state.activeWorktreeId).toBeNull()
  expect(state.unifiedTabsByWorktree[WORKTREE_ID]).toBeUndefined()
  expect(state.openFiles).toEqual([])
})
