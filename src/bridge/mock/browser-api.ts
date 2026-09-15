// Phase 0 mock; replaced by Tauri IPC per view.
import type { PreloadApi } from '../../preload/api-types'
import { withMethodFallback } from '../unimplemented-fallback'
import { noopUnsubscribe } from './noop-unsubscribe'

export function createBrowserApi(): PreloadApi['browser'] {
  // Boot-listener methods used by always-mounted dialogs and app-shell bridges; the rest of the
  // namespace rejects through withMethodFallback until browser panes land.
  return withMethodFallback<PreloadApi['browser']>('browser', {
    sessionListProfiles: async () => [],
    onClientPageRendererRequest: () => noopUnsubscribe,
    onWebAuthnAccountRequest: () => noopUnsubscribe,
    onWebAuthnAccountRequestClosed: () => noopUnsubscribe,
    onGuestLoadFailed: () => noopUnsubscribe,
    onCertificateFailureChanged: () => noopUnsubscribe,
    onNavigationUpdate: () => noopUnsubscribe,
    onActivateView: () => noopUnsubscribe,
    onPaneFocus: () => noopUnsubscribe,
    onOpenLinkInOrcaTab: () => noopUnsubscribe,
    respondWebAuthnAccount: async () => false
  })
}
