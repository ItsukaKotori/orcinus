import { describe, expect, it } from 'vitest'
import { createAdeApi } from './create-api'

describe('createAdeApi', () => {
  it('exposes mocked namespaces with live data', async () => {
    const api = createAdeApi()
    // SAFETY: PluginHostListEntry has no `scope`; the runtime entries contributed by the mock do.
    const entries = (await api.plugins.list()) as Array<{ scope?: string }>
    expect(entries.some((entry) => entry.scope === 'global')).toBe(true)
    expect(entries.some((entry) => entry.scope === 'project')).toBe(true)
  })
})
