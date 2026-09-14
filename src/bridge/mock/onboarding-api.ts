// Phase 0 mock; replaced by Tauri IPC per view.
import type { PreloadApi } from '../../preload/api-types'
import { getDefaultOnboardingState } from '../../shared/constants'
import { withUnimplementedFallback } from '../unimplemented-fallback'

export function createOnboardingApi(): PreloadApi['onboarding'] {
  return withUnimplementedFallback<PreloadApi['onboarding']>({
    get: async () => getDefaultOnboardingState()
  })
}
