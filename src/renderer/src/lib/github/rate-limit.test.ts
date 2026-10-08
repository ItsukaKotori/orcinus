import { describe, expect, it, vi } from 'vitest'
import { createRateLimitClient } from './rate-limit'
import type { GhExecClient } from './repo-identity'

const RESET_NOW = 1_700_000_000_000

const RATE_LIMIT_PAYLOAD = {
  resources: {
    core: { limit: 5000, remaining: 4999, reset: 1700000000 },
    search: { limit: 30, remaining: 29, reset: 1700000060 },
    graphql: { limit: 5000, remaining: 4998, reset: 1700000000 }
  }
}

function createHarness() {
  let currentNow = RESET_NOW
  const runOrThrow = vi.fn(async () => JSON.stringify(RATE_LIMIT_PAYLOAD))
  const client: GhExecClient = {
    run: vi.fn(async () => ({ stdout: '', stderr: '', code: 0 })),
    runOrThrow
  }
  const rateLimit = createRateLimitClient({ client, now: () => currentNow })
  return {
    rateLimit,
    runOrThrow,
    advance: (ms: number) => {
      currentNow += ms
    }
  }
}

describe('rate limit snapshot client', () => {
  it('maps core/search/graphql buckets and stamps fetchedAt', async () => {
    const { rateLimit, runOrThrow } = createHarness()
    const result = await rateLimit.getRateLimit()
    expect(runOrThrow).toHaveBeenCalledWith(['api', 'rate_limit'], {})
    expect(result).toEqual({
      ok: true,
      snapshot: {
        core: { limit: 5000, remaining: 4999, resetAt: 1700000000 },
        search: { limit: 30, remaining: 29, resetAt: 1700000060 },
        graphql: { limit: 5000, remaining: 4998, resetAt: 1700000000 },
        fetchedAt: RESET_NOW
      }
    })
  })

  it('falls back to 0/0/now for missing buckets and fields', async () => {
    const { rateLimit, runOrThrow } = createHarness()
    runOrThrow.mockResolvedValueOnce(JSON.stringify({ resources: { core: { remaining: 5 } } }))
    const result = await rateLimit.getRateLimit()
    expect(result).toEqual({
      ok: true,
      snapshot: {
        core: { limit: 0, remaining: 5, resetAt: Math.floor(RESET_NOW / 1000) },
        search: { limit: 0, remaining: 0, resetAt: Math.floor(RESET_NOW / 1000) },
        graphql: { limit: 0, remaining: 0, resetAt: Math.floor(RESET_NOW / 1000) },
        fetchedAt: RESET_NOW
      }
    })
  })

  it('serves a fresh snapshot from cache for 30s and re-probes after', async () => {
    const { rateLimit, runOrThrow, advance } = createHarness()
    const first = await rateLimit.getRateLimit()
    advance(29_999)
    await expect(rateLimit.getRateLimit()).resolves.toEqual(first)
    expect(runOrThrow).toHaveBeenCalledTimes(1)

    advance(1)
    await rateLimit.getRateLimit()
    expect(runOrThrow).toHaveBeenCalledTimes(2)
  })

  it('bypasses the cache with force', async () => {
    const { rateLimit, runOrThrow, advance } = createHarness()
    await rateLimit.getRateLimit()
    advance(1_000)
    await rateLimit.getRateLimit({ force: true })
    expect(runOrThrow).toHaveBeenCalledTimes(2)
  })

  it('collapses concurrent probes into one spawn', async () => {
    let release!: (value: string) => void
    const runOrThrow = vi.fn(
      () =>
        new Promise<string>((resolve) => {
          release = resolve
        })
    )
    const client: GhExecClient = { run: vi.fn(), runOrThrow }
    const rateLimit = createRateLimitClient({ client, now: () => RESET_NOW })
    const first = rateLimit.getRateLimit()
    const second = rateLimit.getRateLimit()
    release(JSON.stringify(RATE_LIMIT_PAYLOAD))
    await expect(first).resolves.toMatchObject({ ok: true })
    await expect(second).resolves.toMatchObject({ ok: true })
    expect(runOrThrow).toHaveBeenCalledTimes(1)
  })

  it('returns {ok:false,error} and negatively caches the failure for 30s', async () => {
    const { rateLimit, runOrThrow, advance } = createHarness()
    runOrThrow.mockRejectedValueOnce(new Error('HTTP 403: rate limit exceeded'))
    await expect(rateLimit.getRateLimit()).resolves.toEqual({
      ok: false,
      error: 'HTTP 403: rate limit exceeded'
    })

    advance(29_000)
    await expect(rateLimit.getRateLimit()).resolves.toEqual({
      ok: false,
      error: 'HTTP 403: rate limit exceeded'
    })
    expect(runOrThrow).toHaveBeenCalledTimes(1)

    advance(1_000)
    await expect(rateLimit.getRateLimit()).resolves.toMatchObject({ ok: true })
    expect(runOrThrow).toHaveBeenCalledTimes(2)
  })

  it('lets force retry past a negatively cached failure', async () => {
    const { rateLimit, runOrThrow } = createHarness()
    runOrThrow.mockRejectedValueOnce(new Error('boom'))
    await rateLimit.getRateLimit()
    await expect(rateLimit.getRateLimit({ force: true })).resolves.toMatchObject({ ok: true })
    expect(runOrThrow).toHaveBeenCalledTimes(2)
  })
})
