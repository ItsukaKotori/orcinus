import { describe, expect, it } from 'vitest'
import { UiUpdate } from './client-ui-params'

describe('UiUpdate activeView', () => {
  it('round-trips plugin-center and plugin-scoped views through the wire schema', () => {
    expect(UiUpdate.parse({ activeView: 'plugin-center' }).activeView).toBe('plugin-center')
    expect(UiUpdate.parse({ activeView: 'plugin:database-manager.main' }).activeView).toBe(
      'plugin:database-manager.main'
    )
  })

  it('drops an invalid activeView without failing the rest of the patch', () => {
    const parsed = UiUpdate.parse({ activeView: 'plugin:', sidebarWidth: 260 })

    expect(parsed.activeView).toBeUndefined()
    expect(parsed.sidebarWidth).toBe(260)
  })
})
