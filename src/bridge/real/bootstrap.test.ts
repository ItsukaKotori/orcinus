import { invoke } from '@tauri-apps/api/core'
import { afterEach, describe, expect, it, vi } from 'vitest'
import type { GlobalSettings } from '../../shared/global-settings-types'
import { getBootstrap, type AdeBootstrap } from './bootstrap'
import { createPlatformRealApi } from './platform'
import { createSettingsRealApi } from './settings'

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }))

const scope = globalThis as { __ADE_BOOTSTRAP__?: AdeBootstrap }

afterEach(() => {
  delete scope.__ADE_BOOTSTRAP__
  vi.mocked(invoke).mockReset()
})

function bootstrapPayload(): AdeBootstrap {
  return {
    settings: { theme: 'dark' } as GlobalSettings,
    platform: {
      platform: 'darwin',
      osRelease: 'Darwin 25.0.0',
      arch: 'aarch64',
      shell: '/bin/zsh',
      displayServer: null
    },
    schemaVersion: 1
  }
}

describe('bootstrap payload', () => {
  it('is absent when the init script did not run', () => {
    expect(getBootstrap()).toBeNull()
    expect(createSettingsRealApi().getSync()).toBeNull()
  })

  it('makes platform.get throw without a payload', () => {
    expect(() => createPlatformRealApi().get()).toThrow(/bootstrap/)
  })

  it('serves settings.getSync and platform.get from the injected snapshot', () => {
    scope.__ADE_BOOTSTRAP__ = bootstrapPayload()
    expect(createSettingsRealApi().getSync()).toEqual(bootstrapPayload().settings)
    expect(createPlatformRealApi().get()).toEqual(bootstrapPayload().platform)
  })

  it('refreshes the sync snapshot after settings.set resolves', async () => {
    const injected = bootstrapPayload()
    const original = structuredClone(injected)
    scope.__ADE_BOOTSTRAP__ = injected
    const updated = { ...injected.settings, theme: 'light' } as GlobalSettings
    vi.mocked(invoke).mockResolvedValueOnce(updated)

    const returned = await createSettingsRealApi().set({ theme: 'light' })

    expect(returned).toEqual(updated)
    expect(createSettingsRealApi().getSync()).toEqual(updated)
    expect(vi.mocked(invoke)).toHaveBeenCalledWith('settings_set', {
      args: { theme: 'light' }
    })
    // Why: the injected object is the init-script payload, not a write target.
    expect(scope.__ADE_BOOTSTRAP__).toEqual(original)
  })
})
