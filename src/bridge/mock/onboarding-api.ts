// Phase 0 mock; replaced by Tauri IPC per view.
import type { PreloadApi } from '../../preload/api-types'
import { getDefaultOnboardingState } from '../../shared/constants'
import { withUnimplementedFallback } from '../unimplemented-fallback'

export function createOnboardingApi(): PreloadApi['onboarding'] {
  let state = getDefaultOnboardingState()
  return withUnimplementedFallback<PreloadApi['onboarding']>({
    get: async () => state,
    update: async (updates) => {
      state = {
        ...state,
        ...updates,
        // Why: a partial checklist must not wipe flags the UI already observed.
        checklist: { ...state.checklist, ...updates.checklist }
      }
      return state
    }
  })
}
