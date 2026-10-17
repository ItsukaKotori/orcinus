import { beforeEach, describe, expect, it, vi } from 'vitest'
import type { AppState } from '../types'
import { toast } from 'sonner'
import { makeWorktree } from './worktrees-slice-test-fixtures'
import {
  createTestStore,
  mockApi,
  resetRemoteRuntimeMocks,
  resetWorktreeSliceModuleMemory,
  runtimeEnvironmentCall
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

describe('worktree remote runtime mutations', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    resetRemoteRuntimeMocks()
  })

  it('persists worktree metadata through the active remote runtime environment', async () => {
    const store = createTestStore()
    const wt = makeWorktree({ id: 'repo1::/path/wt1', repoId: 'repo1', path: '/path/wt1' })
    runtimeEnvironmentCall.mockResolvedValue({
      id: 'rpc-set',
      ok: true,
      result: { worktree: { ...wt, comment: 'remote note' } },
      _meta: { runtimeId: 'runtime-remote' }
    })
    store.setState({
      settings: { activeRuntimeEnvironmentId: 'env-1' } as never,
      worktreesByRepo: { repo1: [wt] }
    } as Partial<AppState>)

    await store.getState().updateWorktreeMeta(wt.id, { comment: 'remote note' })

    expect(runtimeEnvironmentCall).toHaveBeenCalledWith({
      selector: 'env-1',
      method: 'worktree.set',
      params: expect.objectContaining({ worktree: `id:${wt.id}`, comment: 'remote note' }),
      timeoutMs: 15_000
    })
    expect(mockApi.worktrees.updateMeta).not.toHaveBeenCalled()
    expect(store.getState().worktreesByRepo.repo1[0]?.comment).toBe('remote note')
  })

  it('force-deletes a preserved HUB-owned SSH branch through its HUB', async () => {
    const store = createTestStore()
    const wt = makeWorktree({
      id: 'repo-ssh::/srv/nested-wt',
      repoId: 'repo-ssh',
      hostId: 'ssh:hub-private-target',
      runtimeOwnerEnvironmentId: 'owner-hub'
    })
    runtimeEnvironmentCall.mockResolvedValue({
      id: 'rpc-force-delete-branch',
      ok: true,
      result: { deleted: true },
      _meta: { runtimeId: 'runtime-owner-hub' }
    })
    store.setState({
      settings: { activeRuntimeEnvironmentId: 'different-hub' } as never,
      worktreesByRepo: { 'repo-ssh': [wt] }
    } as Partial<AppState>)

    const result = await store
      .getState()
      .forceDeletePreservedBranch(wt.id, 'feature/nested', 'abc123')

    expect(result).toEqual({ ok: true, deleted: true })
    expect(runtimeEnvironmentCall).toHaveBeenCalledWith({
      selector: 'owner-hub',
      method: 'worktree.forceDeleteBranch',
      params: {
        worktree: `id:${wt.id}`,
        branchName: 'feature/nested',
        expectedHead: 'abc123'
      },
      timeoutMs: 15_000
    })
    expect(mockApi.worktrees.forceDeletePreservedBranch).not.toHaveBeenCalled()
  })

  it('suppresses per-branch feedback for an aggregate delete', async () => {
    const store = createTestStore()
    const wt = makeWorktree({ id: 'repo1::/path/wt1', repoId: 'repo1', path: '/path/wt1' })
    store.setState({ worktreesByRepo: { repo1: [wt] } } as Partial<AppState>)
    vi.mocked(toast.success).mockClear()
    vi.mocked(toast.error).mockClear()

    const result = await store
      .getState()
      .forceDeletePreservedBranch(wt.id, 'feature/test', 'abc123', { suppressToast: true })

    expect(result).toEqual({ ok: true, deleted: true })
    expect(toast.success).not.toHaveBeenCalled()
    expect(toast.error).not.toHaveBeenCalled()
  })

  it('fails preserved branch deletion closed for two HUB owners', async () => {
    const store = createTestStore()
    const worktreeId = 'repo-ssh::/srv/same-wt'
    store.setState({
      worktreesByRepo: {
        'repo-ssh': [
          makeWorktree({
            id: worktreeId,
            repoId: 'repo-ssh',
            hostId: 'ssh:same-private-target',
            runtimeOwnerEnvironmentId: 'hub-a'
          }),
          makeWorktree({
            id: worktreeId,
            repoId: 'repo-ssh',
            hostId: 'ssh:same-private-target',
            runtimeOwnerEnvironmentId: 'hub-b'
          })
        ]
      }
    } as Partial<AppState>)

    const result = await store
      .getState()
      .forceDeletePreservedBranch(worktreeId, 'feature/nested', 'abc123')

    expect(result).toEqual({
      ok: false,
      error: 'Workspace identity is ambiguous across hosts. Refresh projects and try again.'
    })
    expect(runtimeEnvironmentCall).not.toHaveBeenCalled()
    expect(mockApi.worktrees.forceDeletePreservedBranch).not.toHaveBeenCalled()
  })

  it('persists SSH-owned worktree metadata through local IPC even when a runtime is focused', async () => {
    const store = createTestStore()
    const wt = makeWorktree({
      id: 'repo-ssh::/home/orca/wt1',
      repoId: 'repo-ssh',
      path: '/home/orca/wt1',
      hostId: 'ssh:ssh-1'
    })
    store.setState({
      settings: { activeRuntimeEnvironmentId: 'env-1' } as never,
      repos: [
        {
          id: 'repo-ssh',
          path: '/home/orca/repo',
          displayName: 'SSH Repo',
          badgeColor: '#000',
          addedAt: 0,
          connectionId: 'ssh-1'
        }
      ],
      worktreesByRepo: { 'repo-ssh': [wt] }
    } as Partial<AppState>)

    await store.getState().updateWorktreeMeta(wt.id, { comment: 'ssh note' })

    expect(mockApi.worktrees.updateMeta).toHaveBeenCalledWith({
      worktreeId: wt.id,
      executionHostId: 'ssh:ssh-1',
      updates: expect.objectContaining({ comment: 'ssh note' })
    })
    expect(runtimeEnvironmentCall).not.toHaveBeenCalled()
    expect(store.getState().worktreesByRepo['repo-ssh'][0]?.comment).toBe('ssh note')
  })

  it('clears pending first-agent rename when the title is updated', async () => {
    const store = createTestStore()
    const wt = makeWorktree({
      id: 'repo1::/path/wt1',
      repoId: 'repo1',
      path: '/path/wt1',
      displayName: 'Nautilus',
      pendingFirstAgentMessageRename: true
    })
    store.setState({
      worktreesByRepo: { repo1: [wt] }
    } as Partial<AppState>)

    await store.getState().updateWorktreeMeta(wt.id, { displayName: 'Fix auth' })

    expect(mockApi.worktrees.updateMeta).toHaveBeenCalledWith({
      worktreeId: wt.id,
      executionHostId: 'local',
      updates: {
        displayName: 'Fix auth',
        displayNameIsPinned: true,
        pendingFirstAgentMessageRename: false,
        firstAgentMessageRenameError: null
      }
    })
    expect(store.getState().worktreesByRepo.repo1[0]).toMatchObject({
      displayName: 'Fix auth',
      pendingFirstAgentMessageRename: false,
      firstAgentMessageRenameError: null
    })
  })

  it('keeps other provider links untouched when removing linked GitHub PR metadata', async () => {
    const store = createTestStore()
    const wt = makeWorktree({
      id: 'repo1::/path/wt1',
      repoId: 'repo1',
      path: '/path/wt1',
      branch: 'refs/heads/review-branch',
      linkedPR: 456,
      linkedGitLabMR: 789
    })
    store.setState({
      repos: [
        { id: 'repo1', path: '/repo1', displayName: 'Repo 1', badgeColor: '#000', addedAt: 0 }
      ],
      worktreesByRepo: { repo1: [wt] }
    } as Partial<AppState>)

    await store.getState().updateWorktreeMeta(wt.id, { linkedPR: null })

    expect(store.getState().worktreesByRepo.repo1[0]).toMatchObject({
      linkedPR: null,
      linkedGitLabMR: 789
    })
  })

  it('applies batch metadata updates in one store transition', async () => {
    const store = createTestStore()
    const first = makeWorktree({ id: 'repo1::/path/wt1', repoId: 'repo1', path: '/path/wt1' })
    const second = makeWorktree({ id: 'repo1::/path/wt2', repoId: 'repo1', path: '/path/wt2' })
    const subscriber = vi.fn()
    store.setState({
      worktreesByRepo: { repo1: [first, second] },
      sortEpoch: 7
    } as Partial<AppState>)

    const unsubscribe = store.subscribe(subscriber)
    await store.getState().updateWorktreesMeta([
      { worktreeId: first.id, updates: { workspaceStatus: 'in-review' } },
      { worktreeId: second.id, updates: { workspaceStatus: 'completed' } }
    ])
    unsubscribe()

    expect(store.getState().worktreesByRepo.repo1.map((w) => w.workspaceStatus)).toEqual([
      'in-review',
      'completed'
    ])
    expect(store.getState().sortEpoch).toBe(8)
    expect(subscriber).toHaveBeenCalledTimes(1)
    expect(mockApi.worktrees.updateMeta).toHaveBeenCalledTimes(2)
  })

  it('persists a same-id manual rank to every owning host', async () => {
    const store = createTestStore()
    const worktreeId = 'repo1::/same/path'
    const local = makeWorktree({ id: worktreeId, repoId: 'repo1', hostId: 'local' })
    const remote = makeWorktree({
      id: worktreeId,
      repoId: 'repo1',
      hostId: 'runtime:env-1'
    })
    runtimeEnvironmentCall.mockResolvedValue({
      id: 'rpc-set-manual-order',
      ok: true,
      result: { worktree: { ...remote, manualOrder: 9000 } },
      _meta: { runtimeId: 'runtime-remote' }
    })
    store.setState({ worktreesByRepo: { repo1: [local, remote] } } as Partial<AppState>)

    await store.getState().updateWorktreesMeta([{ worktreeId, updates: { manualOrder: 9000 } }])

    expect(store.getState().worktreesByRepo.repo1.map((row) => row.manualOrder)).toEqual([
      9000, 9000
    ])
    expect(mockApi.worktrees.updateMeta).toHaveBeenCalledWith({
      worktreeId,
      executionHostId: 'local',
      updates: { manualOrder: 9000 }
    })
    expect(runtimeEnvironmentCall).toHaveBeenCalledWith({
      selector: 'env-1',
      method: 'worktree.set',
      params: { worktree: `id:${worktreeId}`, manualOrder: 9000 },
      timeoutMs: 15_000
    })
  })
  it('persists an explicit bulk host update to only the selected owner', async () => {
    const store = createTestStore()
    const worktreeId = 'repo1::/same/path'
    const local = makeWorktree({ id: worktreeId, repoId: 'repo1', hostId: 'local' })
    const remote = makeWorktree({
      id: worktreeId,
      repoId: 'repo1',
      hostId: 'runtime:env-1'
    })
    runtimeEnvironmentCall.mockResolvedValue({
      id: 'rpc-set-selected-owner',
      ok: true,
      result: { worktree: { ...remote, comment: 'selected' } },
      _meta: { runtimeId: 'runtime-remote' }
    })
    store.setState({ worktreesByRepo: { repo1: [local, remote] } } as Partial<AppState>)

    await store
      .getState()
      .updateWorktreesMeta([
        { worktreeId, updates: { comment: 'selected' }, executionHostId: 'runtime:env-1' }
      ])

    expect(store.getState().worktreesByRepo.repo1).toEqual([
      expect.objectContaining({ hostId: 'local', comment: '' }),
      expect.objectContaining({ hostId: 'runtime:env-1', comment: 'selected' })
    ])
    expect(mockApi.worktrees.updateMeta).not.toHaveBeenCalled()
    expect(runtimeEnvironmentCall).toHaveBeenCalledWith(
      expect.objectContaining({
        selector: 'env-1',
        method: 'worktree.set',
        params: expect.objectContaining({ comment: 'selected' })
      })
    )
  })
})
