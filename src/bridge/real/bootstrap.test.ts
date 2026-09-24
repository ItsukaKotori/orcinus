import { afterEach, describe, expect, it } from 'vitest'
import type { GlobalSettings } from '../../shared/global-settings-types'
import { getBootstrap, type AdeBootstrap } from './bootstrap'
import { createPlatformRealApi } from './platform'
import { createSettingsRealApi } from './settings'

const scope = globalThis as { __ADE_BOOTSTRAP__?: AdeBootstrap }

afterEach(() => {
  delete scope.__ADE_BOOTSTRAP__
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
})
