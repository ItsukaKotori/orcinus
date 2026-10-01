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
  },
  {
    method: 'create',
    command: 'repos_create',
    args: { parentPath: '/tmp', name: 'calm-otter', kind: 'git' }
  },
  {
    method: 'getBaseRefDefault',
    command: 'repos_get_base_ref_default',
    args: { repoId: 'r1' }
  },
  {
    method: 'searchBaseRefs',
    command: 'repos_search_base_refs',
    args: { repoId: 'r1', query: 'main' }
  },
  {
    method: 'searchBaseRefDetails',
    command: 'repos_search_base_ref_details',
    args: { repoId: 'r1', query: 'main' }
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

  it('passes the {error} contract union through without throwing', async () => {
    const errorResult = { error: 'Not a valid git repository: /tmp/x' }
    invokeMock.mockResolvedValueOnce(errorResult)
    await expect(createReposRealApi().add({ path: '/tmp/x' })).resolves.toEqual(errorResult)
  })

  it('passes a repos.create {error} result through without rejecting', async () => {
    const errorResult = { error: 'Path already exists: /tmp/calm-otter' }
    invokeMock.mockResolvedValueOnce(errorResult)
    await expect(
      createReposRealApi().create({ parentPath: '/tmp', name: 'calm-otter', kind: 'git' })
    ).resolves.toBe(errorResult)
  })

  it('passes a repos.create {repo} result through', async () => {
    const repoResult = { repo: { id: 'r1', path: '/tmp/calm-otter' } }
    invokeMock.mockResolvedValueOnce(repoResult)
    await expect(
      createReposRealApi().create({ parentPath: '/tmp', name: 'calm-otter', kind: 'git' })
    ).resolves.toBe(repoResult)
  })

  it('passes the base-ref helper payloads through unchanged', async () => {
    const baseRefDefault = { defaultBaseRef: 'main', remoteCount: 1 }
    invokeMock.mockResolvedValueOnce(baseRefDefault)
    await expect(createReposRealApi().getBaseRefDefault({ repoId: 'r1' })).resolves.toBe(
      baseRefDefault
    )

    const refs = ['main', 'origin/main']
    invokeMock.mockResolvedValueOnce(refs)
    await expect(
      createReposRealApi().searchBaseRefs({ repoId: 'r1', query: 'main' })
    ).resolves.toBe(refs)

    const details = [{ refName: 'origin/main', localBranchName: 'main' }]
    invokeMock.mockResolvedValueOnce(details)
    await expect(
      createReposRealApi().searchBaseRefDetails({ repoId: 'r1', query: 'main' })
    ).resolves.toBe(details)
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

  it('treats onCloneProgress as a no-op subscription without listening', () => {
    const unsubscribe = createReposRealApi().onCloneProgress(() => {})
    expect(typeof unsubscribe).toBe('function')
    expect(listenMock).not.toHaveBeenCalled()
    expect(() => unsubscribe()).not.toThrow()
  })
})

describe('repos real adapter unimplemented surface', () => {
  it.each([
    'clone',
    'cloneRemote',
    'createRemote',
    'addRemote',
    'cloneAbort',
    'getGitUsername',
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
