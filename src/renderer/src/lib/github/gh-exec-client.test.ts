import { describe, expect, it, vi } from 'vitest'
import { createGhExecClient, GhRunError } from './gh-exec-client'

describe('gh exec client', () => {
  it('returns the executor result on success', async () => {
    const executor = vi.fn(async () => ({ stdout: 'ok', stderr: '', code: 0 }))
    const client = createGhExecClient(executor)
    await expect(client.run(['auth', 'status'])).resolves.toEqual({ stdout: 'ok', stderr: '', code: 0 })
    expect(executor).toHaveBeenCalledTimes(1)
  })

  it('retries transient failures up to 3 attempts', async () => {
    vi.useFakeTimers()
    const executor = vi
      .fn()
      .mockResolvedValueOnce({ stdout: '', stderr: 'HTTP 502 Bad Gateway', code: 1 })
      .mockResolvedValueOnce({ stdout: 'ok', stderr: '', code: 0 })
    const client = createGhExecClient(executor)
    const pending = client.run(['api', 'rate_limit'])
    await vi.runAllTimersAsync()
    await expect(pending).resolves.toMatchObject({ stdout: 'ok' })
    expect(executor).toHaveBeenCalledTimes(2)
    vi.useRealTimers()
  })

  it('does not retry rate-limit stderr with Retry-After', async () => {
    const executor = vi.fn(async () => ({
      stdout: '',
      stderr: 'HTTP 429: rate limit exceeded\nRetry-After: 60',
      code: 1
    }))
    const client = createGhExecClient(executor)
    await expect(client.run(['api', 'rate_limit'])).resolves.toMatchObject({ code: 1 })
    expect(executor).toHaveBeenCalledTimes(1)
  })

  it('runOrThrow raises GhRunError carrying stderr', async () => {
    const executor = vi.fn(async () => ({ stdout: '', stderr: 'boom', code: 2 }))
    const client = createGhExecClient(executor)
    const pending = client.runOrThrow(['pr', 'view', '1'])
    await expect(pending).rejects.toBeInstanceOf(GhRunError)
    await expect(pending).rejects.toMatchObject({
      name: 'GhRunError',
      stderr: 'boom',
      code: 2
    })
  })
})
