import { invoke } from '@tauri-apps/api/core'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { RefreshAgentsResult } from '../../shared/preload-api/api/preflight-api'
import { createPreflightRealApi } from './preflight'

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }))

const invokeMock = vi.mocked(invoke)

const AUTH_STATUS = [
  'github.com',
  '  ✓ Logged in to github.com account alice (keyring)',
  '  - Active account: true',
  "  - Token scopes: 'project', 'read:org', 'repo'",
  ''
].join('\n')

// Why: the gh readiness probe is a module-level singleton with a 60s cache (the
// production wiring), so every test starts one hour after the previous one.
let clock = Date.parse('2026-01-01T00:00:00Z')

beforeEach(() => {
  clock += 60 * 60_000
  vi.useFakeTimers()
  vi.setSystemTime(clock)
  invokeMock.mockReset()
})

afterEach(() => {
  vi.useRealTimers()
})

describe('preflight real adapter', () => {
  it('probes git and reports gh installed+authenticated from gh auth status', async () => {
    invokeMock.mockImplementation(async (command: string) => {
      if (command === 'repos_is_git_available') return true
      if (command === 'gh_exec') return { stdout: AUTH_STATUS, stderr: '', code: 0 }
      throw new Error(`unexpected ${command}`)
    })
    await expect(createPreflightRealApi().check()).resolves.toEqual({
      git: { installed: true },
      gh: { installed: true, authenticated: true }
    })
    expect(invokeMock).toHaveBeenCalledWith('repos_is_git_available')
    expect(invokeMock).toHaveBeenCalledWith('gh_exec', { args: { args: ['auth', 'status'] } })
  })

  it('reports git missing and gh missing when the probes cannot spawn', async () => {
    invokeMock.mockImplementation(async (command: string) => {
      if (command === 'repos_is_git_available') return false
      if (command === 'gh_exec') throw new Error('gh: command not found on PATH')
      throw new Error(`unexpected ${command}`)
    })
    await expect(createPreflightRealApi().check()).resolves.toEqual({
      git: { installed: false },
      gh: { installed: false, authenticated: false }
    })
  })

  it('reports gh installed but unauthenticated when logged out', async () => {
    invokeMock.mockImplementation(async (command: string) => {
      if (command === 'repos_is_git_available') return true
      if (command === 'gh_exec') {
        return { stdout: '', stderr: 'You are not logged into any GitHub hosts.', code: 1 }
      }
      throw new Error(`unexpected ${command}`)
    })
    await expect(createPreflightRealApi().check()).resolves.toEqual({
      git: { installed: true },
      gh: { installed: true, authenticated: false }
    })
  })

  it('refreshes agents through preflight_refresh_agents, passing the runtime context through', async () => {
    const result: RefreshAgentsResult = {
      agents: ['claude'],
      addedPathSegments: ['/opt/cli/bin'],
      shellHydrationOk: true,
      pathSource: 'shell_hydrate',
      pathFailureReason: 'none'
    }
    invokeMock.mockResolvedValueOnce(result)
    const args = { wslDistro: null }
    await expect(createPreflightRealApi().refreshAgents(args)).resolves.toEqual(result)
    expect(invokeMock).toHaveBeenCalledWith('preflight_refresh_agents', { args })
  })

  it('refreshes agents without a runtime context when the caller omits args', async () => {
    invokeMock.mockResolvedValueOnce({
      agents: [],
      addedPathSegments: [],
      shellHydrationOk: false,
      pathSource: 'sync_seed_only',
      pathFailureReason: 'no_shell'
    })
    await expect(createPreflightRealApi().refreshAgents()).resolves.toMatchObject({
      pathSource: 'sync_seed_only'
    })
    expect(invokeMock).toHaveBeenCalledWith('preflight_refresh_agents', { args: undefined })
  })

  it('detects agents through preflight_refresh_agents and extracts only the agent list', async () => {
    const result: RefreshAgentsResult = {
      agents: ['claude', 'codex'],
      addedPathSegments: ['/opt/cli/bin'],
      shellHydrationOk: true,
      pathSource: 'shell_hydrate',
      pathFailureReason: 'none'
    }
    invokeMock.mockResolvedValueOnce(result)
    const args = { wslDistro: null }
    await expect(createPreflightRealApi().detectAgents(args)).resolves.toEqual(['claude', 'codex'])
    expect(invokeMock).toHaveBeenCalledWith('preflight_refresh_agents', { args })
  })
})
