// Phase 0 mock; replaced by Tauri IPC per view.
import type { PreloadApi } from '../../preload/api-types'
import { withMethodFallback } from '../unimplemented-fallback'
import { noopUnsubscribe } from './noop-unsubscribe'

export function createWorkspacePortsApi(): PreloadApi['workspacePorts'] {
  return withMethodFallback<PreloadApi['workspacePorts']>('workspacePorts', {
    // Why: the ports panel scans on mount; Phase 0 has no listener table to read,
    // so it answers an explicit unavailable result instead of a rejection.
    scan: async () => ({
      platform: 'unknown',
      scannedAt: Date.now(),
      ports: [],
      unavailableReason: 'not-implemented'
    }),
    onAdvertisedUrlChanged: () => noopUnsubscribe
  })
}
