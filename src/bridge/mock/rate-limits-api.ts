// Phase 0 mock; replaced by Tauri IPC per view.
import type { PreloadApi } from '../../preload/api-types'
import { createEmptyRateLimitState } from '../../shared/rate-limit-state-factory'
import { withMethodFallback } from '../unimplemented-fallback'
import { noopUnsubscribe } from './noop-unsubscribe'

export function createRateLimitsApi(): PreloadApi['rateLimits'] {
  return withMethodFallback<PreloadApi['rateLimits']>('rateLimits', {
    // Why the shared factory: a new provider field must not silently leave the mock's state malformed.
    get: async () => createEmptyRateLimitState(),
    onUpdate: () => noopUnsubscribe
  })
}
