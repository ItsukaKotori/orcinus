// Phase 0 mock; replaced by Tauri IPC per view.
import type { PreloadApi } from '../../preload/api-types'
import { withMethodFallback } from '../unimplemented-fallback'
import { noopUnsubscribe } from './noop-unsubscribe'

export function createGhApi(): PreloadApi['gh'] {
  // Why null: checkOrcaStarred's contract returns null when the star state cannot be resolved, which
  // the landing surface renders as its web fallback instead of retrying gh CLI access.
  return withMethodFallback<PreloadApi['gh']>('gh', {
    checkOrcaStarred: async () => null,
    reportVisiblePRRefreshCandidates: async () => false,
    onPRRefreshEvent: () => noopUnsubscribe
  })
}
