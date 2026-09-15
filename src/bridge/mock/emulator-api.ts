// Phase 0 mock; replaced by Tauri IPC per view.
import type { PreloadApi } from '../../preload/api-types'
import { withMethodFallback } from '../unimplemented-fallback'
import { noopUnsubscribe } from './noop-unsubscribe'

export function createEmulatorApi(): PreloadApi['emulator'] {
  return withMethodFallback<PreloadApi['emulator']>('emulator', {
    onAutoAttach: () => noopUnsubscribe,
    onPaneFocus: () => noopUnsubscribe
  })
}
