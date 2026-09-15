// Phase 0 mock; replaced by Tauri IPC per view.
import type { PreloadApi } from '../../preload/api-types'
import { withMethodFallback } from '../unimplemented-fallback'
import { noopUnsubscribe } from './noop-unsubscribe'
import type { ComputerAwakeStatus } from '../../shared/computer-awake-mode'

const INACTIVE_STATUS: ComputerAwakeStatus = { mode: 'off', active: false }

export function createAgentAwakeApi(): PreloadApi['agentAwake'] {
  // Why: the status bar's Caffeinate segment mounts at boot and expects onChanged to return an
  // unsubscribe function synchronously (the preload contract); a Promise here crashes the segment.
  return withMethodFallback<PreloadApi['agentAwake']>('agentAwake', {
    getStatus: async () => INACTIVE_STATUS,
    onChanged: () => noopUnsubscribe
  })
}
