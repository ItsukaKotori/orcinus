import type { PreloadApi } from '../../shared/preload-api/api-types'
import { withMethodFallback } from '../unimplemented-fallback'
import { invokeCommand, subscribeToEvent } from './invoke'

/**
 * Real `ui` adapter (spec §5.4). State reads/writes and the interaction
 * counter map onto `ui_*` commands; tray/menu/shortcut event subscriptions
 * stay unimplemented until the surfaces that emit them exist (spec §2.2).
 */
export function createUiRealApi(): PreloadApi['ui'] {
  return withMethodFallback<PreloadApi['ui']>('ui', {
    get: () => invokeCommand('ui_get'),
    set: (args) => invokeCommand('ui_set', { args }),
    setWithAck: (args) => invokeCommand('ui_set', { args }),
    recordFeatureInteraction: (id) =>
      invokeCommand('ui_record_feature_interaction', { args: { id } }),
    onStateChanged: (callback) => subscribeToEvent('ui:stateChanged', callback)
  })
}
