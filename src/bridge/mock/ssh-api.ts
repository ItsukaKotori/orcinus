// Phase 0 mock; replaced by Tauri IPC per view.
import type { PreloadApi } from '../../preload/api-types'
import { withMethodFallback } from '../unimplemented-fallback'
import { noopUnsubscribe } from './noop-unsubscribe'

export function createSshApi(): PreloadApi['ssh'] {
  return withMethodFallback<PreloadApi['ssh']>('ssh', {
    listTargets: async () => [],
    listRemovedTargetLabels: async () => ({}),
    onStateChanged: () => noopUnsubscribe,
    onPortForwardsChanged: () => noopUnsubscribe,
    onDetectedPortsChanged: () => noopUnsubscribe,
    onCredentialRequest: () => noopUnsubscribe,
    onCredentialResolved: () => noopUnsubscribe
  })
}
