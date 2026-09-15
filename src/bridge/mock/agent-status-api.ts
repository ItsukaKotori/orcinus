// Phase 0 mock; replaced by Tauri IPC per view.
import type { PreloadApi } from '../../preload/api-types'
import { withMethodFallback } from '../unimplemented-fallback'
import { noopUnsubscribe } from './noop-unsubscribe'

export function createAgentStatusApi(): PreloadApi['agentStatus'] {
  return withMethodFallback<PreloadApi['agentStatus']>('agentStatus', {
    getSnapshot: async () => [],
    getMigrationUnsupportedSnapshot: async () => [],
    // Why void: the preload contract is fire-and-forget; a Promise would be a silent contract break.
    drop: () => {},
    onSet: () => noopUnsubscribe,
    onClear: () => noopUnsubscribe,
    onMigrationUnsupported: () => noopUnsubscribe,
    onMigrationUnsupportedClear: () => noopUnsubscribe,
    onLegacyWorkerTerminalRecovery: () => noopUnsubscribe
  })
}
