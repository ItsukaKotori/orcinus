import type { PlatformApi } from '../../shared/preload-api/api/app-api'
import { getBootstrap } from './bootstrap'

/**
 * `platform.get()` is synchronous by contract (the renderer reads fields
 * without awaiting); it can only be served from the bootstrap payload.
 */
export function createPlatformRealApi(): PlatformApi {
  return {
    get: () => {
      const bootstrap = getBootstrap()
      if (!bootstrap) {
        throw new Error(
          'platform.get() requires the ADE bootstrap payload (window.__ADE_BOOTSTRAP__)'
        )
      }
      return bootstrap.platform
    }
  }
}
