// Phase 0 mock; replaced by Tauri IPC per view.
import type { PreloadApi } from '../../preload/api-types'
import { UnimplementedBridgeError } from '../unimplemented-fallback'

export function createDocPreviewApi(): PreloadApi['docPreview'] {
  // DocPreviewExternalLinkConfirmation is always mounted and subscribes at boot; mock 不发放
  // grant，预览能力未实现前一律拒绝，authorization 同样报告拒绝。
  return {
    mintGrant: async () => {
      throw new UnimplementedBridgeError('docPreview.mintGrant')
    },
    revokeGrant: async () => false,
    authorizeDirectory: async () => false,
    onExternalLink: () => () => {},
    onLoadFailure: () => () => {}
  }
}
