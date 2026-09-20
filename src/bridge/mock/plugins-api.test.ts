import { describe, expect, it } from 'vitest'
import { createPluginsApi } from './plugins-api'

describe('Phase 0 plugins mock', () => {
  it('does not leak closure state through the entries list and consent return', async () => {
    const plugins = createPluginsApi()

    const entries = await plugins.list()
    entries[0].status = 'disabled'
    entries[0].capabilities.push({ kind: 'workspace:read', description: 'polluted' })
    entries.push({ ...entries[0], pluginKey: 'polluted' })

    const reread = await plugins.list()
    expect(reread).toHaveLength(3)
    expect(reread[0]).toMatchObject({ pluginKey: 'database-manager', status: 'idle' })
    expect(reread[0].capabilities).toHaveLength(1)

    const consented = await plugins.consent({
      pluginKey: 'database-manager',
      reviewedFingerprint: 'mock-fingerprint-db',
      decision: 'approve'
    })
    consented.pop()
    await expect(plugins.refresh()).resolves.toHaveLength(3)
  })
})
