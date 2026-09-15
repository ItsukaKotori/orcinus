// Phase 0 mock; replaced by Tauri IPC per view.
import type { PreloadApi } from '../../preload/api-types'
import { withMethodFallback } from '../unimplemented-fallback'

export function createJiraApi(): PreloadApi['jira'] {
  // Why disconnected: the status surfaces render their "connect Jira" affordance for an
  // unconnected account, which is the only truthful Phase 0 state.
  return withMethodFallback<PreloadApi['jira']>('jira', {
    status: async () => ({ connected: false, viewer: null }),
    readStatus: async () => ({ connected: false, viewer: null })
  })
}
