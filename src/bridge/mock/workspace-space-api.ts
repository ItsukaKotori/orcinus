// Phase 0 mock; replaced by Tauri IPC per view.
import type { PreloadApi } from '../../preload/api-types'
import { withMethodFallback } from '../unimplemented-fallback'
import { noopUnsubscribe } from './noop-unsubscribe'

export function createWorkspaceSpaceApi(): PreloadApi['workspaceSpace'] {
  return withMethodFallback<PreloadApi['workspaceSpace']>('workspaceSpace', {
    onProgress: () => noopUnsubscribe
  })
}
