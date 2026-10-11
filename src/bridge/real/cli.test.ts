import { invoke } from '@tauri-apps/api/core'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import type { CliInstallStatus } from '../../shared/cli-install-types'
import { createCliRealApi } from './cli'

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }))

const invokeMock = vi.mocked(invoke)

const UNSUPPORTED: CliInstallStatus = {
  platform: 'darwin',
  commandName: 'orcinus',
  commandPath: null,
  pathDirectory: null,
  pathConfigured: false,
  launcherPath: null,
  installMethod: null,
  supported: false,
  state: 'unsupported',
  currentTarget: null,
  unsupportedReason: 'platform_not_supported',
  detail: 'CLI registration is not implemented on this platform.'
}

beforeEach(() => {
  invokeMock.mockReset()
})

describe('cli real adapter', () => {
  it('routes the desktop triple to its bare commands', async () => {
    invokeMock.mockResolvedValue(UNSUPPORTED)
    const api = createCliRealApi()

    await expect(api.getInstallStatus()).resolves.toEqual(UNSUPPORTED)
    expect(invokeMock).toHaveBeenLastCalledWith('cli_get_install_status')

    await expect(api.install()).resolves.toEqual(UNSUPPORTED)
    expect(invokeMock).toHaveBeenLastCalledWith('cli_install')

    await expect(api.remove()).resolves.toEqual(UNSUPPORTED)
    expect(invokeMock).toHaveBeenLastCalledWith('cli_remove')
  })

  it('passes the distro through the args envelope and omits it when absent', async () => {
    invokeMock.mockResolvedValue(UNSUPPORTED)
    const api = createCliRealApi()

    await api.getWslInstallStatus({ distro: 'Ubuntu' })
    expect(invokeMock).toHaveBeenLastCalledWith('cli_get_wsl_install_status', {
      args: { distro: 'Ubuntu' }
    })

    await api.installWsl()
    expect(invokeMock).toHaveBeenLastCalledWith('cli_install_wsl', { args: undefined })

    await api.removeWsl({ distro: null })
    expect(invokeMock).toHaveBeenLastCalledWith('cli_remove_wsl', { args: { distro: null } })
  })

  it('surfaces command failures as Error instances', async () => {
    invokeMock.mockRejectedValueOnce({ message: 'boom' })
    await expect(createCliRealApi().install()).rejects.toThrow('boom')
  })
})
