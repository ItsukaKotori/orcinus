// Phase 0 mock; replaced by Tauri IPC per view.
import type { PreloadApi } from '../../preload/api-types'
import { getDefaultOnboardingState } from '../../shared/constants'
import { withMethodFallback } from '../unimplemented-fallback'
import { cloneMockValue } from './clone-mock-value'

export function createOnboardingApi(): PreloadApi['onboarding'] {
  let state = getDefaultOnboardingState()
  return withMethodFallback<PreloadApi['onboarding']>('onboarding', {
    get: async () => cloneMockValue(state),
    update: async (updates) => {
      state = {
        ...state,
        ...updates,
        // Why: a partial checklist must not wipe flags the UI already observed.
        checklist: { ...state.checklist, ...updates.checklist }
      }
      return cloneMockValue(state)
    }
  })
}
