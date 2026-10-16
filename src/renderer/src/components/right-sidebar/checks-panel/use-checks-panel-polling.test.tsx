// @vitest-environment happy-dom

import { act, cleanup, renderHook } from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { PRCheckDetail } from '../../../../../shared/github/check-types'

const poller = vi.hoisted(() => ({
  install: vi.fn(),
  run: null as null | (() => Promise<void> | void),
  getDelayMs: null as null | (() => number),
  cleanup: vi.fn()
}))

vi.mock('@/lib/window-visibility-timeout-poller', () => ({
  installWindowVisibilityTimeoutPoller: vi.fn(
    (config: { run: () => Promise<void> | void; getDelayMs: () => number }) => {
      poller.run = config.run
      poller.getDelayMs = config.getDelayMs
      poller.install()
      return poller.cleanup
    }
  )
}))

import { useChecksPanelPolling } from './use-checks-panel-polling'

type PollingInput = Parameters<typeof useChecksPanelPolling>[0]

function createModel(overrides: Partial<PollingInput> = {}): PollingInput {
  const fetchPRChecks = vi.fn<() => Promise<PRCheckDetail[]>>().mockResolvedValue([])
  return {
    branch: 'main',
    fetchPRChecks,
    isCurrentAsyncResult: () => true,
    isPanelVisible: true,
    pollIntervalRef: { current: 30_000 },
    pr: {
      number: 42,
      headSha: 'head-1',
      prRepo: { owner: 'orca', repo: 'app', host: 'github.com' }
    } as NonNullable<PollingInput['pr']>,
    prCacheKey: 'cache',
    prNumber: 42,
    prevChecksRef: { current: '' },
    repo: { id: 'repo-1', path: '/workspace/repo' } as NonNullable<PollingInput['repo']>,
    setChecks: vi.fn(),
    setChecksLoading: vi.fn(),
    ...overrides
  }
}

beforeEach(() => {
  poller.install.mockReset()
  poller.cleanup.mockReset()
  poller.run = null
  poller.getDelayMs = null
})

afterEach(() => {
  cleanup()
})

describe('useChecksPanelPolling live behavior', () => {
  it('gates installation by panel visibility and cleans the active poller', () => {
    const model = createModel({ isPanelVisible: false })
    const hook = renderHook(({ input }) => useChecksPanelPolling(input), {
      initialProps: { input: model }
    })

    expect(poller.install).not.toHaveBeenCalled()

    hook.rerender({ input: { ...model, isPanelVisible: true } })
    expect(poller.install).toHaveBeenCalledOnce()
    expect(poller.getDelayMs?.()).toBe(30_000)

    hook.rerender({ input: { ...model, isPanelVisible: false } })
    expect(poller.cleanup).toHaveBeenCalledOnce()
  })

  it('preserves live repeated-empty backoff at 30, 60, then 120 seconds', async () => {
    const model = createModel()
    renderHook(() => useChecksPanelPolling(model))

    await act(async () => poller.run?.())
    expect(model.pollIntervalRef.current).toBe(30_000)
    expect(poller.getDelayMs?.()).toBe(30_000)
    await act(async () => poller.run?.())
    expect(model.pollIntervalRef.current).toBe(60_000)
    expect(poller.getDelayMs?.()).toBe(60_000)
    await act(async () => poller.run?.())
    expect(model.pollIntervalRef.current).toBe(120_000)
    expect(poller.getDelayMs?.()).toBe(120_000)
  })

  it('resets backoff when checks change between polls', async () => {
    const first: PRCheckDetail[] = [
      { name: 'build', status: 'in_progress', conclusion: null, url: null }
    ]
    const second: PRCheckDetail[] = [
      { name: 'build', status: 'completed', conclusion: 'success', url: null }
    ]
    const model = createModel({
      fetchPRChecks: vi
        .fn<() => Promise<PRCheckDetail[]>>()
        .mockResolvedValueOnce(first)
        .mockResolvedValue(second)
    })
    renderHook(() => useChecksPanelPolling(model))

    await act(async () => poller.run?.())
    expect(model.pollIntervalRef.current).toBe(30_000)
    await act(async () => poller.run?.())
    expect(model.pollIntervalRef.current).toBe(30_000)
    await act(async () => poller.run?.())
    expect(model.pollIntervalRef.current).toBe(60_000)
  })
})
