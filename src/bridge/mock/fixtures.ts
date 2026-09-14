import type { PluginHostListEntry } from '../../preload/api/plugin-host-api'

export type PluginScope = 'global' | 'project'
export type PluginCenterEntry = PluginHostListEntry & { scope: PluginScope }

export const MOCK_GLOBAL_PLUGINS: PluginCenterEntry[] = [
  {
    pluginKey: 'database-manager',
    name: 'Database Manager',
    version: '0.1.0',
    publisher: 'ade-labs',
    description: 'Browse and query local databases',
    scope: 'global',
    consentFingerprint: 'mock-fingerprint-db',
    needsReconsent: false,
    status: 'idle',
    isDev: false,
    official: false,
    bundled: false,
    capabilities: [{ kind: 'workspace:read', description: 'Read workspace metadata' }],
    panels: [
      {
        id: 'database-manager.main',
        title: 'Database',
        icon: 'database',
        tabKey: 'plugin:database-manager.main'
      }
    ],
    commands: [],
    hasWorker: true,
    restarts: 0
  },
  {
    pluginKey: 'api-tester',
    name: 'API Tester',
    version: '0.2.0',
    publisher: 'ade-labs',
    description: 'Send HTTP requests from a panel',
    scope: 'global',
    consentFingerprint: 'mock-fingerprint-api',
    needsReconsent: false,
    status: 'idle',
    isDev: false,
    official: false,
    bundled: false,
    capabilities: [{ kind: 'storage', description: 'Store requests in the plugin folder' }],
    panels: [
      {
        id: 'api-tester.main',
        title: 'API Tester',
        icon: 'plug',
        tabKey: 'plugin:api-tester.main'
      }
    ],
    commands: [],
    hasWorker: true,
    restarts: 0
  }
]

export const MOCK_PROJECT_PLUGINS: PluginCenterEntry[] = [
  {
    ...MOCK_GLOBAL_PLUGINS[0],
    pluginKey: 'repo-notes',
    name: 'Repo Notes',
    version: '0.1.0',
    publisher: 'my-team',
    description: 'Project-scoped notes panel',
    scope: 'project',
    consentFingerprint: 'mock-fingerprint-notes',
    panels: [
      {
        id: 'repo-notes.main',
        title: 'Repo Notes',
        icon: 'filetext',
        tabKey: 'plugin:repo-notes.main'
      }
    ]
  }
]
