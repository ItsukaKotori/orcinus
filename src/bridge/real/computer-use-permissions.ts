import type {
  ComputerUsePermissionId,
  ComputerUsePermissionResetResult,
  ComputerUsePermissionSetupResult,
  ComputerUsePermissionStatusResult
} from '../../shared/computer-use-permissions-types'
import type { PreloadApi } from '../../shared/preload-api/api-types'
import { withMethodFallback } from '../unimplemented-fallback'
import { invokeCommand } from './invoke'

/**
 * Real `computerUsePermissions` adapter (spec §3.4): the three host-backed
 * methods route to the `computer_use_permissions_*` commands.
 *
 * The command path records no feature interaction — the renderer's
 * `openSetup` already records `computer-use-setup` itself, and RPC-originated
 * setup calls are recorded inside the runtime server. Optional fields are
 * omitted, never materialized as null.
 */
export function createComputerUsePermissionsRealApi(): PreloadApi['computerUsePermissions'] {
  return withMethodFallback<PreloadApi['computerUsePermissions']>('computerUsePermissions', {
    getStatus: () =>
      invokeCommand<ComputerUsePermissionStatusResult>('computer_use_permissions_get_status'),
    openSetup: (args?: { id?: ComputerUsePermissionId }) =>
      invokeCommand<ComputerUsePermissionSetupResult>('computer_use_permissions_open_setup', {
        args
      }),
    reset: () =>
      invokeCommand<ComputerUsePermissionResetResult>('computer_use_permissions_reset')
  })
}
