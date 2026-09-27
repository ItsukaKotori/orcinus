import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import type { WorktreeApi } from '../../shared/preload-api/api/worktree-api'
import type { ProviderRequestId } from '../../shared/detected-worktree-provider-contract'
import type { Worktree } from '../../shared/worktree/types'
import { UnimplementedBridgeError } from '../unimplemented-fallback'
import { createWorktreesRealApi } from './worktrees'

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }))
vi.mock('@tauri-apps/api/event', () => ({ listen: vi.fn() }))

const invokeMock = vi.mocked(invoke)
const listenMock = vi.mocked(listen)

type WorktreesMethod = keyof WorktreeApi

const mainWorktree: Worktree = {
  id: 'r1::/repo',
  repoId: 'r1',
  path: '/repo',
  head: 'abc',
  branch: 'refs/heads/main',
  isBare: false,
  isMainWorktree: true,
  displayName: 'main',
  comment: '',
  linkedIssue: null,
  linkedPR: null,
  linkedLinearIssue: null,
  isArchived: false,
  isUnread: false,
  isPinned: false,
  sortOrder: 0,
  lastActivityAt: 0
}

const linkedWorktree: Worktree = {
  ...mainWorktree,
  id: 'r1::/repo/feature',
  path: '/repo/feature',
  branch: 'refs/heads/feature',
  isMainWorktree: false,
  displayName: 'feature'
}

beforeEach(() => {
  invokeMock.mockReset()
  listenMock.mockReset()
})

describe('worktrees real adapter commands', () => {
  it('maps list to worktrees_list with the { args } envelope', async () => {
    invokeMock.mockResolvedValueOnce([])
    await expect(createWorktreesRealApi().list({ repoId: 'r1' })).resolves.toEqual([])
    expect(invokeMock).toHaveBeenCalledWith('worktrees_list', { args: { repoId: 'r1' } })
  })

  it('maps listAll to worktrees_list_all without a payload', async () => {
    invokeMock.mockResolvedValueOnce([])
    await expect(createWorktreesRealApi().listAll()).resolves.toEqual([])
    expect(invokeMock).toHaveBeenCalledWith('worktrees_list_all')
  })

  it('maps a {message} rejection to a normal Error', async () => {
    invokeMock.mockRejectedValueOnce({ message: 'git worktree list failed' })
    await expect(createWorktreesRealApi().list({ repoId: 'r1' })).rejects.toBeInstanceOf(Error)
    invokeMock.mockRejectedValueOnce({ message: 'git worktree list failed' })
    await expect(createWorktreesRealApi().list({ repoId: 'r1' })).rejects.toThrow(
      'git worktree list failed'
    )
  })
})

describe('worktrees real adapter detected listing', () => {
  it('maps a provider request to worktrees_list and answers the legacy detected shape', async () => {
    invokeMock.mockResolvedValueOnce([mainWorktree, linkedWorktree])

    const result = await createWorktreesRealApi().listDetected({
      providerRequestId: 'pr1' as ProviderRequestId,
      repoId: 'r1',
      executionHostId: 'local'
    })

    expect(invokeMock).toHaveBeenCalledWith('worktrees_list', { args: { repoId: 'r1' } })
    expect(result).toEqual({
      repoId: 'r1',
      authoritative: true,
      source: 'git',
      worktrees: [
        { ...mainWorktree, ownership: 'orca-managed', selectedCheckout: true, visible: true },
        { ...linkedWorktree, ownership: 'orca-managed', selectedCheckout: false, visible: true }
      ]
    })
  })

  it('accepts the legacy { repoId } request shape', async () => {
    invokeMock.mockResolvedValueOnce([])

    await expect(createWorktreesRealApi().listDetected({ repoId: 'r1' })).resolves.toEqual({
      repoId: 'r1',
      authoritative: true,
      source: 'git',
      worktrees: []
    })
    expect(invokeMock).toHaveBeenCalledWith('worktrees_list', { args: { repoId: 'r1' } })
  })

  it('answers listKnownForExecutionHost with a rejected result instead of throwing', async () => {
    const result = await createWorktreesRealApi().listKnownForExecutionHost?.({
      repoId: 'r1',
      executionHostId: 'ssh:box'
    })
    expect(result).toEqual({ status: 'rejected', repoId: 'r1', executionHostId: 'ssh:box' })
    expect(invokeMock).not.toHaveBeenCalled()
  })

  it('answers forgetRemovedForExecutionHost without pretending to forget', async () => {
    const result = await createWorktreesRealApi().forgetRemovedForExecutionHost?.({
      repoId: 'r1',
      executionHostId: 'ssh:box',
      worktreeIds: ['r1::/gone']
    })
    expect(result).toEqual({ forgottenWorktreeIds: [] })
    expect(invokeMock).not.toHaveBeenCalled()
  })

  it('cancels provider requests as a resolved no-op', async () => {
    await expect(
      createWorktreesRealApi().cancelListDetected?.({ providerRequestId: 'pr1' as ProviderRequestId })
    ).resolves.toBeUndefined()
    expect(invokeMock).not.toHaveBeenCalled()
  })
})

describe('worktrees real adapter lineage', () => {
  it('answers listLineage with an empty local lineage map without invoking', async () => {
    await expect(createWorktreesRealApi().listLineage()).resolves.toEqual({ lineage: {} })
    expect(invokeMock).not.toHaveBeenCalled()
  })
})

describe('worktrees real adapter events', () => {
  it('subscribes to worktrees:changed with { repoId } and returns an unsubscriber', async () => {
    const unlisten = vi.fn()
    const handlers: Array<(event: { payload: unknown }) => void> = []
    listenMock.mockImplementationOnce(async (_event, callback) => {
      handlers.push(callback as (event: { payload: unknown }) => void)
      return unlisten
    })
    const callback = vi.fn()

    const unsubscribe = createWorktreesRealApi().onChanged(callback)

    expect(listenMock).toHaveBeenCalledWith('worktrees:changed', expect.any(Function))
    handlers[0]?.({ payload: { repoId: 'r1' } })
    expect(callback).toHaveBeenCalledWith({ repoId: 'r1' })

    unsubscribe()
    await Promise.resolve()
    expect(unlisten).toHaveBeenCalledTimes(1)
  })

  it.each([
    'onHeadIdentitiesChanged',
    'onBaseStatus',
    'onRemoteBranchConflict',
    'onCreateProgress',
    'onGitStatusMetadataChanged'
  ] satisfies WorktreesMethod[])(
    '%s hands back a no-op unsubscriber without listening',
    (method) => {
      const subscribe = createWorktreesRealApi() as unknown as Record<
        string,
        (callback: () => void) => () => void
      >
      const unsubscribe = subscribe[method](() => {})
      expect(typeof unsubscribe).toBe('function')
      expect(listenMock).not.toHaveBeenCalled()
      expect(() => unsubscribe()).not.toThrow()
    }
  )
})

describe('worktrees real adapter unimplemented surface', () => {
  it.each([
    'listRetiredNames',
    'create',
    'remove',
    'prefetchCreateBase',
    'updateMeta',
    'resolvePrBase'
  ] satisfies WorktreesMethod[])('rejects %s with UnimplementedBridgeError', async (method) => {
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => {})
    const worktrees = createWorktreesRealApi() as unknown as Record<
      string,
      (callArgs?: unknown) => Promise<unknown>
    >
    await expect(worktrees[method]({})).rejects.toBeInstanceOf(UnimplementedBridgeError)
    expect(invokeMock).not.toHaveBeenCalled()
    warn.mockRestore()
  })
})
