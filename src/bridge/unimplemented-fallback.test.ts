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

  it('leaves then and symbol properties unfabricated so the namespace is not thenable', async () => {
    type Namespace = { files: { readFile: (target: unknown) => Promise<string> } }
    const api = withUnimplementedFallback<Namespace>({})
    await expect(Promise.resolve(api.files)).resolves.toBe(api.files)
    expect(Reflect.get(api.files, Symbol.iterator)).toBeUndefined()
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

  it('leaves then and symbol properties unfabricated so it is not thenable', async () => {
    const api = withMethodFallback('browser', { onRequest: () => () => {} })
    await expect(Promise.resolve(api)).resolves.toBe(api)
    expect(Reflect.get(api, Symbol.iterator)).toBeUndefined()
  })
})
