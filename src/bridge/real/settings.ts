import type { SettingsApi } from '../../shared/preload-api/api/settings-api'
import type { GlobalSettings } from '../../shared/global-settings-types'
import { withMethodFallback } from '../unimplemented-fallback'
import { getBootstrap, updateBootstrapSettings } from './bootstrap'
import { invokeCommand, subscribeToEvent } from './invoke'

export function createSettingsRealApi(): SettingsApi {
  return withMethodFallback<SettingsApi>('settings', {
    get: () => invokeCommand('settings_get'),
    getSync: () => getBootstrap()?.settings ?? null,
    set: async (args) => {
      const settings = await invokeCommand<GlobalSettings>('settings_set', { args })
      updateBootstrapSettings(settings)
      return settings
    },
    onChanged: (callback) =>
      subscribeToEvent<Partial<GlobalSettings>>('settings:changed', callback)
  })
}
