import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { getDefaultOnboardingState } from '../../../shared/constants'
import { markOnboardingProjectAdded } from './onboarding-project-checklist'

const mocks = vi.hoisted(() => ({
  onboardingGet: vi.fn(),
  onboardingUpdate: vi.fn()
}))

describe('markOnboardingProjectAdded', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    mocks.onboardingGet.mockResolvedValue(getDefaultOnboardingState())
    mocks.onboardingUpdate.mockResolvedValue(getDefaultOnboardingState())
    vi.stubGlobal('window', {
      api: {
        onboarding: {
          get: mocks.onboardingGet,
          update: mocks.onboardingUpdate
        }
      }
    })
  })

  afterEach(() => {
    vi.unstubAllGlobals()
  })

  it('marks an added Git project', async () => {
    await markOnboardingProjectAdded('addedRepo')

    expect(mocks.onboardingUpdate).toHaveBeenCalledWith({
      checklist: { addedRepo: true }
    })
  })

  it('marks an added folder', async () => {
    await markOnboardingProjectAdded('addedFolder')

    expect(mocks.onboardingUpdate).toHaveBeenCalledWith({
      checklist: { addedFolder: true }
    })
  })

  it('does not rewrite an already completed item', async () => {
    mocks.onboardingGet.mockResolvedValue({
      ...getDefaultOnboardingState(),
      checklist: { ...getDefaultOnboardingState().checklist, addedRepo: true }
    })

    await markOnboardingProjectAdded('addedRepo')

    expect(mocks.onboardingUpdate).not.toHaveBeenCalled()
  })
})
