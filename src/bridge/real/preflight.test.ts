import { invoke } from '@tauri-apps/api/core'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { UnimplementedBridgeError } from '../unimplemented-fallback'
import { createPreflightRealApi } from './preflight'

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }))

const invokeMock = vi.mocked(invoke)

beforeEach(() => {
  invokeMock.mockReset()
})

describe('preflight real adapter', () => {
  it('probes git through repos_is_git_available and reports gh as unavailable', async () => {
    invokeMock.mockResolvedValueOnce(true)
    await expect(createPreflightRealApi().check()).resolves.toEqual({
      git: { installed: true },
      gh: { installed: false, authenticated: false }
    })
    expect(invokeMock).toHaveBeenCalledWith('repos_is_git_available')
  })

  it('reports git as missing when the probe answers false', async () => {
    invokeMock.mockResolvedValueOnce(false)
    await expect(createPreflightRealApi().check()).resolves.toEqual({
      git: { installed: false },
      gh: { installed: false, authenticated: false }
    })
  })

  it('keeps refreshAgents on the loudly failing fallback', async () => {
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => {})
    await expect(createPreflightRealApi().refreshAgents()).rejects.toBeInstanceOf(
      UnimplementedBridgeError
    )
    expect(invokeMock).not.toHaveBeenCalled()
    warn.mockRestore()
  })
})
