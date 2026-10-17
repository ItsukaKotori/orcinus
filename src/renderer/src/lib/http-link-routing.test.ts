import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { openHttpLink, resolveLocalhostHttpLinkDisplayUrl } from './http-link-routing'

const openUrlMock = vi.fn()

beforeEach(() => {
  vi.stubGlobal('window', {
    api: {
      shell: {
        openUrl: openUrlMock
      }
    }
  })
})

afterEach(() => {
  vi.clearAllMocks()
  vi.unstubAllGlobals()
})

describe('openHttpLink', () => {
  it('opens an http link in the system browser', () => {
    openHttpLink('https://example.com/')

    expect(openUrlMock).toHaveBeenCalledWith('https://example.com/')
  })

  it('opens local, runtime, and SSH owners in the system browser', () => {
    openHttpLink('https://example.com/', { sourceOwner: { kind: 'local' } })
    openHttpLink('https://example.com/', {
      sourceOwner: { kind: 'runtime', runtimeEnvironmentId: 'env-1' }
    })
    openHttpLink('https://example.com/', {
      sourceOwner: { kind: 'ssh', connectionId: 'ssh-1' }
    })

    expect(openUrlMock).toHaveBeenCalledTimes(3)
  })

  it('keeps unresolved document ownership non-actionable', () => {
    openHttpLink('https://example.com/', { sourceOwner: { kind: 'unknown' } })

    expect(openUrlMock).not.toHaveBeenCalled()
  })
})

describe('resolveLocalhostHttpLinkDisplayUrl', () => {
  it('resolves no localhost display URL without the workspace port scan', async () => {
    await expect(resolveLocalhostHttpLinkDisplayUrl('http://localhost:5180/')).resolves.toBe(null)
    await expect(
      resolveLocalhostHttpLinkDisplayUrl('http://localhost:5180/', { kind: 'local' })
    ).resolves.toBe(null)
  })
})
