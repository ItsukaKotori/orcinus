import { shutdownBufferCaptures } from '../components/terminal-pane/shutdown-buffer-captures'

/**
 * Serialize every mounted tab's buffers into the store (spec §4.1 triggers B/C).
 * Default capture options keep local buffers — ade has no daemon, so the
 * renderer capture is the only durable scrollback copy (spec R2).
 */
export function captureAllMountedTabBuffers(): void {
  for (const capture of shutdownBufferCaptures.values()) {
    try {
      capture()
    } catch {
      // One pane's serialization failure must not block the rest.
    }
  }
}
