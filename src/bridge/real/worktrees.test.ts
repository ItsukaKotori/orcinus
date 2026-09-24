import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import type { WorktreeApi } from '../../shared/preload-api/api/worktree-api'
import { UnimplementedBridgeError } from '../unimplemented-fallback'
import { createWorktreesRealApi } from './worktrees'

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }))
vi.mock('@tauri-apps/api/event', () => ({ listen: vi.fn() }))

const invokeMock = vi.mocked(invoke)
const listenMock = vi.mocked(listen)

type WorktreesMethod = keyof WorktreeApi

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
})

describe('worktrees real adapter unimplemented surface', () => {
  it.each([
    'listRetiredNames',
    'listDetected',
    'create',
    'remove',
    'prefetchCreateBase',
    'listLineage',
    'onCreateProgress',
    'onGitStatusMetadataChanged'
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
