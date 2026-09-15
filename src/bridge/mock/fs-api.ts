// Phase 0 mock; replaced by Tauri IPC per view.
import type { PreloadApi } from '../../preload/api-types'
import { withMethodFallback } from '../unimplemented-fallback'
import { noopUnsubscribe } from './noop-unsubscribe'

export function createFsApi(): PreloadApi['fs'] {
  return withMethodFallback<PreloadApi['fs']>('fs', {
    onFsChanged: () => noopUnsubscribe
  })
}
