import type { CliInstallStatus } from '../../shared/cli-install-types'
import type { CliApi } from '../../shared/preload-api/api/cli-install-api'
import { withMethodFallback } from '../unimplemented-fallback'
import { invokeCommand } from './invoke'

/**
 * Real `cli` adapter (spec §3.8): symlink PATH registration for the `orcinus`
 * launcher. The three WSL methods are unsupported stubs on this host and
 * answer the same `platform_not_supported` shape as a non-macOS desktop.
 */
export function createCliRealApi(): CliApi {
  return withMethodFallback<CliApi>('cli', {
    getInstallStatus: () => invokeCommand<CliInstallStatus>('cli_get_install_status'),
    install: () => invokeCommand<CliInstallStatus>('cli_install'),
    remove: () => invokeCommand<CliInstallStatus>('cli_remove'),
    getWslInstallStatus: (args?: { distro?: string | null }) =>
      invokeCommand<CliInstallStatus>('cli_get_wsl_install_status', { args }),
    installWsl: (args?: { distro?: string | null }) =>
      invokeCommand<CliInstallStatus>('cli_install_wsl', { args }),
    removeWsl: (args?: { distro?: string | null }) =>
      invokeCommand<CliInstallStatus>('cli_remove_wsl', { args })
  })
}
