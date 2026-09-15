// Phase 0 mock; replaced by Tauri IPC per view.
import type { StarNagApi } from '../../preload/api/onboarding-api'

export function createStarNagApi(): StarNagApi {
  // StarNagToastHost is always mounted and subscribes at boot; all user-triggered methods
  // resolve as no-ops so the Phase 0 shell never sees a rejected star-nag call.
  return {
    onShow: () => () => {},
    onHide: () => () => {},
    dismiss: async () => {},
    later: async () => {},
    complete: async () => {},
    disable: async () => {},
    openWeb: async () => {},
    starOrca: async () => false,
    forceShow: async () => {},
    agentValueMoment: async () => ({ status: 'skipped' }),
    showAgentValueMoment: async () => {},
    onboardingCompleted: async () => {}
  }
}
