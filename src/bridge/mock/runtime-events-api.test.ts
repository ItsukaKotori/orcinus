import { describe, expect, it } from 'vitest'
import { createRuntimeApi } from './runtime-events-api'

describe('Phase 0 runtime mock', () => {
  it('does not leak module state through the status it returns', async () => {
    const runtime = createRuntimeApi()

    const status = await runtime.getStatus()
    status.liveTabCount = 4
    status.runtimeId = 'polluted'
    await expect(runtime.getStatus()).resolves.toMatchObject({
      runtimeId: 'phase0-local',
      liveTabCount: 0
    })

    const synced = await runtime.syncWindowGraph({
      tabs: [],
      leaves: [],
      rendererGeneration: 'test'
    })
    synced.graphStatus = 'reloading'
    await expect(runtime.getStatus()).resolves.toMatchObject({ graphStatus: 'ready' })
  })
})
