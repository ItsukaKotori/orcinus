export type TerminalBufferCaptureSchedulerDeps = {
  captureAll: () => void
  isDocumentHidden: () => boolean
  intervalMs?: number
}

/**
 * R3 (spec §5.3): a coarse crash-loss floor, not the primary durability path —
 * graceful quit goes through the host-driven flush (§3.4). Periodic full
 * re-serialize was removed upstream for main-thread stalls (#461); the 60s
 * interval matches the existing resume-capture cadence and skips while hidden.
 */
export function createTerminalBufferCaptureScheduler(
  deps: TerminalBufferCaptureSchedulerDeps
): () => void {
  const intervalMs = deps.intervalMs ?? 60_000
  const captureIfVisible = (): void => {
    if (!deps.isDocumentHidden()) {
      deps.captureAll()
    }
  }
  const onVisibilityChange = (): void => {
    if (deps.isDocumentHidden()) {
      deps.captureAll()
    }
  }
  document.addEventListener('visibilitychange', onVisibilityChange)
  const timer = window.setInterval(captureIfVisible, intervalMs)
  return () => {
    window.clearInterval(timer)
    document.removeEventListener('visibilitychange', onVisibilityChange)
  }
}
