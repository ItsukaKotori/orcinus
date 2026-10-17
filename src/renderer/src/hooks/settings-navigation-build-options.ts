import type { Repo } from '../../../shared/repo-types'

export type SettingsNavigationBuildOptions = {
  isMac: boolean
  isWindows: boolean
  isLocalWindowsHost: boolean
  isWindowsTerminalHost: boolean
  isWebClient: boolean
  isDev: boolean
  repos: readonly Repo[]
}
