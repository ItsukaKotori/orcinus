// Phase 0 mock; replaced by Tauri IPC per view.
import type { PlatformApi } from '../../preload/api/app-api'

export function detectBrowserPlatform(): NodeJS.Platform {
  if (navigator.userAgent.includes('Windows')) {
    return 'win32'
  }
  if (navigator.userAgent.includes('Linux')) {
    return 'linux'
  }
  return 'darwin'
}

export function createPlatformApi(): PlatformApi {
  // Why synchronous: the preload contract returns PlatformInfo directly, and renderer call sites
  // read fields without awaiting; a Promise would silently break them and reject unhandled.
  return {
    get: () => ({
      platform: detectBrowserPlatform(),
      osRelease: '',
      arch: '',
      shell: '',
      displayServer: null
    })
  }
}
