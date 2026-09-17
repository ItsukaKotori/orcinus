import { useCallback, useEffect, useState } from 'react'
import { onOnboardingReopened } from '../components/onboarding/show-onboarding-event'
import { shouldShowOnboarding } from '../components/onboarding/should-show-onboarding'
import type { OnboardingState } from '../../../shared/onboarding-state-types'

export type OnboardingGate = ReturnType<typeof useOnboarding>

/**
 * Owns the onboarding flow's visibility at the App root.
 */
export function useOnboarding() {
  const [onboarding, setOnboarding] = useState<OnboardingState | null>(null)

  const applyStartupOnboardingState = useCallback((state: OnboardingState): void => {
    setOnboarding(state)
  }, [])

  useEffect(() => {
    return onOnboardingReopened(setOnboarding)
  }, [])

  return {
    applyStartupOnboardingState,
    onboarding,
    setOnboarding,
    shouldRender: onboarding !== null && shouldShowOnboarding(onboarding)
  }
}
