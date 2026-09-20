import { describe, expect, it } from 'vitest'
import { createSettingsApi } from './settings-api'

describe('Phase 0 settings mock', () => {
  it('does not leak closure state through the settings it returns', async () => {
    const settings = createSettingsApi()

    const read = await settings.get()
    read.pluginSystemEnabled = false
    read.notifications.suppressWhenFocused = false
    expect(settings.getSync()).toMatchObject({
      pluginSystemEnabled: true,
      notifications: { suppressWhenFocused: true }
    })

    const updated = await settings.set({ pluginSystemEnabled: false })
    updated.pluginSystemEnabled = true
    await expect(settings.get()).resolves.toMatchObject({ pluginSystemEnabled: false })
  })
})
