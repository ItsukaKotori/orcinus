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

  it('platform.get returns platform info synchronously, not a rejection promise', () => {
    const api = createAdeApi()
    const info = api.platform.get()
    expect(info).not.toBeInstanceOf(Promise)
    expect(['darwin', 'win32', 'linux']).toContain(info.platform)
    expect(info.displayServer).toBeNull()
  })

  it('browser WebAuthn subscriptions hand back unsubscribe functions', () => {
    const api = createAdeApi()
    const stopRequests = api.browser.onWebAuthnAccountRequest(() => {})
    expect(typeof stopRequests).toBe('function')
    stopRequests()
    const stopClosed = api.browser.onWebAuthnAccountRequestClosed(() => {})
    expect(typeof stopClosed).toBe('function')
    stopClosed()
  })
})
