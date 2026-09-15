// Phase 0 mock; replaced by Tauri IPC per view.
import type { PreloadApi } from '../../preload/api-types'
import { getKeybindingPlatform } from '../../shared/keybindings'
import type { KeybindingFileSnapshot } from '../../shared/keybindings'
import { withMethodFallback } from '../unimplemented-fallback'
import { noopUnsubscribe } from './noop-unsubscribe'
import { detectBrowserPlatform } from './platform-api'

export function createKeybindingsApi(): PreloadApi['keybindings'] {
  // Why exists:false: no user keybinding file backs the Phase 0 shell, so the settings surface
  // shows the default-bindings state instead of reporting a missing-file error.
  const emptySnapshot = (): KeybindingFileSnapshot => ({
    path: '',
    platform: getKeybindingPlatform(detectBrowserPlatform()),
    exists: false,
    overrides: {},
    commonOverrides: {},
    platformOverrides: {},
    diagnostics: []
  })
  return withMethodFallback<PreloadApi['keybindings']>('keybindings', {
    get: async () => emptySnapshot(),
    onChanged: () => noopUnsubscribe
  })
}
