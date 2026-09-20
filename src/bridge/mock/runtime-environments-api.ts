// Phase 0 mock; replaced by Tauri IPC per view.
import type { PreloadApi } from '../../preload/api-types'
import { withMethodFallback } from '../unimplemented-fallback'

export function createRuntimeEnvironmentsApi(): PreloadApi['runtimeEnvironments'] {
  return withMethodFallback<PreloadApi['runtimeEnvironments']>('runtimeEnvironments', {
    list: async () => [],
    getStatusSnapshots: async () => [],
    onStatusChanged: () => () => {}
  })
}
