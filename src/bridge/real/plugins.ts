import type { PluginOpenTabRequest } from '../../shared/preload-api/api/plugin-host-api'
import type { PreloadApi } from '../../shared/preload-api/api-types'
import type { PluginChangeEvent } from '../../shared/plugins/plugin-change-event'
import type {
  PluginPanelActionErrorCode,
  PluginPanelActionOutcome
} from '../../shared/plugins/plugin-panel-bridge'
import { withMethodFallback } from '../unimplemented-fallback'
import { invokeCommand, subscribeToEvent } from './invoke'

/** Flat wire shape of `plugins_panel_action` (Rust `PluginPanelActionOutcomeDto`). */
type PluginPanelActionOutcomeDto = {
  ok: boolean
  value?: unknown
  code?: string
  error?: string
}

/** Runtime mirror of the frozen `PluginPanelActionErrorCode` union. */
const PANEL_ACTION_ERROR_CODES = [
  'invalid_request',
  'unknown_method',
  'capability_denied',
  'consent_required',
  'panel_forbidden',
  'invalid_params',
  'rate_limited',
  'unavailable',
  'action_failed'
] as const satisfies readonly PluginPanelActionErrorCode[]

function isPanelActionErrorCode(value: string | undefined): value is PluginPanelActionErrorCode {
  return value !== undefined && (PANEL_ACTION_ERROR_CODES as readonly string[]).includes(value)
}

/** Bridge-only code for the feature-gate refusal; the frozen renderer union has
 *  no `disabled_system`, so it surfaces as the generic `unavailable`. */
const DISABLED_SYSTEM_CODE = 'disabled_system'

/**
 * Narrows the Rust outcome DTO into the frozen TS union: `disabled_system`
 * maps to `unavailable`, unknown codes fold into `action_failed` rather than
 * leaking an out-of-union string into renderer switch statements.
 */
function toPanelActionOutcome(outcome: PluginPanelActionOutcomeDto): PluginPanelActionOutcome {
  if (outcome.ok) {
    return { ok: true, value: outcome.value ?? null }
  }
  const error = outcome.error ?? 'plugin panel action failed'
  if (outcome.code === DISABLED_SYSTEM_CODE) {
    return { ok: false, code: 'unavailable', error }
  }
  return {
    ok: false,
    code: isPanelActionErrorCode(outcome.code) ? outcome.code : 'action_failed',
    error
  }
}

/**
 * Real `plugins` adapter (spec §3.9): all 25 contract methods route to the 23
 * `plugins_*` commands plus the `plugins:changed` / `plugins:open-tab` Tauri
 * events. `plugins_install` and `plugins_add_marketplace` take `{ source }`;
 * every other invokable takes the shared `{ args }` envelope.
 */
export function createPluginsRealApi(): PreloadApi['plugins'] {
  return withMethodFallback<PreloadApi['plugins']>('plugins', {
    list: () => invokeCommand('plugins_list'),
    listLanguagePacks: () => invokeCommand('plugins_list_language_packs'),
    consent: (args) => invokeCommand('plugins_consent', { args }),
    setEnabled: (args) => invokeCommand('plugins_set_enabled', { args }),
    readPanelEntry: (args) => invokeCommand('plugins_read_panel_entry', { args }),
    readTabEntry: (args) => invokeCommand('plugins_read_tab_entry', { args }),
    invokeCommand: (args) => invokeCommand('plugins_invoke_command', { args }),
    panelAction: (args) =>
      invokeCommand<PluginPanelActionOutcomeDto>('plugins_panel_action', { args }).then(
        toPanelActionOutcome
      ),
    install: (source) => invokeCommand('plugins_install', { source }),
    listMarketplaces: () => invokeCommand('plugins_list_marketplaces'),
    addMarketplace: (source) => invokeCommand('plugins_add_marketplace', { source }),
    removeMarketplace: (args) => invokeCommand('plugins_remove_marketplace', { args }),
    // The Rust command declares a required (all-optional-fields) struct, so the
    // omitted-args call still ships `{}` instead of a missing `args` key.
    refreshMarketplaces: (args) =>
      invokeCommand('plugins_refresh_marketplaces', { args: args ?? {} }),
    listMarketplacePlugins: () => invokeCommand('plugins_list_marketplace_plugins'),
    previewMarketplacePlugin: (args) =>
      invokeCommand('plugins_preview_marketplace_plugin', { args }),
    installMarketplacePlugin: (args) =>
      invokeCommand('plugins_install_marketplace_plugin', { args }),
    previewMarketplaceUpdate: (args) =>
      invokeCommand('plugins_preview_marketplace_update', { args }),
    rollbackMarketplacePlugin: (args) =>
      invokeCommand('plugins_rollback_marketplace_plugin', { args }),
    remove: (args) => invokeCommand('plugins_remove', { args }),
    getLogs: (args) => invokeCommand('plugins_get_logs', { args }),
    getProjectLockState: (args) => invokeCommand('plugins_get_project_lock_state', { args }),
    installMissingFromLockfile: (args) =>
      invokeCommand('plugins_install_missing_from_lockfile', { args }),
    refresh: () => invokeCommand('plugins_refresh'),
    onOpenTab: (callback) => subscribeToEvent<PluginOpenTabRequest>('plugins:open-tab', callback),
    onChanged: (callback) => subscribeToEvent<PluginChangeEvent>('plugins:changed', callback)
  })
}
