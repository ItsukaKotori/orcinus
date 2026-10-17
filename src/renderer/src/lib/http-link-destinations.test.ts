import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { openRoutedHttpLink } from './http-link-destinations'

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

describe('openRoutedHttpLink', () => {
  it('opens the link in the system browser', () => {
    openRoutedHttpLink('https://example.com/', { sourceOwner: { kind: 'local' } })

    expect(openUrlMock).toHaveBeenCalledWith('https://example.com/')
  })

  it('opens remote-owned links in the system browser too', () => {
    openRoutedHttpLink('https://example.com/', {
      sourceOwner: { kind: 'runtime', runtimeEnvironmentId: 'env-1' }
    })

    expect(openUrlMock).toHaveBeenCalledWith('https://example.com/')
  })

  it('keeps unresolved document ownership non-actionable', () => {
    openRoutedHttpLink('https://example.com/', { sourceOwner: { kind: 'unknown' } })

    expect(openUrlMock).not.toHaveBeenCalled()
  })
})
