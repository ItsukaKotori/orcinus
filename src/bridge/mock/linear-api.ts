// Phase 0 mock; replaced by Tauri IPC per view.
import type { PreloadApi } from '../../preload/api-types'
import { withMethodFallback } from '../unimplemented-fallback'

export function createLinearApi(): PreloadApi['linear'] {
  // Why disconnected: the status surfaces render their "connect Linear" affordance for an
  // unconnected account, which is the only truthful Phase 0 state.
  return withMethodFallback<PreloadApi['linear']>('linear', {
    status: async () => ({ connected: false, viewer: null })
  })
}
