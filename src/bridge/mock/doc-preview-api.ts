// Phase 0 mock; replaced by Tauri IPC per view.
import type { PreloadApi } from '../../preload/api-types'

export function createDocPreviewApi(): PreloadApi['docPreview'] {
  // DocPreviewExternalLinkConfirmation is always mounted and subscribes at boot; grants and
  // authorization report refusal so no preview can claim a capability the mock does not have.
  return {
    mintGrant: async () => ({ grantId: 'mock-grant', url: 'about:blank' }),
    revokeGrant: async () => false,
    authorizeDirectory: async () => false,
    onExternalLink: () => () => {},
    onLoadFailure: () => () => {}
  }
}
