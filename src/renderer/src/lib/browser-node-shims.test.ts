import { describe, expect, it } from 'vitest'
import {
  Buffer,
  createHash,
  homedir,
  isAbsolute,
  join,
  openSync,
  statSync
} from './browser-node-shims'

describe('browser node shims for the agent-hook normalization graph', () => {
  it('provides a Buffer sufficient for module-load constant allocation', () => {
    expect(Buffer.alloc(0)).toBeInstanceOf(Uint8Array)
    const joined = Buffer.concat([Buffer.from('ab'), Buffer.from('c')])
    expect(joined).toBeInstanceOf(Uint8Array)
    expect(Buffer.byteLength('héllo')).toBe(6)
  })

  it('throws only when a node-only transcript/crypto path is actually reached', () => {
    expect(() => statSync('/tmp/x')).toThrow(/not available in the renderer/)
    expect(() => openSync('/tmp/x', 'r')).toThrow(/not available in the renderer/)
    expect(() => createHash('sha256')).toThrow(/not available in the renderer/)
  })

  it('keeps the pure path helpers used by lazily-reached transcript discovery benign', () => {
    expect(homedir()).toBe('')
    expect(join('a', 'b', 'c')).toBe('a/b/c')
    expect(isAbsolute('/tmp')).toBe(true)
    expect(isAbsolute('tmp')).toBe(false)
  })
})
