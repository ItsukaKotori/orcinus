// Phase 0 mock; replaced by Tauri IPC per view.
import type { PreloadApi } from '../../preload/api-types'
import type { CliInstallStatus } from '../../shared/cli-install-types'
import { withUnimplementedFallback } from '../unimplemented-fallback'

const MOCK_CLI_STATUS: CliInstallStatus = {
  platform: 'win32',
  commandName: 'ade',
  commandPath: null,
  pathDirectory: null,
  pathConfigured: false,
  launcherPath: null,
  installMethod: null,
  supported: false,
  state: 'not_installed',
  currentTarget: null,
  unsupportedReason: null,
  detail: null
}

export function createCliApi(): PreloadApi['cli'] {
  return withUnimplementedFallback<PreloadApi['cli']>({
    getInstallStatus: async () => MOCK_CLI_STATUS
  })
}
