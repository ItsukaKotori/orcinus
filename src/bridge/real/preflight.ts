import type { PreloadApi } from '../../shared/preload-api/api-types'
import { withMethodFallback } from '../unimplemented-fallback'
import { invokeCommand } from './invoke'

/**
 * Real `preflight` adapter (spec §5.10). Git availability reuses the `repos`
 * machine probe (`repos_is_git_available`) so the landing setup card reflects the
 * host instead of the all-negative Phase 0 mock; gh credentials are not wired
 * yet and report unavailable. Agent detection stays on `withMethodFallback`.
 */
export function createPreflightRealApi(): PreloadApi['preflight'] {
  return withMethodFallback<PreloadApi['preflight']>('preflight', {
    check: async () => ({
      git: { installed: await invokeCommand<boolean>('repos_is_git_available') },
      gh: { installed: false, authenticated: false }
    })
  })
}
