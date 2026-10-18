import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import type { PluginChangeEvent } from '../../shared/plugins/plugin-change-event'
import type { PluginOpenTabRequest } from '../../shared/preload-api/api/plugin-host-api'
import { createPluginsRealApi } from './plugins'

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }))
vi.mock('@tauri-apps/api/event', () => ({ listen: vi.fn() }))

const invokeMock = vi.mocked(invoke)
const listenMock = vi.mocked(listen)

beforeEach(() => {
  invokeMock.mockReset()
  listenMock.mockReset()
})

describe('plugins real adapter routing', () => {
  it('routes list and refresh bare, without an args envelope', async () => {
    invokeMock.mockResolvedValueOnce([])
    await expect(createPluginsRealApi().list()).resolves.toEqual([])
    expect(invokeMock).toHaveBeenCalledWith('plugins_list')

    invokeMock.mockResolvedValueOnce([])
    await expect(createPluginsRealApi().refresh()).resolves.toEqual([])
    expect(invokeMock).toHaveBeenCalledWith('plugins_refresh')
  })

  it('routes consent with the PluginRef identity, fingerprint, and decision', async () => {
    invokeMock.mockResolvedValueOnce([])
    const args = {
      pluginKey: 'acme.tools',
      scope: 'project' as const,
      repoId: 'repo-1',
      reviewedFingerprint: 'fp-1',
      decision: 'approve' as const
    }
    await expect(createPluginsRealApi().consent(args)).resolves.toEqual([])
    expect(invokeMock).toHaveBeenCalledWith('plugins_consent', { args })
  })

  it('routes readPanelEntry with panelId on the args envelope', async () => {
    const entry = { html: '<html></html>', sessionToken: 'session-token' }
    invokeMock.mockResolvedValueOnce(entry)
    await expect(
      createPluginsRealApi().readPanelEntry({
        pluginKey: 'acme.tools',
        scope: 'global',
        panelId: 'main-panel'
      })
    ).resolves.toEqual(entry)
    expect(invokeMock).toHaveBeenCalledWith('plugins_read_panel_entry', {
      args: { pluginKey: 'acme.tools', scope: 'global', panelId: 'main-panel' }
    })
  })

  it('routes readTabEntry with the tab identity and owner window on the args envelope', async () => {
    const entry = { html: '<html></html>', sessionToken: 'session-token' }
    invokeMock.mockResolvedValueOnce(entry)
    const args = {
      pluginKey: 'acme.tools',
      scope: 'project' as const,
      repoId: 'repo-1',
      tabId: 'board',
      instanceId: 'inst-1',
      worktreeId: 'repo-1::/repo',
      ownerWindow: 'main'
    }
    await expect(createPluginsRealApi().readTabEntry(args)).resolves.toEqual(entry)
    expect(invokeMock).toHaveBeenCalledWith('plugins_read_tab_entry', { args })
  })

  it('passes readTabEntry bridge errors through as Error instances', async () => {
    invokeMock.mockRejectedValueOnce({
      message: 'consent_required: acme.tools is not approved for activation'
    })
    await expect(
      createPluginsRealApi().readTabEntry({
        pluginKey: 'acme.tools',
        scope: 'global',
        tabId: 'board',
        instanceId: 'inst-1',
        ownerWindow: 'main'
      })
    ).rejects.toThrow('consent_required: acme.tools is not approved for activation')
  })

  it('routes invokeCommand with the nested command args untouched', async () => {
    invokeMock.mockResolvedValueOnce({ echoed: true })
    await expect(
      createPluginsRealApi().invokeCommand({
        pluginKey: 'acme.tools',
        scope: 'project',
        repoId: 'repo-1',
        commandId: 'deploy',
        args: { target: 'staging' }
      })
    ).resolves.toEqual({ echoed: true })
    expect(invokeMock).toHaveBeenCalledWith('plugins_invoke_command', {
      args: {
        pluginKey: 'acme.tools',
        scope: 'project',
        repoId: 'repo-1',
        commandId: 'deploy',
        args: { target: 'staging' }
      }
    })
  })
})

describe('plugins real adapter panel action', () => {
  const call = {
    sessionToken: 's'.repeat(32),
    action: 'storage.get',
    params: { key: 'k' }
  }

  it('routes panelAction and normalizes an absent value to null', async () => {
    invokeMock.mockResolvedValueOnce({ ok: true })
    await expect(createPluginsRealApi().panelAction(call)).resolves.toEqual({
      ok: true,
      value: null
    })
    expect(invokeMock).toHaveBeenCalledWith('plugins_panel_action', { args: call })
  })

  it('passes contract error codes through unchanged', async () => {
    invokeMock.mockResolvedValueOnce({
      ok: false,
      code: 'capability_denied',
      error: 'capability not granted'
    })
    await expect(createPluginsRealApi().panelAction(call)).resolves.toEqual({
      ok: false,
      code: 'capability_denied',
      error: 'capability not granted'
    })
  })

  it('maps the bridge disabled_system code to the frozen unavailable code', async () => {
    invokeMock.mockResolvedValueOnce({
      ok: false,
      code: 'disabled_system',
      error: 'plugin system is disabled'
    })
    await expect(createPluginsRealApi().panelAction(call)).resolves.toEqual({
      ok: false,
      code: 'unavailable',
      error: 'plugin system is disabled'
    })
  })

  it('folds unknown bridge codes into action_failed instead of leaking them', async () => {
    invokeMock.mockResolvedValueOnce({ ok: false, code: 'hostile_code', error: 'nope' })
    await expect(createPluginsRealApi().panelAction(call)).resolves.toEqual({
      ok: false,
      code: 'action_failed',
      error: 'nope'
    })
  })
})

describe('plugins real adapter install surface', () => {
  it('routes install with the source object (not an args envelope), scope included', async () => {
    const result = {
      ok: true,
      pluginKey: 'acme.tools',
      version: '1.0.0',
      contentHash: 'a'.repeat(64),
      consentFingerprint: 'fp-1',
      resolvedCommit: 'b'.repeat(40)
    }
    invokeMock.mockResolvedValueOnce(result)
    const source = {
      kind: 'git' as const,
      url: 'https://example.com/acme.git',
      ref: 'main',
      scope: 'project' as const,
      repoId: 'repo-1'
    }
    await expect(createPluginsRealApi().install(source)).resolves.toEqual(result)
    expect(invokeMock).toHaveBeenCalledWith('plugins_install', { source })
  })

  it('routes marketplace preview and install with the reviewed identity and scope', async () => {
    const preview = {
      marketplaceSourceId: 'ms-1',
      marketplaceName: 'Acme',
      marketplaceOwner: 'acme',
      marketplaceCommit: 'a'.repeat(40),
      pluginKey: 'acme.tools',
      source: { kind: 'git', url: 'https://example.com/acme.git', ref: 'main' },
      resolvedCommit: 'b'.repeat(40),
      contentHash: 'c'.repeat(64),
      consentFingerprint: 'fp-1',
      manifest: {},
      official: true,
      bundled: false
    }
    invokeMock.mockResolvedValueOnce(preview)
    await expect(
      createPluginsRealApi().previewMarketplacePlugin({
        marketplaceSourceId: 'ms-1',
        pluginKey: 'acme.tools'
      })
    ).resolves.toEqual(preview)
    expect(invokeMock).toHaveBeenCalledWith('plugins_preview_marketplace_plugin', {
      args: { marketplaceSourceId: 'ms-1', pluginKey: 'acme.tools' }
    })

    const installArgs = {
      marketplaceSourceId: 'ms-1',
      marketplaceCommit: 'a'.repeat(40),
      pluginKey: 'acme.tools',
      resolvedCommit: 'b'.repeat(40),
      scope: 'project' as const,
      repoId: 'repo-1'
    }
    invokeMock.mockResolvedValueOnce({ ok: false, error: 'source_unavailable' })
    await expect(createPluginsRealApi().installMarketplacePlugin(installArgs)).resolves.toEqual({
      ok: false,
      error: 'source_unavailable'
    })
    expect(invokeMock).toHaveBeenCalledWith('plugins_install_marketplace_plugin', {
      args: installArgs
    })
  })
})

describe('plugins real adapter project lockfile', () => {
  it('routes getProjectLockState and installMissingFromLockfile with the repoId', async () => {
    const state = { repoId: 'repo-1', lockfilePath: '/repo/plugins.lock', entries: [] }
    invokeMock.mockResolvedValueOnce(state)
    await expect(
      createPluginsRealApi().getProjectLockState({ repoId: 'repo-1' })
    ).resolves.toEqual(state)
    expect(invokeMock).toHaveBeenCalledWith('plugins_get_project_lock_state', {
      args: { repoId: 'repo-1' }
    })

    const results = [{ ok: true, pluginKey: 'acme.tools' }]
    invokeMock.mockResolvedValueOnce(results)
    await expect(
      createPluginsRealApi().installMissingFromLockfile({ repoId: 'repo-1' })
    ).resolves.toEqual(results)
    expect(invokeMock).toHaveBeenCalledWith('plugins_install_missing_from_lockfile', {
      args: { repoId: 'repo-1' }
    })
  })
})

describe('plugins real adapter events', () => {
  it('forwards plugins:changed payloads and returns an idempotent unsubscriber', async () => {
    const unlisten = vi.fn()
    const handlers: Array<(event: { payload: unknown }) => void> = []
    listenMock.mockImplementationOnce(async (_event, callback) => {
      handlers.push(callback as (event: { payload: unknown }) => void)
      return unlisten
    })
    const callback = vi.fn()
    const unsubscribe = createPluginsRealApi().onChanged(callback)

    expect(listenMock).toHaveBeenCalledWith('plugins:changed', expect.any(Function))
    const event: PluginChangeEvent = { contentPacksChanged: true }
    handlers[0]?.({ payload: event })
    expect(callback).toHaveBeenCalledWith(event)

    unsubscribe()
    unsubscribe()
    await Promise.resolve()
    expect(unlisten).toHaveBeenCalledTimes(1)
  })

  it('forwards plugins:open-tab requests and returns an unsubscriber', async () => {
    const unlisten = vi.fn()
    const handlers: Array<(event: { payload: unknown }) => void> = []
    listenMock.mockImplementationOnce(async (_event, callback) => {
      handlers.push(callback as (event: { payload: unknown }) => void)
      return unlisten
    })
    const callback = vi.fn()
    const unsubscribe = createPluginsRealApi().onOpenTab(callback)

    expect(listenMock).toHaveBeenCalledWith('plugins:open-tab', expect.any(Function))
    const request: PluginOpenTabRequest = {
      pluginKey: 'acme.tools',
      tabId: 'dashboard',
      instanceId: 'inst-1',
      title: 'Dashboard',
      params: { view: 'overview' },
      worktreeId: 'repo-1::/repo',
      scope: 'project',
      repoId: 'repo-1'
    }
    handlers[0]?.({ payload: request })
    expect(callback).toHaveBeenCalledWith(request)

    unsubscribe()
    await Promise.resolve()
    expect(unlisten).toHaveBeenCalledTimes(1)
  })

  it('subscribes onPanelPush to plugins:panel-push', async () => {
    const unlisten = vi.fn()
    const handlers: Array<(event: { payload: unknown }) => void> = []
    listenMock.mockImplementationOnce(async (_event, callback) => {
      handlers.push(callback as (event: { payload: unknown }) => void)
      return unlisten
    })
    const callback = vi.fn()
    const unsubscribe = createPluginsRealApi().onPanelPush(callback)

    expect(listenMock).toHaveBeenCalledWith('plugins:panel-push', expect.any(Function))
    handlers[0]?.({ payload: { sessionToken: 't', payload: 1 } })
    expect(callback).toHaveBeenCalledWith({ sessionToken: 't', payload: 1 })

    unsubscribe()
    await Promise.resolve()
    expect(unlisten).toHaveBeenCalledTimes(1)
  })
})
