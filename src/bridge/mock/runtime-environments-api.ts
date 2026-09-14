// Phase 0 mock; replaced by Tauri IPC per view.
import type { PreloadApi } from '../../preload/api-types'
import { withUnimplementedFallback } from '../unimplemented-fallback'

export function createRuntimeEnvironmentsApi(): PreloadApi['runtimeEnvironments'] {
  return withUnimplementedFallback<PreloadApi['runtimeEnvironments']>({
    list: async () => [],
    getStatusSnapshots: async () => [],
    onStatusChanged: () => () => {}
  })
}
