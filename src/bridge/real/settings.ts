import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import type { SettingsApi } from '../../shared/preload-api/api/settings-api'
import type { GlobalSettings } from '../../shared/global-settings-types'
import { withMethodFallback } from '../unimplemented-fallback'
import { getBootstrap } from './bootstrap'

/** Subscribe to a Tauri event and return an idempotent unsubscriber. */
export function subscribeToEvent<T>(
  event: string,
  callback: (payload: T) => void
): () => void {
  let disposed = false
  let unlisten: (() => void) | null = null
  void listen<T>(event, (message) => callback(message.payload))
    .then((stop) => {
      if (disposed) {
        stop()
        return
      }
      unlisten = stop
    })
    .catch(() => {})
  return () => {
    disposed = true
    unlisten?.()
    unlisten = null
  }
}

export function createSettingsRealApi(): SettingsApi {
  return withMethodFallback<SettingsApi>('settings', {
    get: () => invoke<GlobalSettings>('settings_get'),
    getSync: () => getBootstrap()?.settings ?? null,
    set: (args) => invoke<GlobalSettings>('settings_set', { args }),
    onChanged: (callback) =>
      subscribeToEvent<Partial<GlobalSettings>>('settings:changed', callback)
  })
}
