// Phase 0 mock; replaced by Tauri IPC per view.
import type { PreloadApi } from '../../preload/api-types'
import { withMethodFallback } from '../unimplemented-fallback'
import { noopUnsubscribe } from './noop-unsubscribe'

export function createUpdaterApi(): PreloadApi['updater'] {
  return withMethodFallback<PreloadApi['updater']>('updater', {
    // Why hard-coded: mirrors package.json version; Phase 0 has no updater backend to report it.
    getVersion: async () => '0.0.1',
    getStatus: async () => ({ state: 'idle' }),
    onStatus: () => noopUnsubscribe,
    onClearDismissal: () => noopUnsubscribe
  })
}
