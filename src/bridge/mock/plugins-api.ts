import type { PluginHostListEntry, PluginsApi } from '../../shared/preload-api/api/plugin-host-api'
import { UnimplementedBridgeError } from '../unimplemented-fallback'
import { MOCK_GLOBAL_PLUGINS, MOCK_PROJECT_PLUGINS } from './fixtures'
import { cloneMockValue } from './clone-mock-value'

function unimplemented(path: string): never {
  throw new UnimplementedBridgeError(path)
}

export function createPluginsApi(): PluginsApi {
  let state: PluginHostListEntry[] = [...MOCK_GLOBAL_PLUGINS, ...MOCK_PROJECT_PLUGINS]
  return {
    list: async () => cloneMockValue(state),
    listLanguagePacks: async () => [],
    consent: async () => cloneMockValue(state),
    setEnabled: async ({ pluginKey, enabled }) => {
      state = state.map((entry) =>
        entry.pluginKey === pluginKey
          ? { ...entry, status: enabled ? 'idle' : 'disabled' }
          : entry
      )
      return cloneMockValue(state)
    },
    readPanelEntry: async () => null,
    readTabEntry: async () => null,
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
    installMarketplacePlugin: async (preview) => {
      if (preview.scope !== 'global' && preview.scope !== 'project') {
        throw new Error('plugins.installMarketplacePlugin requires a scope')
      }
      if (preview.scope === 'project' && !preview.repoId) {
        throw new Error('plugins.installMarketplacePlugin requires a repoId for project scope')
      }
      return {
        ok: true,
        pluginKey: preview.pluginKey,
        version: '0.0.0-mock',
        contentHash: 'mock-content-hash',
        consentFingerprint: 'mock-consent-fingerprint',
        resolvedCommit: preview.resolvedCommit
      }
    },
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
      return cloneMockValue(state)
    },
    getLogs: async () => [{ ts: Date.now(), level: 'info', line: 'mock log line' }],
    getProjectLockState: async ({ repoId }) => ({ repoId, lockfilePath: '', entries: [] }),
    installMissingFromLockfile: async () => [],
    refresh: async () => cloneMockValue(state),
    onOpenTab: () => () => {},
    onPanelPush: () => () => {},
    onChanged: () => () => {}
  }
}
