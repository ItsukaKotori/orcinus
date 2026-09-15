// @vitest-environment happy-dom
import { describe, expect, it } from 'vitest'
import { persistStep } from '../renderer/src/components/onboarding/use-onboarding-flow-persistence'
import { createAdeApi } from './create-api'

describe('onboarding update through the installed mock bridge', () => {
  it('persistStep(1) resolves and get() reflects the step', async () => {
    window.api = createAdeApi()

    const state = await persistStep(1)

    expect(state.lastCompletedStep).toBe(1)
    await expect(window.api.onboarding.get()).resolves.toMatchObject({ lastCompletedStep: 1 })
  })
})
