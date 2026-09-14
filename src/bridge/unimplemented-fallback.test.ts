import { describe, expect, it, vi } from 'vitest'
import { withUnimplementedFallback, UnimplementedBridgeError } from './unimplemented-fallback'

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
