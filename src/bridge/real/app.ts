import type { AppApi } from '../../shared/preload-api/api/app-api'
import { createAppApi } from '../mock/app-api'
import { invokeCommand } from './invoke'

/**
 * Real `app` adapter (spec §2.1/§5.5): only `getIdentity` is backed by a command
 * in A. The host services (relaunch/restart, startup barriers, keyboard probes)
 * are not ported yet, so the Phase 0 benign implementations are reused instead
 * of fabricated rejections: renderer startup awaits the barriers unguarded, and
 * the barriers are trivially satisfied without those services.
 */
export function createAppRealApi(): AppApi {
  return {
    ...createAppApi(),
    getIdentity: () => invokeCommand('app_get_identity')
  }
}
