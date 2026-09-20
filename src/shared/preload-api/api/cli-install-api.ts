import type { CliInstallStatus } from '../../cli-install-types'

export type CliApi = {
  getInstallStatus: () => Promise<CliInstallStatus>
  install: () => Promise<CliInstallStatus>
  remove: () => Promise<CliInstallStatus>
  getWslInstallStatus: (args?: { distro?: string | null }) => Promise<CliInstallStatus>
  installWsl: (args?: { distro?: string | null }) => Promise<CliInstallStatus>
  removeWsl: (args?: { distro?: string | null }) => Promise<CliInstallStatus>
}
