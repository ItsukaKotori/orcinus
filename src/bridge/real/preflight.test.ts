import { invoke } from '@tauri-apps/api/core'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import type { RefreshAgentsResult } from '../../shared/preload-api/api/preflight-api'
import { createPreflightRealApi } from './preflight'

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }))

const invokeMock = vi.mocked(invoke)

beforeEach(() => {
  invokeMock.mockReset()
})

describe('preflight real adapter', () => {
  it('probes git availability from the repos machine probe', async () => {
    invokeMock.mockImplementation(async (command: string) => {
      if (command === 'repos_is_git_available') return true
      throw new Error(`unexpected ${command}`)
    })
    await expect(createPreflightRealApi().check()).resolves.toEqual({
      git: { installed: true }
    })
    expect(invokeMock).toHaveBeenCalledWith('repos_is_git_available')
  })

  it('reports git missing when the probe cannot run', async () => {
    invokeMock.mockImplementation(async (command: string) => {
      if (command === 'repos_is_git_available') return false
      throw new Error(`unexpected ${command}`)
    })
    await expect(createPreflightRealApi().check()).resolves.toEqual({
      git: { installed: false }
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
