import type {
  PluginPanelActionOutcome,
  PluginPanelEntry
} from '../../plugins/plugin-panel-bridge'
import type { PluginConsentRequest } from '../../plugins/plugin-consent-request'
import type { PluginLanguagePackRegistration } from '../../plugins/plugin-language-pack-artifact'
import type { PluginChangeEvent } from '../../plugins/plugin-change-event'
import type { PluginInstallSource } from '../../plugins/plugin-install-lockfile'
import type { PluginManifest } from '../../plugins/plugin-manifest'
import type { PluginMarketplaceGitSource } from '../../plugins/plugin-marketplace'

/** Install/deployment scope, mirroring the Rust `PluginScope`. */
export type PluginScope = 'global' | 'project'

/** Locates one plugin instance across the dual-scope model (same key may be
 *  installed globally and in several projects). */
export type PluginRef = {
  pluginKey: string
  scope: PluginScope
  repoId?: string
}

/** Panel contribution as surfaced by the main-process plugin service. */
export type PluginHostPanel = {
  id: string
  title: string
  /** Lucide icon name declared in the plugin manifest. */
  icon?: string
  tabKey: `plugin:${string}`
}

/** Main-area tab contribution as surfaced by the main-process plugin service. */
export type PluginHostTab = {
  id: string
  title: string
  /** Lucide icon name declared in the plugin manifest. */
  icon?: string
}

/** `pending` = awaiting (re-)consent; `idle` = enabled, worker not running
 *  (lazy); `restarting` = waiting for supervised backoff; `errored` = crashed past the restart budget or failed to start;
 *  `invalid` = unreadable manifest. */
export type PluginHostStatus =
  | 'running'
  | 'restarting'
  | 'idle'
  | 'pending'
  | 'disabled'
  | 'errored'
  | 'invalid'

/** Wire shape of plugins:list — must stay assignable from the main-process
 *  projection in src/main/plugins/plugin-list-projection.ts. */
export type PluginHostListEntry = {
  pluginKey: string
  scope: PluginScope
  repoId?: string
  consentFingerprint: string | null
  name: string
  version: string
  publisher: string
  description?: string
  status: PluginHostStatus
  needsReconsent: boolean
  error?: string
  isDev: boolean
  official: boolean
  bundled: boolean
  capabilities: { kind: string; description: string }[]
  panels: PluginHostPanel[]
  tabs: PluginHostTab[]
  commands: {
    id: string
    title: string
    context: 'global' | 'worktree'
    handler: { type: 'built-in'; action: string } | { type: 'worker' }
    keybindings: { key: string; when: 'global' | 'worktree' }[]
  }[]
  hasWorker: boolean
  vmRecipes?: {
    id: string
    name: string
    description?: string
    commands: {
      phase: 'create' | 'suspend' | 'resume' | 'destroy'
      command: string
    }[]
  }[]
  restarts: number
  blockedByKillList?: { reason: string; advisoryUrl?: string }
  source?: {
    kind: 'local-path' | 'git' | 'marketplace' | 'bundled' | 'dev' | 'installed'
    reference?: string
    resolvedCommit?: string | null
    contentHash?: string
    marketplace?: { reference: string; resolvedCommit: string }
  }
  installHash?: string
}

export type PluginHostLogLine = {
  ts: number
  level: 'info' | 'warn' | 'error'
  line: string
}

export type PluginHostInstallSource =
  | { kind: 'local-path'; path: string; scope: PluginScope; repoId?: string }
  | { kind: 'git'; url: string; ref: string; scope: PluginScope; repoId?: string }

export type PluginHostInstallResult =
  | {
      ok: true
      pluginKey: string
      version: string
      contentHash: string
      consentFingerprint: string
      resolvedCommit: string | null
    }
  | { ok: false; error: string }

export type PluginMarketplaceHostSourceState = {
  id: string
  source: PluginMarketplaceGitSource
  addedAt: number
  marketplace: {
    name: string
    owner: string
    resolvedCommit: string
    fetchedAt: number
  } | null
  stale: boolean
  official: boolean
  error?: string
}

export type PluginMarketplaceHostListing = {
  marketplaceSourceId: string
  marketplaceName: string
  marketplaceOwner: string
  marketplaceCommit: string
  pluginKey: string
  source: PluginMarketplaceGitSource
  description?: string
  categories: string[]
  official: boolean
  bundled: boolean
  blockedByKillList?: { reason: string; advisoryUrl?: string }
}

export type PluginMarketplaceHostInstallPreview = {
  marketplaceSourceId: string
  marketplaceName: string
  marketplaceOwner: string
  marketplaceCommit: string
  pluginKey: string
  source: PluginMarketplaceGitSource
  resolvedCommit: string
  contentHash: string
  consentFingerprint: string
  manifest: PluginManifest
  official: boolean
  bundled: boolean
  blockedByKillList?: { reason: string; advisoryUrl?: string }
}

/** Classified state of one project lockfile entry (wire parity with the Rust
 *  `LockEntryStatus`). */
export type PluginProjectLockStatus = 'installed' | 'missing' | 'hash-mismatch' | 'source-unavailable'

export type PluginProjectLockEntry = {
  pluginKey: string
  version: string
  source: PluginInstallSource
  contentHash: string
  status: PluginProjectLockStatus
  detail?: string
}

export type PluginProjectLockState = {
  repoId: string
  lockfilePath: string
  entries: PluginProjectLockEntry[]
}

/** Payload of the `plugins:open-tab` bridge event: the renderer opens (or
 *  focuses) a plugin tab in the main workspace area. */
export type PluginOpenTabRequest = {
  pluginKey: string
  tabId: string
  instanceId: string
  title?: string
  params?: unknown
  worktreeId: string
  scope: PluginScope
  repoId?: string
}

export type PluginsApi = {
  list: () => Promise<PluginHostListEntry[]>
  listLanguagePacks: () => Promise<PluginLanguagePackRegistration[]>
  /** Records the consent-dialog answer; approval is keyed to the plugin's
   *  current capability and trusted-worker fingerprint. */
  consent: (args: PluginConsentRequest) => Promise<PluginHostListEntry[]>
  setEnabled: (args: PluginRef & { enabled: boolean }) => Promise<PluginHostListEntry[]>
  /** Returns the panel's CSP-wrapped HTML, or null when the plugin or
   *  panel is missing/disabled. Rendered only inside a sandboxed iframe. */
  readPanelEntry: (args: PluginRef & { panelId: string }) => Promise<PluginPanelEntry | null>
  invokeCommand: (args: PluginRef & { commandId: string; args?: unknown }) => Promise<unknown>
  /** Relays a sandboxed panel's bridge request to main, which enforces the
   *  plugin's consented capabilities before executing. */
  panelAction: (args: {
    sessionToken: string
    action: string
    params?: unknown
  }) => Promise<PluginPanelActionOutcome>
  install: (source: PluginHostInstallSource) => Promise<PluginHostInstallResult>
  listMarketplaces: () => Promise<PluginMarketplaceHostSourceState[]>
  addMarketplace: (source: PluginMarketplaceGitSource) => Promise<PluginMarketplaceHostSourceState>
  removeMarketplace: (args: { sourceId: string }) => Promise<PluginMarketplaceHostSourceState[]>
  refreshMarketplaces: (args?: { sourceId?: string }) => Promise<PluginMarketplaceHostSourceState[]>
  listMarketplacePlugins: () => Promise<PluginMarketplaceHostListing[]>
  previewMarketplacePlugin: (args: {
    marketplaceSourceId: string
    pluginKey: string
  }) => Promise<PluginMarketplaceHostInstallPreview>
  installMarketplacePlugin: (
    preview: Pick<
      PluginMarketplaceHostInstallPreview,
      'marketplaceSourceId' | 'marketplaceCommit' | 'pluginKey' | 'resolvedCommit'
    >
  ) => Promise<PluginHostInstallResult>
  previewMarketplaceUpdate: (args: {
    pluginKey: string
  }) => Promise<PluginMarketplaceHostInstallPreview>
  rollbackMarketplacePlugin: (args: { pluginKey: string }) => Promise<PluginHostInstallResult>
  remove: (args: PluginRef) => Promise<PluginHostListEntry[]>
  getLogs: (args: { pluginKey: string }) => Promise<PluginHostLogLine[]>
  /** Classifies the project lockfile for the repo (empty state when unknown). */
  getProjectLockState: (args: { repoId: string }) => Promise<PluginProjectLockState>
  /** Reinstalls every `missing`/`hash-mismatch` lockfile entry from its
   *  recorded source; `source-unavailable` entries come back as `ok: false`. */
  installMissingFromLockfile: (args: { repoId: string }) => Promise<PluginHostInstallResult[]>
  /** Subscribes to `workspace.openTab` requests from plugin workers/panels. */
  onOpenTab: (callback: (request: PluginOpenTabRequest) => void) => () => void
  /** Re-discovers after settings edits (feature flag, dev paths). */
  refresh: () => Promise<PluginHostListEntry[]>
  /** Fires whenever installed plugins, worker states, panels, or content packs change. */
  onChanged: (callback: (event: PluginChangeEvent) => void) => () => void
}
