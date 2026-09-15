import { describe, expect, it, vi } from 'vitest'
import {
  withMethodFallback,
  withUnimplementedFallback,
  UnimplementedBridgeError
} from './unimplemented-fallback'

describe('withUnimplementedFallback', () => {
  it('passes through implemented namespaces', async () => {
    const api = withUnimplementedFallback({ app: { ping: async () => 'pong' } })
    await expect(api.app.ping()).resolves.toBe('pong')
  })

  it('rejects unknown namespace methods with a tagged error', async () => {
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => {})
    const api = withUnimplementedFallback({})
    await expect(api.files.readFile({})).rejects.toBeInstanceOf(UnimplementedBridgeError)
    expect(warn).toHaveBeenCalledWith(expect.stringContaining('files.readFile'))
  })
})

describe('withMethodFallback', () => {
  it('passes through implemented methods', () => {
    const api = withMethodFallback('browser', { onRequest: () => () => {} })
    expect(typeof api.onRequest()).toBe('function')
  })

  it('rejects unknown methods with the namespaced path', async () => {
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => {})
    const api = withMethodFallback('browser', { onRequest: () => () => {} })
    await expect(api.setViewportOverride({})).rejects.toBeInstanceOf(UnimplementedBridgeError)
    expect(warn).toHaveBeenCalledWith(
      expect.stringContaining('browser.setViewportOverride')
    )
  })
})
