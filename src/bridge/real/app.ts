import type { AppApi } from '../../shared/preload-api/api/app-api'
import { withMethodFallback } from '../unimplemented-fallback'
import { invokeCommand } from './invoke'

/**
 * Real `app` adapter (spec §2.1/§5.5): only `getIdentity` is backed by a
 * command in A; relaunch/restart, the startup barriers, and the macOS
 * keyboard probes stay unimplemented until their subprojects land.
 */
export function createAppRealApi(): AppApi {
  return withMethodFallback<AppApi>('app', {
    getIdentity: () => invokeCommand('app_get_identity')
  })
}
