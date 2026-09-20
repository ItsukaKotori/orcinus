import { describe, expect, it } from 'vitest'
import { createAgentAwakeApi } from './agent-awake-api'

describe('Phase 0 agentAwake mock', () => {
  it('does not leak module state through the status it returns', async () => {
    const agentAwake = createAgentAwakeApi()

    const status = await agentAwake.getStatus()
    status.mode = 'on'
    status.active = true

    await expect(agentAwake.getStatus()).resolves.toEqual({ mode: 'off', active: false })
  })
})
