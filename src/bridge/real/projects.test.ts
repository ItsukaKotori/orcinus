import { invoke } from '@tauri-apps/api/core'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import type { Repo } from '../../shared/repo-types'
import { UnimplementedBridgeError } from '../unimplemented-fallback'
import { createProjectsRealApi } from './projects'

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }))

const invokeMock = vi.mocked(invoke)

beforeEach(() => {
  invokeMock.mockReset()
})

function repoFixture(overrides: Partial<Repo> = {}): Repo {
  return {
    id: 'r1',
    path: '/repo',
    displayName: 'Repo',
    badgeColor: '#737373',
    addedAt: 1_000,
    kind: 'git',
    ...overrides
  }
}

describe('projects real adapter projection', () => {
  it('projects repos_list into projects', async () => {
    invokeMock.mockResolvedValueOnce([repoFixture()])

    await expect(createProjectsRealApi().list()).resolves.toEqual([
      {
        id: 'repo:r1',
        displayName: 'Repo',
        badgeColor: '#737373',
        kind: 'git',
        sourceRepoIds: ['r1'],
        createdAt: 1_000,
        updatedAt: 1_000
      }
    ])
    expect(invokeMock).toHaveBeenCalledWith('repos_list')
  })

  it('projects repos_list into host setups', async () => {
    invokeMock.mockResolvedValueOnce([repoFixture()])

    await expect(createProjectsRealApi().listHostSetups()).resolves.toEqual([
      expect.objectContaining({
        id: 'r1',
        projectId: 'repo:r1',
        hostId: 'local',
        repoId: 'r1',
        path: '/repo',
        setupState: 'ready',
        setupMethod: 'legacy-repo'
      })
    ])
  })

  it('groups two repos that resolve to one project identity', async () => {
    const upstream = { owner: 'orcinus', repo: 'ade' }
    invokeMock.mockResolvedValueOnce([
      repoFixture({ id: 'r1', addedAt: 1_000, upstream }),
      repoFixture({ id: 'r2', addedAt: 2_000, upstream })
    ])

    const projects = await createProjectsRealApi().list()

    expect(projects).toHaveLength(1)
    expect(projects[0]?.id).toBe('github:orcinus/ade')
    expect(projects[0]?.sourceRepoIds).toEqual(['r1', 'r2'])
    expect(projects[0]?.createdAt).toBe(1_000)
    expect(projects[0]?.updatedAt).toBe(2_000)
  })

  it('maps a {message} rejection to a normal Error', async () => {
    invokeMock.mockRejectedValueOnce({ message: 'store unreadable' })
    const rejection = createProjectsRealApi().list()
    await expect(rejection).rejects.toBeInstanceOf(Error)
    await expect(rejection).rejects.toThrow('store unreadable')
  })
})

describe('projects real adapter update', () => {
  it('returns the projected project with the preference applied in memory', async () => {
    invokeMock.mockResolvedValueOnce([repoFixture()])

    const updated = await createProjectsRealApi().update({
      projectId: 'repo:r1',
      updates: { localWindowsRuntimePreference: { kind: 'windows-host' } }
    })

    expect(updated).toMatchObject({
      id: 'repo:r1',
      displayName: 'Repo',
      localWindowsRuntimePreference: { kind: 'windows-host' }
    })
    // Why: only the read path crosses IPC; the preference is not persisted in A.
    expect(invokeMock).toHaveBeenCalledTimes(1)
    expect(invokeMock).toHaveBeenCalledWith('repos_list')
  })

  it('clears the preference when the update passes undefined', async () => {
    invokeMock.mockResolvedValueOnce([repoFixture()])

    const updated = await createProjectsRealApi().update({
      projectId: 'repo:r1',
      updates: { localWindowsRuntimePreference: undefined }
    })

    expect(updated).not.toHaveProperty('localWindowsRuntimePreference')
  })

  it('returns null when no project matches the id', async () => {
    invokeMock.mockResolvedValueOnce([repoFixture()])

    await expect(
      createProjectsRealApi().update({
        projectId: 'repo:missing',
        updates: { localWindowsRuntimePreference: { kind: 'windows-host' } }
      })
    ).resolves.toBeNull()
  })
})

describe('projects real adapter unimplemented surface', () => {
  it.each(['createHostSetup', 'setupExistingFolder', 'updateHostSetup', 'deleteHostSetup'] as const)(
    'rejects %s with UnimplementedBridgeError',
    async (method) => {
      const warn = vi.spyOn(console, 'warn').mockImplementation(() => {})
      const projects = createProjectsRealApi() as unknown as Record<
        string,
        (callArgs: unknown) => Promise<unknown>
      >
      await expect(projects[method]({})).rejects.toBeInstanceOf(UnimplementedBridgeError)
      expect(invokeMock).not.toHaveBeenCalled()
      warn.mockRestore()
    }
  )
})
