import { invoke } from '@tauri-apps/api/core'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import type {
  ComputerUsePermissionResetResult,
  ComputerUsePermissionStatusResult
} from '../../shared/computer-use-permissions-types'
import { createComputerUsePermissionsRealApi } from './computer-use-permissions'

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }))

const invokeMock = vi.mocked(invoke)

const STATUS: ComputerUsePermissionStatusResult = {
  platform: 'darwin',
  helperAppPath: '/Applications/Orcinus Computer Use.app',
  helperUnavailableReason: null,
  permissions: [
    { id: 'accessibility', status: 'granted' },
    { id: 'screenshots', status: 'not-granted' }
  ]
}

beforeEach(() => {
  invokeMock.mockReset()
})

describe('computerUsePermissions real adapter', () => {
  it('routes getStatus and reset to their bare commands', async () => {
    invokeMock.mockResolvedValueOnce(STATUS)
    await expect(createComputerUsePermissionsRealApi().getStatus()).resolves.toEqual(STATUS)
    expect(invokeMock).toHaveBeenCalledWith('computer_use_permissions_get_status')

    const reset: ComputerUsePermissionResetResult = {
      ...STATUS,
      bundleId: 'dev.itsuka.orcinus.computer-use'
    }
    invokeMock.mockResolvedValueOnce(reset)
    await expect(createComputerUsePermissionsRealApi().reset()).resolves.toEqual(reset)
    expect(invokeMock).toHaveBeenCalledWith('computer_use_permissions_reset')
  })

  it('passes the requested id through the args envelope and omits it when absent', async () => {
    invokeMock.mockResolvedValue({
      platform: 'darwin',
      helperAppPath: null,
      openedSettings: false,
      launchedHelper: false,
      nextStep: null
    })
    await createComputerUsePermissionsRealApi().openSetup({ id: 'accessibility' })
    expect(invokeMock).toHaveBeenCalledWith('computer_use_permissions_open_setup', {
      args: { id: 'accessibility' }
    })

    invokeMock.mockClear()
    await createComputerUsePermissionsRealApi().openSetup()
    expect(invokeMock).toHaveBeenCalledWith('computer_use_permissions_open_setup', {
      args: undefined
    })
  })

  it('surfaces command failures as Error instances', async () => {
    invokeMock.mockRejectedValueOnce({ message: 'boom' })
    await expect(createComputerUsePermissionsRealApi().getStatus()).rejects.toThrow('boom')
  })
})
