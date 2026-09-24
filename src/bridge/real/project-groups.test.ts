import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import type { ProjectGroupsApi } from '../../shared/preload-api/api/repository-api'
import { createProjectGroupsRealApi } from './project-groups'

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }))
vi.mock('@tauri-apps/api/event', () => ({ listen: vi.fn() }))

const invokeMock = vi.mocked(invoke)
const listenMock = vi.mocked(listen)

type ProjectGroupsMethod = keyof ProjectGroupsApi

beforeEach(() => {
  invokeMock.mockReset()
  listenMock.mockReset()
})

const argCases = [
  { method: 'create', command: 'project_groups_create', args: { name: 'Group' } },
  {
    method: 'update',
    command: 'project_groups_update',
    args: { groupId: 'g1', updates: { name: 'Renamed' } }
  },
  { method: 'delete', command: 'project_groups_delete', args: { groupId: 'g1' } },
  {
    method: 'moveProject',
    command: 'project_groups_move_project',
    args: { projectId: 'r1', groupId: null }
  },
  { method: 'scanNested', command: 'project_groups_scan_nested', args: { path: '/root' } },
  {
    method: 'cancelNestedScan',
    command: 'project_groups_cancel_nested_scan',
    args: { scanId: 's1' }
  },
  {
    method: 'importNested',
    command: 'project_groups_import_nested',
    args: { parentPath: '/root', groupName: 'G', projectPaths: ['/root/a'], mode: 'group' }
  }
] satisfies Array<{
  method: ProjectGroupsMethod
  command: string
  args: Record<string, unknown>
}>

describe('projectGroups real adapter commands', () => {
  it('exposes every contract method', () => {
    const projectGroups = createProjectGroupsRealApi()
    for (const method of [
      'list',
      'create',
      'update',
      'delete',
      'moveProject',
      'scanNested',
      'cancelNestedScan',
      'onNestedScanProgress',
      'importNested'
    ] satisfies ProjectGroupsMethod[]) {
      expect(typeof projectGroups[method]).toBe('function')
    }
  })

  it.each(argCases)('maps $method to $command with the { args } envelope', async ({
    method,
    command,
    args
  }) => {
    invokeMock.mockResolvedValueOnce(null)
    const projectGroups = createProjectGroupsRealApi() as unknown as Record<
      string,
      (callArgs: unknown) => Promise<unknown>
    >
    await projectGroups[method](args)
    expect(invokeMock).toHaveBeenCalledWith(command, { args })
  })

  it('maps list to project_groups_list without a payload', async () => {
    invokeMock.mockResolvedValueOnce([])
    await expect(createProjectGroupsRealApi().list()).resolves.toEqual([])
    expect(invokeMock).toHaveBeenCalledWith('project_groups_list')
  })

  it('maps a {message} rejection to a normal Error', async () => {
    invokeMock.mockRejectedValueOnce({ message: 'invalid_project_group_create_args' })
    const rejection = createProjectGroupsRealApi().create({ name: '' })
    await expect(rejection).rejects.toBeInstanceOf(Error)
    await expect(rejection).rejects.toThrow('invalid_project_group_create_args')
  })
})

describe('projectGroups real adapter events', () => {
  it('subscribes to project-groups:scan-nested-progress with { scanId, scan }', async () => {
    const unlisten = vi.fn()
    const handlers: Array<(event: { payload: unknown }) => void> = []
    listenMock.mockImplementationOnce(async (_event, callback) => {
      handlers.push(callback as (event: { payload: unknown }) => void)
      return unlisten
    })
    const callback = vi.fn()
    const scan = { selectedPath: '/root', repos: [] }

    const unsubscribe = createProjectGroupsRealApi().onNestedScanProgress(callback)

    expect(listenMock).toHaveBeenCalledWith(
      'project-groups:scan-nested-progress',
      expect.any(Function)
    )
    handlers[0]?.({ payload: { scanId: 's1', scanned: 4, found: 1, scan } })
    expect(callback).toHaveBeenCalledWith({ scanId: 's1', scan })

    unsubscribe()
    await Promise.resolve()
    expect(unlisten).toHaveBeenCalledTimes(1)
  })
})
