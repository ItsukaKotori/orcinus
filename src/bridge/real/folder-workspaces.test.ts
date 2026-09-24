import { invoke } from '@tauri-apps/api/core'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import type { FolderWorkspacesApi } from '../../shared/preload-api/api/worktree-api'
import { createFolderWorkspacesRealApi } from './folder-workspaces'

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }))
vi.mock('@tauri-apps/api/event', () => ({ listen: vi.fn() }))

const invokeMock = vi.mocked(invoke)

type FolderWorkspacesMethod = keyof FolderWorkspacesApi

beforeEach(() => {
  invokeMock.mockReset()
})

const argCases = [
  {
    method: 'create',
    command: 'folder_workspaces_create',
    args: { projectGroupId: 'g1' }
  },
  {
    method: 'update',
    command: 'folder_workspaces_update',
    args: { folderWorkspaceId: 'w1', updates: { name: 'Renamed' } }
  },
  {
    method: 'delete',
    command: 'folder_workspaces_delete',
    args: { folderWorkspaceId: 'w1' }
  },
  {
    method: 'getPathStatus',
    command: 'folder_workspaces_get_path_status',
    args: { scope: 'folder-workspace', folderWorkspaceId: 'w1' }
  }
] satisfies Array<{
  method: FolderWorkspacesMethod
  command: string
  args: Record<string, unknown>
}>

describe('folderWorkspaces real adapter commands', () => {
  it('exposes every contract method', () => {
    const folderWorkspaces = createFolderWorkspacesRealApi()
    for (const method of [
      'list',
      'getPathStatus',
      'create',
      'update',
      'delete'
    ] satisfies FolderWorkspacesMethod[]) {
      expect(typeof folderWorkspaces[method]).toBe('function')
    }
  })

  it.each(argCases)('maps $method to $command with the { args } envelope', async ({
    method,
    command,
    args
  }) => {
    invokeMock.mockResolvedValueOnce(null)
    const folderWorkspaces = createFolderWorkspacesRealApi() as unknown as Record<
      string,
      (callArgs: unknown) => Promise<unknown>
    >
    await folderWorkspaces[method](args)
    expect(invokeMock).toHaveBeenCalledWith(command, { args })
  })

  it('maps list to folder_workspaces_list without a payload', async () => {
    invokeMock.mockResolvedValueOnce([])
    await expect(createFolderWorkspacesRealApi().list()).resolves.toEqual([])
    expect(invokeMock).toHaveBeenCalledWith('folder_workspaces_list')
  })

  it('maps a {message} rejection to a normal Error', async () => {
    invokeMock.mockRejectedValueOnce({ message: 'folder_workspace_project_group_not_found' })
    const rejection = createFolderWorkspacesRealApi().create({ projectGroupId: 'missing' })
    await expect(rejection).rejects.toBeInstanceOf(Error)
    await expect(rejection).rejects.toThrow('folder_workspace_project_group_not_found')
  })
})
