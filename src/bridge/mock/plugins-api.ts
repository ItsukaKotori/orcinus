import type { PluginsApi } from '../../preload/api/plugin-host-api'
import { UnimplementedBridgeError } from '../unimplemented-fallback'
import { MOCK_GLOBAL_PLUGINS, MOCK_PROJECT_PLUGINS, type PluginCenterEntry } from './fixtures'

function unimplemented(path: string): never {
  throw new UnimplementedBridgeError(path)
}

export function createPluginsApi(): PluginsApi {
  let state: PluginCenterEntry[] = [...MOCK_GLOBAL_PLUGINS, ...MOCK_PROJECT_PLUGINS]
  return {
    list: async () => state,
    listLanguagePacks: async () => [],
    consent: async () => state,
    setEnabled: async ({ pluginKey, enabled }) => {
      state = state.map((entry) =>
        entry.pluginKey === pluginKey
          ? { ...entry, status: enabled ? 'idle' : 'disabled' }
          : entry
      )
      return state
    },
    readPanelEntry: async () => null,
    invokeCommand: async () => undefined,
    panelAction: async () => ({ ok: true, value: null }),
    install: async () => ({
      ok: true,
      pluginKey: 'mock-installed',
      version: '0.0.0-mock',
      contentHash: 'mock-content-hash',
      consentFingerprint: 'mock-consent-fingerprint',
      resolvedCommit: null
    }),
    listMarketplaces: async () => [],
    addMarketplace: async () => unimplemented('plugins.addMarketplace'),
    removeMarketplace: async () => [],
    refreshMarketplaces: async () => [],
    listMarketplacePlugins: async () => [],
    previewMarketplacePlugin: async () =>
      unimplemented('plugins.previewMarketplacePlugin'),
    installMarketplacePlugin: async () => ({
      ok: true,
      pluginKey: 'mock-marketplace',
      version: '0.0.0-mock',
      contentHash: 'mock-content-hash',
      consentFingerprint: 'mock-consent-fingerprint',
      resolvedCommit: null
    }),
    previewMarketplaceUpdate: async () =>
      unimplemented('plugins.previewMarketplaceUpdate'),
    rollbackMarketplacePlugin: async () => ({
      ok: true,
      pluginKey: 'mock-rollback',
      version: '0.0.0-mock',
      contentHash: 'mock-content-hash',
      consentFingerprint: 'mock-consent-fingerprint',
      resolvedCommit: null
    }),
    remove: async ({ pluginKey }) => {
      state = state.filter((entry) => entry.pluginKey !== pluginKey)
      return state
    },
    getLogs: async () => [{ ts: Date.now(), level: 'info', line: 'mock log line' }],
    refresh: async () => state,
    onChanged: () => () => {}
  }
}
