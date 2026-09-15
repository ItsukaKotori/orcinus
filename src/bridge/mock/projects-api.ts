// Phase 0 mock; replaced by Tauri IPC per view.
import type { PreloadApi } from '../../preload/api-types'
import { withMethodFallback } from '../unimplemented-fallback'

export function createProjectsApi(): PreloadApi['projects'] {
  // Why empty: no projects are registered in Phase 0, which is exactly the state the landing
  // surface renders as its add-a-project empty state.
  return withMethodFallback<PreloadApi['projects']>('projects', {
    list: async () => [],
    listHostSetups: async () => []
  })
}
