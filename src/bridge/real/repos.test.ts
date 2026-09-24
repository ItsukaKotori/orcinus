import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import type { RepositoryApi } from '../../shared/preload-api/api/repository-api'
import { UnimplementedBridgeError } from '../unimplemented-fallback'
import { createReposRealApi } from './repos'

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }))
vi.mock('@tauri-apps/api/event', () => ({ listen: vi.fn() }))

const invokeMock = vi.mocked(invoke)
const listenMock = vi.mocked(listen)

type ReposMethod = keyof RepositoryApi

beforeEach(() => {
  invokeMock.mockReset()
  listenMock.mockReset()
})

const argCases = [
  { method: 'add', command: 'repos_add', args: { path: '/tmp/x' } },
  {
    method: 'update',
    command: 'repos_update',
    args: { repoId: 'r1', updates: { displayName: 'Renamed' } }
  },
  { method: 'remove', command: 'repos_remove', args: { repoId: 'r1' } },
  {
    method: 'reorderForHost',
    command: 'repos_reorder_for_host',
    args: { orderedIds: ['r1', 'r2'], hostId: 'local' }
  }
] satisfies Array<{ method: ReposMethod; command: string; args: Record<string, unknown> }>

const noArgCases = [
  { method: 'list', command: 'repos_list' },
  { method: 'pickFolder', command: 'repos_pick_folder' },
  { method: 'pickFolders', command: 'repos_pick_folders' },
  { method: 'pickDirectory', command: 'repos_pick_directory' },
  { method: 'isGitAvailable', command: 'repos_is_git_available' },
  {
    method: 'getDefaultCreateProjectParent',
    command: 'repos_get_default_create_project_parent'
  }
] satisfies Array<{ method: ReposMethod; command: string }>

describe('repos real adapter commands', () => {
  it.each(argCases)('maps $method to $command with the { args } envelope', async ({
    method,
    command,
    args
  }) => {
    invokeMock.mockResolvedValueOnce(null)
    const repos = createReposRealApi() as unknown as Record<
      string,
      (callArgs: unknown) => Promise<unknown>
    >
    await repos[method](args)
    expect(invokeMock).toHaveBeenCalledWith(command, { args })
  })

  it.each(noArgCases)('maps $method to $command without a payload', async ({
    method,
    command
  }) => {
    invokeMock.mockResolvedValueOnce(null)
    const repos = createReposRealApi() as unknown as Record<string, () => Promise<unknown>>
    await repos[method]()
    expect(invokeMock).toHaveBeenCalledWith(command)
  })

  it('maps a {message} rejection to a normal Error', async () => {
    invokeMock.mockRejectedValueOnce({ message: 'Repo not found: r1' })
    const rejection = createReposRealApi().update({ repoId: 'r1', updates: {} })
    await expect(rejection).rejects.toBeInstanceOf(Error)
    await expect(rejection).rejects.toThrow('Repo not found: r1')
  })
})

describe('repos real adapter events', () => {
  it('subscribes to repos:changed and returns an unsubscriber', async () => {
    const unlisten = vi.fn()
    const handlers: Array<(event: { payload: unknown }) => void> = []
    listenMock.mockImplementationOnce(async (_event, callback) => {
      handlers.push(callback as (event: { payload: unknown }) => void)
      return unlisten
    })
    const callback = vi.fn()

    const unsubscribe = createReposRealApi().onChanged(callback)

    expect(listenMock).toHaveBeenCalledWith('repos:changed', expect.any(Function))
    handlers[0]?.({ payload: null })
    expect(callback).toHaveBeenCalledTimes(1)

    unsubscribe()
    await Promise.resolve()
    expect(unlisten).toHaveBeenCalledTimes(1)
  })
})

describe('repos real adapter unimplemented surface', () => {
  it.each([
    'clone',
    'cloneRemote',
    'createRemote',
    'addRemote',
    'create',
    'cloneAbort',
    'getGitUsername',
    'getBaseRefDefault',
    'searchBaseRefs',
    'searchBaseRefDetails',
    'reorder',
    'removeForHost'
  ] satisfies ReposMethod[])('rejects %s with UnimplementedBridgeError', async (method) => {
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => {})
    const repos = createReposRealApi() as unknown as Record<
      string,
      (callArgs?: unknown) => Promise<unknown>
    >
    await expect(repos[method]({})).rejects.toBeInstanceOf(UnimplementedBridgeError)
    expect(invokeMock).not.toHaveBeenCalled()
    warn.mockRestore()
  })
})
