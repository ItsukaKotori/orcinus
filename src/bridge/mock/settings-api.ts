// Phase 0 mock; replaced by Tauri IPC per view.
import type { SettingsApi } from '../../preload/api/settings-api'
import { getDefaultSettings } from '../../shared/constants'
import type { GlobalSettings } from '../../shared/global-settings-types'

const MOCK_HOME = 'C:\\Users\\ade'

export function createSettingsApi(): SettingsApi {
  let state: GlobalSettings = {
    ...getDefaultSettings(MOCK_HOME),
    // Simplified groups from spec 6.4; only 插件/遥测 carry explicit mock values.
    pluginSystemEnabled: true,
    telemetry: {
      optedIn: false,
      installId: 'mock-install-id',
      existedBeforeTelemetryRelease: true
    }
  }
  return {
    get: async () => state,
    getSync: () => state,
    set: async (args) => {
      state = { ...state, ...args }
      return state
    },
    setActiveRuntimeEnvironmentPreference: async ({ environmentId }) => {
      state = { ...state, activeRuntimeEnvironmentId: environmentId }
      return state
    },
    updatePRBotAuthorOverride: async ({ author, isBot }) => {
      const normalized = author.trim().toLowerCase()
      const others = state.prBotAuthorOverrides.filter((entry) => entry !== normalized)
      state = {
        ...state,
        prBotAuthorOverrides: isBot ? [...others, normalized] : others
      }
      return state
    },
    listFonts: async () => [
      'Cascadia Mono',
      'Consolas',
      'Courier New',
      'DejaVu Sans Mono',
      'Fira Code',
      'JetBrains Mono',
      'SF Mono'
    ],
    previewGhosttyImport: async () => ({ found: false, diff: {}, unsupportedKeys: [] }),
    previewWarpThemeImport: async () => ({ found: false, themes: [], skippedFiles: [] }),
    onChanged: (_callback) => () => {}
  }
}
