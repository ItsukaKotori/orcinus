import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import type { FilesystemApi } from '../../shared/preload-api/api/filesystem-api'
import { UnimplementedBridgeError } from '../unimplemented-fallback'
import { createFsRealApi } from './fs'

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }))
vi.mock('@tauri-apps/api/event', () => ({ listen: vi.fn() }))

const invokeMock = vi.mocked(invoke)
const listenMock = vi.mocked(listen)

type FsApi = FilesystemApi['fs']
type FsMethod = keyof FsApi

beforeEach(() => {
  invokeMock.mockReset()
  listenMock.mockReset()
})

const commandCases = [
  { method: 'readDir', command: 'fs_read_dir', args: { dirPath: '/repo' } },
  { method: 'readFile', command: 'fs_read_file', args: { filePath: '/repo/a.ts' } },
  {
    method: 'writeFile',
    command: 'fs_write_file',
    args: { filePath: '/repo/a.ts', content: 'x' }
  },
  { method: 'createFile', command: 'fs_create_file', args: { filePath: '/repo/a.ts' } },
  { method: 'createDir', command: 'fs_create_dir', args: { dirPath: '/repo/src' } },
  {
    method: 'rename',
    command: 'fs_rename',
    args: { oldPath: '/repo/a.ts', newPath: '/repo/b.ts' }
  },
  {
    method: 'copy',
    command: 'fs_copy',
    args: { sourcePath: '/repo/a.ts', destinationPath: '/repo/b.ts' }
  },
  { method: 'deletePath', command: 'fs_delete_path', args: { targetPath: '/repo/a.ts' } },
  { method: 'stat', command: 'fs_stat', args: { filePath: '/repo/a.ts' } },
  { method: 'pathExists', command: 'fs_path_exists', args: { filePath: '/repo/a.ts' } },
  { method: 'pathsExist', command: 'fs_paths_exist', args: { filePaths: ['/repo/a.ts'] } },
  { method: 'listFiles', command: 'fs_list_files', args: { rootPath: '/repo' } },
  { method: 'cancelListFiles', command: 'fs_cancel_list_files', args: { requestToken: 't1' } },
  {
    method: 'search',
    command: 'fs_search',
    args: { query: 'needle', rootPath: '/repo' }
  },
  { method: 'watchWorktree', command: 'fs_watch_worktree', args: { worktreePath: '/repo' } },
  { method: 'unwatchWorktree', command: 'fs_unwatch_worktree', args: { worktreePath: '/repo' } },
  {
    method: 'listMarkdownDocuments',
    command: 'fs_list_markdown_documents',
    args: { rootPath: '/repo' }
  },
  {
    method: 'authorizeExternalPath',
    command: 'fs_authorize_external_path',
    args: { targetPath: '/outside' }
  }
] satisfies Array<{ method: FsMethod; command: string; args: Record<string, unknown> }>

describe('fs real adapter commands', () => {
  it.each(commandCases)('maps $method to $command with the { args } envelope', async ({
    method,
    command,
    args
  }) => {
    invokeMock.mockResolvedValueOnce(null)
    const fs = createFsRealApi() as unknown as Record<
      string,
      (callArgs: unknown) => Promise<unknown>
    >
    await fs[method](args)
    expect(invokeMock).toHaveBeenCalledWith(command, { args })
  })

  it('maps a {message} rejection to a normal Error', async () => {
    invokeMock.mockRejectedValueOnce({
      message:
        'Access denied: path resolves outside allowed directories. If this blocks a legitimate workflow, please file a GitHub issue.'
    })
    const rejection = createFsRealApi().readFile({ filePath: '/outside/a.ts' })
    await expect(rejection).rejects.toBeInstanceOf(Error)
    await expect(rejection).rejects.toThrow(/Access denied/)
  })
})

describe('fs real adapter events', () => {
  it('subscribes to fs:changed with the payload and returns an unsubscriber', async () => {
    const unlisten = vi.fn()
    const handlers: Array<(event: { payload: unknown }) => void> = []
    listenMock.mockImplementationOnce(async (_event, callback) => {
      handlers.push(callback as (event: { payload: unknown }) => void)
      return unlisten
    })
    const callback = vi.fn()

    const unsubscribe = createFsRealApi().onFsChanged(callback)

    expect(listenMock).toHaveBeenCalledWith('fs:changed', expect.any(Function))
    const payload = { worktreePath: '/repo', events: [] }
    handlers[0]?.({ payload })
    expect(callback).toHaveBeenCalledWith(payload)

    unsubscribe()
    await Promise.resolve()
    expect(unlisten).toHaveBeenCalledTimes(1)
  })
})

describe('fs real adapter unimplemented surface', () => {
  it.each([
    ['downloadFile', { filePath: '/a', connectionId: 'ssh:1' }],
    ['readLocalLogTail', { subscriptionId: 's1' }],
    ['importExternalPaths', { sourcePaths: [], destDir: '/repo' }]
  ] as const)('rejects %s with UnimplementedBridgeError', async (method, args) => {
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => {})
    const fs = createFsRealApi() as unknown as Record<
      string,
      (callArgs: unknown) => Promise<unknown>
    >
    await expect(fs[method](args)).rejects.toBeInstanceOf(UnimplementedBridgeError)
    expect(invokeMock).not.toHaveBeenCalled()
    warn.mockRestore()
  })
})
