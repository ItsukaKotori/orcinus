// Phase 0 mock; replaced by Tauri IPC per view.
import type { PreloadApi } from '../../preload/api-types'
import { withUnimplementedFallback } from '../unimplemented-fallback'

export function createReposApi(): PreloadApi['repos'] {
  return withUnimplementedFallback<PreloadApi['repos']>({
    list: async () => [],
    onChanged: () => () => {}
  })
}
