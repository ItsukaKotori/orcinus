import { describe, expect, it, vi } from 'vitest'
import { createSessionFlushPersist } from './session-flush-persist'

function makeDeps(overrides?: Partial<Parameters<typeof createSessionFlushPersist>[0]>) {
  const captureAll = vi.fn()
  const captureTranscripts = vi.fn().mockResolvedValue(undefined)
  const patch = vi.fn().mockResolvedValue(undefined)
  const flush = vi.fn().mockResolvedValue(undefined)
  const deps = {
    captureAll,
    captureTranscripts,
    buildPayload: () => ({ activeTabId: 't1' }),
    canPersist: () => true,
    patch,
    flush,
    ...overrides
  }
  return { deps, captureAll, captureTranscripts, patch, flush }
}

describe('createSessionFlushPersist', () => {
  it('captures buffers and transcripts before patching and flushing, in order', async () => {
    const { deps, captureAll, captureTranscripts, patch, flush } = makeDeps()
    const order: string[] = []
    captureAll.mockImplementation(() => order.push('capture'))
    captureTranscripts.mockImplementation(async () => {
      order.push('transcripts')
    })
    patch.mockImplementation(async () => {
      order.push('patch')
    })
    flush.mockImplementation(async () => {
      order.push('flush')
    })
    await createSessionFlushPersist(deps)()
    expect(order).toEqual(['capture', 'transcripts', 'patch', 'flush'])
    expect(patch).toHaveBeenCalledWith({ activeTabId: 't1' })
  })

  it('skips patch and flush when persistence is gated off', async () => {
    const { deps, captureAll, patch, flush } = makeDeps({ canPersist: () => false })
    await createSessionFlushPersist(deps)()
    expect(captureAll).toHaveBeenCalledTimes(1)
    expect(patch).not.toHaveBeenCalled()
    expect(flush).not.toHaveBeenCalled()
  })
})
