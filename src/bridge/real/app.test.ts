import { invoke } from '@tauri-apps/api/core'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import type { AppApi } from '../../shared/preload-api/api/app-api'
import { UnimplementedBridgeError } from '../unimplemented-fallback'
import { createAppRealApi } from './app'

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }))

const invokeMock = vi.mocked(invoke)

type AppMethod = keyof AppApi

beforeEach(() => {
  invokeMock.mockReset()
})

describe('app real adapter commands', () => {
  it('maps getIdentity to app_get_identity without a payload', async () => {
    invokeMock.mockResolvedValueOnce({
      name: 'Orcinus',
      version: '0.0.1',
      isDev: false,
      devLabel: null,
      devBranch: null,
      devWorktreeName: null,
      devRepoRoot: null,
      dockBadgeLabel: null
    })

    const identity = await createAppRealApi().getIdentity()

    expect(invokeMock).toHaveBeenCalledWith('app_get_identity')
    expect(identity).toMatchObject({ name: 'Orcinus', isDev: false })
    // Why: the native identity is a superset of the renderer contract and carries the crate version.
    expect(identity).toHaveProperty('version', '0.0.1')
  })

  it('maps a {message} rejection to a normal Error', async () => {
    invokeMock.mockRejectedValueOnce({ message: 'identity unavailable' })
    await expect(createAppRealApi().getIdentity()).rejects.toBeInstanceOf(Error)
    invokeMock.mockRejectedValueOnce({ message: 'identity unavailable' })
    await expect(createAppRealApi().getIdentity()).rejects.toThrow('identity unavailable')
  })
})

describe('app real adapter unimplemented surface', () => {
  it.each([
    'relaunch',
    'restart',
    'reload',
    'awaitFirstWindowStartupServices',
    'getFloatingTerminalCwd',
    'onKeyboardLayoutChanged'
  ] satisfies AppMethod[])('rejects %s with UnimplementedBridgeError', async (method) => {
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => {})
    const app = createAppRealApi() as unknown as Record<
      string,
      (callArgs?: unknown) => Promise<unknown>
    >
    await expect(app[method]({})).rejects.toBeInstanceOf(UnimplementedBridgeError)
    expect(invokeMock).not.toHaveBeenCalled()
    warn.mockRestore()
  })
})
