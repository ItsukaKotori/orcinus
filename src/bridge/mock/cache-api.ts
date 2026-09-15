// Phase 0 mock; replaced by Tauri IPC per view.
import type { PreloadApi } from '../../preload/api-types'
import { withMethodFallback } from '../unimplemented-fallback'

export function createCacheApi(): PreloadApi['cache'] {
  // Why empty records: the renderer hydrates its GitHub cache from this payload and falls back to
  // empty maps when there is nothing on disk, so an empty shape is the no-cache state.
  return withMethodFallback<PreloadApi['cache']>('cache', {
    getGitHub: async () => ({ pr: {}, issue: {} })
  })
}
