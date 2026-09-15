// Phase 0 mock; replaced by Tauri IPC per view.
import type { PreloadApi } from '../../preload/api-types'
import { withMethodFallback } from '../unimplemented-fallback'

export function createProjectGroupsApi(): PreloadApi['projectGroups'] {
  // Why empty: no project groups exist in Phase 0; group consumers render their empty state.
  return withMethodFallback<PreloadApi['projectGroups']>('projectGroups', {
    list: async () => []
  })
}
