import { describe, expect, it, vi } from 'vitest'
import { createGhReadinessProbe } from './preflight-gh'
import type { GhExecClient } from './repo-identity'

const NOW = 1_700_000_000_000

const ACTIVE_STATUS = [
  'github.com',
  '  ✓ Logged in to github.com account alice (keyring)',
  '  - Active account: true',
  "  - Token scopes: 'project', 'read:org', 'repo'",
  ''
].join('\n')

const INACTIVE_STATUS = [
  'github.com',
  '  ✓ Logged in to github.com account bob (keyring)',
  '  - Active account: false',
  ''
].join('\n')

function createHarness() {
  let currentNow = NOW
  const run = vi.fn<GhExecClient['run']>(async () => ({
    stdout: '',
    stderr: ACTIVE_STATUS,
    code: 0
  }))
  const client: GhExecClient = { run, runOrThrow: vi.fn() }
  const probe = createGhReadinessProbe({ client, now: () => currentNow })
  return {
    probe,
    run,
    advance: (ms: number) => {
      currentNow += ms
    }
  }
}

describe('gh readiness probe', () => {
  it('reports installed+authenticated for an active account', async () => {
    const { probe, run } = createHarness()
    await expect(probe()).resolves.toEqual({ installed: true, authenticated: true })
    expect(run).toHaveBeenCalledWith(['auth', 'status'])
  })

  it('reports not installed when gh is missing', async () => {
    const { probe, run } = createHarness()
    run.mockRejectedValueOnce(new Error('gh: command not found on PATH'))
    await expect(probe()).resolves.toEqual({ installed: false, authenticated: false })
  })

  it('parses the auth status even on a non-zero exit code', async () => {
    const { probe, run } = createHarness()
    run.mockResolvedValueOnce({ stdout: '', stderr: ACTIVE_STATUS, code: 1 })
    await expect(probe()).resolves.toEqual({ installed: true, authenticated: true })
  })

  it('reports installed but unauthenticated without an active account', async () => {
    const { probe, run } = createHarness()
    run.mockResolvedValueOnce({ stdout: '', stderr: INACTIVE_STATUS, code: 1 })
    await expect(probe()).resolves.toEqual({ installed: true, authenticated: false })
  })

  it('keeps installed true for a non-spawn failure', async () => {
    const { probe, run } = createHarness()
    run.mockRejectedValueOnce(new Error('gh exec timed out'))
    await expect(probe()).resolves.toEqual({ installed: true, authenticated: false })
  })

  it('caches the readiness answer for 60s', async () => {
    const { probe, run, advance } = createHarness()
    await probe()
    advance(59_000)
    await expect(probe()).resolves.toEqual({ installed: true, authenticated: true })
    expect(run).toHaveBeenCalledTimes(1)

    advance(2_000)
    await probe()
    expect(run).toHaveBeenCalledTimes(2)
  })
})
