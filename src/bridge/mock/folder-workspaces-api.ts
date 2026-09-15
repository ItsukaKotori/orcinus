// Phase 0 mock; replaced by Tauri IPC per view.
import type { PreloadApi } from '../../preload/api-types'
import { withMethodFallback } from '../unimplemented-fallback'

export function createFolderWorkspacesApi(): PreloadApi['folderWorkspaces'] {
  // Why empty: no folder workspaces exist in Phase 0; consumers render their empty state.
  return withMethodFallback<PreloadApi['folderWorkspaces']>('folderWorkspaces', {
    list: async () => []
  })
}
