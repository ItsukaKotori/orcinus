// Phase 0 mock; replaced by Tauri IPC per view.
import type { PreloadApi } from '../../preload/api-types'
import { getDefaultWorkspaceSession } from '../../shared/constants'
import { withUnimplementedFallback } from '../unimplemented-fallback'

export function createSessionApi(): PreloadApi['session'] {
  return withUnimplementedFallback<PreloadApi['session']>({
    get: async () => getDefaultWorkspaceSession(),
    patch: async () => {}
  })
}

export function createRemoteWorkspaceApi(): PreloadApi['remoteWorkspace'] {
  return withUnimplementedFallback<PreloadApi['remoteWorkspace']>({
    clientId: async () => 'mock-client-id',
    setForConnectedTargets: async () => [],
    onChanged: () => () => {}
  })
}
