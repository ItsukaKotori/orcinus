import type { PreloadApi } from '../../preload/api-types'

/** Phase 0 mock：mock 模式与 web-stub 同形（规格 §5.2）。 */
export function createNotificationsApi(): PreloadApi['notifications'] {
  return {
    getDesktopAwayState: async () => undefined,
    dispatch: async () => ({ delivered: false, reason: 'not-supported' }),
    dismiss: async () => ({ dismissed: 0 }),
    openSystemSettings: async () => {},
    getPermissionStatus: async () => ({ supported: false, platform: 'darwin', requested: false }),
    probeDelivery: async () => ({ state: 'unsupported', authoritative: false }),
    playSound: async () => ({ played: false, reason: 'missing-path' })
  }
}
