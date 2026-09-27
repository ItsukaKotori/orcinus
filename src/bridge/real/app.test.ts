import { invoke } from '@tauri-apps/api/core'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { createAppRealApi } from './app'

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }))

const invokeMock = vi.mocked(invoke)

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

describe('app real adapter host semantics', () => {
  it('resolves the startup barriers without a ported host service', async () => {
    // Regression: an unguarded `awaitGitEnvironmentStartupBarrier()` rejection
    // aborted the whole renderer hydration chain, so real mode restored no
    // worktrees and no session.
    const app = createAppRealApi()
    await expect(app.awaitGitEnvironmentStartupBarrier()).resolves.toBeUndefined()
    await expect(app.awaitFirstWindowStartupServices()).resolves.toBeUndefined()
    await expect(app.prepareTerminalStartupRestoration()).resolves.toBeUndefined()
    await expect(app.recoverLegacyWorkerTerminalsForRendererStartup()).resolves.toBeUndefined()
    expect(app.stageBeforeUnloadSync({ sessions: [], ui: {} })).toBeUndefined()
    expect(invokeMock).not.toHaveBeenCalled()
  })

  it('keeps the remaining unported methods benign instead of fabricating rejections', async () => {
    const app = createAppRealApi()
    await expect(app.relaunch()).resolves.toBeUndefined()
    await expect(app.getFloatingTerminalCwd()).resolves.toEqual(expect.any(String))
    expect(typeof app.onKeyboardLayoutChanged(() => {})).toBe('function')
    expect(invokeMock).not.toHaveBeenCalled()
  })
})
