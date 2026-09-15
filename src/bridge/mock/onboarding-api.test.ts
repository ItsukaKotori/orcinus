import { describe, expect, it } from 'vitest'
import { createOnboardingApi } from './onboarding-api'

describe('Phase 0 onboarding mock', () => {
  it('update resolves the merged state and get reflects it', async () => {
    const onboarding = createOnboardingApi()

    await expect(onboarding.update({ lastCompletedStep: 1 })).resolves.toMatchObject({
      lastCompletedStep: 1
    })
    await expect(onboarding.get()).resolves.toMatchObject({ lastCompletedStep: 1 })
  })

  it('update merges the checklist field-by-field, keeping existing flags', async () => {
    const onboarding = createOnboardingApi()

    await onboarding.update({ checklist: { choseAgent: true } })
    const state = await onboarding.update({ checklist: { dismissed: true } })

    expect(state.checklist).toMatchObject({ choseAgent: true, dismissed: true, addedRepo: false })
    await expect(onboarding.get()).resolves.toMatchObject({
      checklist: { choseAgent: true, dismissed: true, addedRepo: false }
    })
  })
})
