import type { PreloadApi } from '../../shared/preload-api/api-types'
import { withMethodFallback } from '../unimplemented-fallback'
import { invokeCommand } from './invoke'

/**
 * Real `onboarding` adapter. The wizard state is host-persisted
 * (`<app data dir>/onboarding.json`) so the guide no longer resets on every
 * launch; the mock's checklist field-by-field merge semantics live in the
 * Rust `OnboardingStore`.
 */
export function createOnboardingRealApi(): PreloadApi['onboarding'] {
  return withMethodFallback<PreloadApi['onboarding']>('onboarding', {
    get: () => invokeCommand('onboarding_get'),
    update: (args) => invokeCommand('onboarding_update', { args })
  })
}
