import type { OnboardingState } from '../../../shared/onboarding-state-types'

export type OnboardingProjectChecklistItem = 'addedRepo' | 'addedFolder'

export async function markOnboardingProjectAdded(
  item: OnboardingProjectChecklistItem
): Promise<void> {
  if (typeof window === 'undefined' || !window.api?.onboarding) {
    return
  }
  const onboarding = await window.api.onboarding.get().catch(() => null)
  if (!onboarding || onboarding.checklist[item]) {
    return
  }

  const checklist: Partial<OnboardingState['checklist']> = {}
  checklist[item] = true
  try {
    await window.api.onboarding.update({ checklist })
  } catch (err) {
    console.warn('[onboarding] Failed to update project checklist item:', err)
  }
}
