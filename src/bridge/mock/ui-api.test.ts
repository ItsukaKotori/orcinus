import { describe, expect, it } from 'vitest'
import { createUiApi } from './ui-api'

describe('Phase 0 ui mock', () => {
  it('does not leak closure state through the persisted ui state it returns', async () => {
    const ui = createUiApi()

    const state = await ui.get()
    state.rightSidebarOpen = false
    state.manualRepoOrder?.push({ hostId: 'local', repoId: 'polluted-repo' })
    await expect(ui.get()).resolves.toMatchObject({
      rightSidebarOpen: true,
      manualRepoOrder: []
    })

    const recorded = await ui.recordFeatureInteraction('cmd-j')
    recorded.activeView = 'settings'
    await expect(ui.get()).resolves.toMatchObject({ activeView: 'terminal' })
  })
})
