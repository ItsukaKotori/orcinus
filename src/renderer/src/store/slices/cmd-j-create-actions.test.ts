import { beforeEach, describe, expect, it, vi } from 'vitest'
import type { AppState } from '../types'
import { createTestStore, makeWorktree, seedStore, TEST_REPO } from './store-test-helpers'
const createWebRuntimeSessionTerminalMock = vi.hoisted(() => vi.fn())

vi.mock('@/runtime/web-runtime-session', () => ({
  createWebRuntimeSessionTerminal: createWebRuntimeSessionTerminalMock
}))

vi.mock('@/lib/focus-terminal-tab-surface', () => ({
  focusTerminalTabSurface: vi.fn()
}))

function browserCapableRuntimeStatus(
  environmentId: string
): AppState['runtimeStatusByEnvironmentId'] {
  return new Map([
    [environmentId, { status: { capabilities: ['browser.screencast.v1'] }, checkedAt: 1 }]
  ]) as AppState['runtimeStatusByEnvironmentId']
}

function seedActiveWorkspace(store: ReturnType<typeof createTestStore>): void {
  seedStore(store, {
    activeWorktreeId: 'wt-1',
    settings: { activeRuntimeEnvironmentId: 'runtime-1' } as AppState['settings'],
    runtimeStatusByEnvironmentId: browserCapableRuntimeStatus('runtime-1'),
    worktreesByRepo: {
      [TEST_REPO.id]: [
        makeWorktree({
          id: 'wt-1',
          repoId: TEST_REPO.id,
          hostId: 'runtime:runtime-1',
          runtimeOwnerEnvironmentId: 'runtime-1'
        })
      ]
    },
    groupsByWorktree: {
      'wt-1': [{ id: 'group-1', worktreeId: 'wt-1', activeTabId: null, tabOrder: [] }]
    },
    activeGroupIdByWorktree: { 'wt-1': 'group-1' }
  })
}

describe('Cmd+J lifted creation actions', () => {
  beforeEach(() => {
    createWebRuntimeSessionTerminalMock.mockReset()
  })

  it('does not fall back to a local terminal tab when runtime session creation fails', async () => {
    createWebRuntimeSessionTerminalMock.mockResolvedValue(false)
    const store = createTestStore()
    seedActiveWorkspace(store)

    await store.getState().openNewTerminalTabInActiveWorkspace('group-1')

    expect(createWebRuntimeSessionTerminalMock).toHaveBeenCalledWith({
      worktreeId: 'wt-1',
      environmentId: 'runtime-1',
      targetGroupId: 'group-1',
      activate: true
    })
    expect(store.getState().tabsByWorktree['wt-1'] ?? []).toEqual([])
  })

  it('creates desktop remote-server terminal tabs through the owning runtime', async () => {
    createWebRuntimeSessionTerminalMock.mockResolvedValue(false)
    const store = createTestStore()
    seedActiveWorkspace(store)
    store.setState({
      repos: [{ ...TEST_REPO, executionHostId: 'runtime:owner-runtime' }],
      worktreesByRepo: {
        [TEST_REPO.id]: [
          makeWorktree({
            id: 'wt-1',
            repoId: TEST_REPO.id,
            hostId: 'runtime:owner-runtime',
            runtimeOwnerEnvironmentId: 'owner-runtime'
          })
        ]
      },
      settings: { activeRuntimeEnvironmentId: null } as AppState['settings']
    })

    await store.getState().openNewTerminalTabInActiveWorkspace('group-1')

    expect(createWebRuntimeSessionTerminalMock).toHaveBeenCalledWith({
      worktreeId: 'wt-1',
      environmentId: 'owner-runtime',
      targetGroupId: 'group-1',
      activate: true
    })
    expect(store.getState().tabsByWorktree['wt-1'] ?? []).toEqual([])
  })

  it('fails terminal creation closed for duplicate repo IDs owned by different HUBs', async () => {
    const store = createTestStore()
    seedActiveWorkspace(store)
    store.setState({
      repos: [
        { ...TEST_REPO, executionHostId: 'runtime:hub-a' },
        { ...TEST_REPO, executionHostId: 'runtime:hub-b' }
      ],
      worktreesByRepo: {
        [TEST_REPO.id]: [makeWorktree({ id: 'wt-1', repoId: TEST_REPO.id })]
      },
      settings: { activeRuntimeEnvironmentId: 'hub-b' } as AppState['settings']
    })

    await store.getState().openNewTerminalTabInActiveWorkspace('group-1')

    expect(createWebRuntimeSessionTerminalMock).not.toHaveBeenCalled()
    expect(store.getState().tabsByWorktree['wt-1'] ?? []).toEqual([])
  })

  it('keeps desktop terminal creation local when a local worktree overrides a runtime repo owner', async () => {
    createWebRuntimeSessionTerminalMock.mockResolvedValue(false)
    const store = createTestStore()
    seedActiveWorkspace(store)
    store.setState({
      repos: [{ ...TEST_REPO, executionHostId: 'runtime:owner-runtime' }],
      worktreesByRepo: {
        [TEST_REPO.id]: [makeWorktree({ id: 'wt-1', repoId: TEST_REPO.id, hostId: 'local' })]
      },
      settings: { activeRuntimeEnvironmentId: null } as AppState['settings']
    })

    await store.getState().openNewTerminalTabInActiveWorkspace('group-1')

    expect(createWebRuntimeSessionTerminalMock).not.toHaveBeenCalled()
    expect(store.getState().tabsByWorktree['wt-1'] ?? []).toHaveLength(1)
  })
})
