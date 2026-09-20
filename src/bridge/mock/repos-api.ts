// Phase 0 mock; replaced by Tauri IPC per view.
import type { PreloadApi } from '../../preload/api-types'
import { withMethodFallback } from '../unimplemented-fallback'

export function createReposApi(): PreloadApi['repos'] {
  return withMethodFallback<PreloadApi['repos']>('repos', {
    list: async () => [],
    onChanged: () => () => {}
  })
}
