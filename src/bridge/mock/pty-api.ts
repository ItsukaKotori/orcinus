// Phase 0 mock; replaced by Tauri IPC per view.
import type { PreloadApi } from '../../preload/api-types'
import { withMethodFallback } from '../unimplemented-fallback'
import { noopUnsubscribe } from './noop-unsubscribe'

export function createPtyApi(): PreloadApi['pty'] {
  return withMethodFallback<PreloadApi['pty']>('pty', {
    listSessions: async () => [],
    onSpawned: () => noopUnsubscribe,
    onExit: () => noopUnsubscribe,
    // The terminal pane mounts these listeners and stores the return value in an
    // unsubscribe list; on this host no PTY ever streams, so they stay no-ops.
    onData: () => noopUnsubscribe,
    onReplay: () => noopUnsubscribe,
    onWriteUnavailable: () => noopUnsubscribe,
    onClearBufferRequest: () => noopUnsubscribe,
    onSerializeBufferRequest: () => noopUnsubscribe,
    resize: () => {},
    sendSerializedBuffer: () => {},
    publishTerminalViewAttributes: () => {}
  })
}
