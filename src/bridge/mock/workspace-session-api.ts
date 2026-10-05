// Phase 0 mock; replaced by Tauri IPC per view.
import type { PreloadApi } from '../../preload/api-types'
import { getDefaultWorkspaceSession } from '../../shared/constants'
import { withMethodFallback } from '../unimplemented-fallback'

export function createSessionApi(): PreloadApi['session'] {
  return withMethodFallback<PreloadApi['session']>('session', {
    get: async () => getDefaultWorkspaceSession(),
    set: async () => {},
    patch: async () => {},
    flush: async () => {},
    readTerminalScrollback: () => null,
    setSync: () => {}
  })
}

export function createRemoteWorkspaceApi(): PreloadApi['remoteWorkspace'] {
  return withMethodFallback<PreloadApi['remoteWorkspace']>('remoteWorkspace', {
    clientId: async () => 'mock-client-id',
    setForConnectedTargets: async () => [],
    onChanged: () => () => {}
  })
}
