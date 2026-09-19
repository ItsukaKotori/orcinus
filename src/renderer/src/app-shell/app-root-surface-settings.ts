import type { AppState } from '../store/types'

type AppRootSurfaceSettingsState = Pick<AppState, 'settings'>

export function selectAppRootSurfaceTelemetryOptedIn(
  state: AppRootSurfaceSettingsState
): boolean | 'unknown' {
  return state.settings?.telemetry?.optedIn ?? 'unknown'
}
