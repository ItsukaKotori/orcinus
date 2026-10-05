// @vitest-environment happy-dom

import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { createTerminalBufferCaptureScheduler } from './terminal-buffer-capture-scheduler'

describe('createTerminalBufferCaptureScheduler', () => {
  beforeEach(() => {
    vi.useFakeTimers()
  })
  afterEach(() => {
    vi.useRealTimers()
  })

  it('captures on the interval while visible', () => {
    const captureAll = vi.fn()
    const stop = createTerminalBufferCaptureScheduler({
      captureAll,
      isDocumentHidden: () => false
    })
    vi.advanceTimersByTime(120_000)
    expect(captureAll).toHaveBeenCalledTimes(2)
    stop()
  })

  it('skips the interval capture while hidden', () => {
    const captureAll = vi.fn()
    const stop = createTerminalBufferCaptureScheduler({
      captureAll,
      isDocumentHidden: () => true
    })
    vi.advanceTimersByTime(120_000)
    expect(captureAll).not.toHaveBeenCalled()
    stop()
  })

  it('captures immediately when visibility turns hidden', () => {
    const captureAll = vi.fn()
    let hidden = false
    const stop = createTerminalBufferCaptureScheduler({
      captureAll,
      isDocumentHidden: () => hidden
    })
    hidden = true
    document.dispatchEvent(new Event('visibilitychange'))
    expect(captureAll).toHaveBeenCalledTimes(1)
    stop()
  })

  it('stop tears down the interval and the listener', () => {
    const captureAll = vi.fn()
    let hidden = true
    const stop = createTerminalBufferCaptureScheduler({
      captureAll,
      isDocumentHidden: () => hidden
    })
    stop()
    vi.advanceTimersByTime(120_000)
    document.dispatchEvent(new Event('visibilitychange'))
    expect(captureAll).not.toHaveBeenCalled()
  })
})
