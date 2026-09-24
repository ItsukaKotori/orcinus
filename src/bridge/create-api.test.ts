import { invoke } from '@tauri-apps/api/core'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { createAdeApi } from './create-api'

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }))
vi.mock('@tauri-apps/api/event', () => ({ listen: vi.fn() }))

const invokeMock = vi.mocked(invoke)

beforeEach(() => {
  invokeMock.mockReset()
})

afterEach(() => {
  vi.unstubAllEnvs()
})

describe('createAdeApi mock mode', () => {
  it('exposes mocked namespaces with live data', async () => {
    const api = createAdeApi({ mode: 'mock' })
    // SAFETY: PluginHostListEntry has no `scope`; the runtime entries contributed by the mock do.
    const entries = (await api.plugins.list()) as Array<{ scope?: string }>
    expect(entries.some((entry) => entry.scope === 'global')).toBe(true)
    expect(entries.some((entry) => entry.scope === 'project')).toBe(true)
  })

  it('platform.get returns platform info synchronously, not a rejection promise', () => {
    const api = createAdeApi({ mode: 'mock' })
    const info = api.platform.get()
    expect(info).not.toBeInstanceOf(Promise)
    expect(['darwin', 'win32', 'linux']).toContain(info.platform)
    expect(info.displayServer).toBeNull()
  })

  it('browser WebAuthn subscriptions hand back unsubscribe functions', () => {
    const api = createAdeApi({ mode: 'mock' })
    const stopRequests = api.browser.onWebAuthnAccountRequest(() => {})
    expect(typeof stopRequests).toBe('function')
    stopRequests()
    const stopClosed = api.browser.onWebAuthnAccountRequestClosed(() => {})
    expect(typeof stopClosed).toBe('function')
    stopClosed()
  })
})

describe('createAdeApi mode resolution', () => {
  it('defaults to the real bridge', async () => {
    const api = createAdeApi()
    invokeMock.mockResolvedValueOnce([])
    await expect(api.repos.list()).resolves.toEqual([])
    expect(invokeMock).toHaveBeenCalledWith('repos_list')
  })

  it('falls back to the full mock bridge when VITE_ADE_BRIDGE=mock', async () => {
    vi.stubEnv('VITE_ADE_BRIDGE', 'mock')
    const api = createAdeApi()
    await expect(api.repos.list()).resolves.toEqual([])
    expect(api.platform.get()).not.toBeInstanceOf(Promise)
    expect(invokeMock).not.toHaveBeenCalled()
  })

  it('honors an explicit mode over the environment', async () => {
    vi.stubEnv('VITE_ADE_BRIDGE', 'mock')
    const api = createAdeApi({ mode: 'real' })
    invokeMock.mockResolvedValueOnce([])
    await api.repos.list()
    expect(invokeMock).toHaveBeenCalledWith('repos_list')
  })
})

describe('createAdeApi real assembly', () => {
  it('routes the ten real domains through their commands', async () => {
    const api = createAdeApi()
    invokeMock.mockResolvedValue([])

    await api.repos.list()
    await api.fs.readDir({ dirPath: '/repo' })
    await api.projects.list()
    await api.projectGroups.list()
    await api.folderWorkspaces.list()
    await api.settings.get()
    await api.ui.get()
    await api.worktrees.listAll()
    await api.app.getIdentity()

    expect(invokeMock.mock.calls.map(([command]) => command)).toEqual([
      'repos_list',
      'fs_read_dir',
      'repos_list',
      'project_groups_list',
      'folder_workspaces_list',
      'settings_get',
      'ui_get',
      'worktrees_list_all',
      'app_get_identity'
    ])
  })

  it('assembles the real platform adapter from the bootstrap payload', () => {
    const api = createAdeApi()
    expect(() => api.platform.get()).toThrow(/bootstrap/)
  })

  it('keeps unported namespaces on the mock implementations', async () => {
    const api = createAdeApi()
    await expect(api.plugins.list()).resolves.toEqual(expect.any(Array))
    await expect(api.agentAwake.getStatus()).resolves.toEqual({ mode: 'off', active: false })
    expect(invokeMock).not.toHaveBeenCalled()
  })

  it('rejects unknown namespaces with the unimplemented fallback', async () => {
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => {})
    const api = createAdeApi() as unknown as { notADomain: { ping: () => Promise<unknown> } }
    await expect(api.notADomain.ping()).rejects.toMatchObject({
      name: 'UnimplementedBridgeError'
    })
    warn.mockRestore()
  })
})
