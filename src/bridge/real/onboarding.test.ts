import { invoke } from '@tauri-apps/api/core'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { createOnboardingRealApi } from './onboarding'

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }))

const invokeMock = vi.mocked(invoke)

beforeEach(() => {
  invokeMock.mockReset()
})

describe('onboarding real adapter commands', () => {
  it('maps get to onboarding_get without a payload', async () => {
    const state = { lastCompletedStep: 1 }
    invokeMock.mockResolvedValueOnce(state)
    await expect(createOnboardingRealApi().get()).resolves.toEqual(state)
    expect(invokeMock).toHaveBeenCalledWith('onboarding_get')
  })

  it('maps update to onboarding_update with the { args } envelope', async () => {
    const updates = { lastCompletedStep: 2, checklist: { choseAgent: true } }
    invokeMock.mockResolvedValueOnce({ lastCompletedStep: 2 })
    await expect(createOnboardingRealApi().update(updates)).resolves.toEqual({
      lastCompletedStep: 2
    })
    expect(invokeMock).toHaveBeenCalledWith('onboarding_update', { args: updates })
  })

  it('maps a {message} rejection to a normal Error', async () => {
    invokeMock.mockRejectedValueOnce({ message: 'onboarding write failed' })
    await expect(createOnboardingRealApi().get()).rejects.toBeInstanceOf(Error)
    invokeMock.mockRejectedValueOnce({ message: 'onboarding write failed' })
    await expect(createOnboardingRealApi().get()).rejects.toThrow('onboarding write failed')
  })
})
