// Phase 0 mock; replaced by Tauri IPC per view.
import type { PreloadApi } from '../../preload/api-types'
import type { RuntimeStatus } from '../../shared/runtime-types'
import { withMethodFallback } from '../unimplemented-fallback'
import { noopUnsubscribe } from './noop-unsubscribe'

// Why a local runtime id: Phase 0 has no Rust runtime host; the renderer graph reads this as its
// own in-process identity, with a ready graph and no live panes.
const LOCAL_RUNTIME_STATUS: RuntimeStatus = {
  runtimeId: 'phase0-local',
  rendererGraphEpoch: 0,
  graphStatus: 'ready',
  authoritativeWindowId: null,
  liveTabCount: 0,
  liveLeafCount: 0
}

export function createRuntimeApi(): PreloadApi['runtime'] {
  return withMethodFallback<PreloadApi['runtime']>('runtime', {
    getStatus: async () => LOCAL_RUNTIME_STATUS,
    syncWindowGraph: async () => LOCAL_RUNTIME_STATUS,
    getTerminalFitOverrides: async () => [],
    getTerminalDrivers: async () => [],
    getBrowserDrivers: async () => [],
    getBrowserRemoteViewerPages: async () => [],
    getClientHostedBrowserRows: async () => [],
    onTerminalFitOverrideChanged: () => noopUnsubscribe,
    onTerminalDriverChanged: () => noopUnsubscribe,
    onBrowserDriverChanged: () => noopUnsubscribe,
    onBrowserRemoteViewersChanged: () => noopUnsubscribe,
    onClientHostedBrowserRowsChanged: () => noopUnsubscribe
  })
}
