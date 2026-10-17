import { useCallback, useRef, type RefObject } from 'react'
import { installTabDragMissedEndListeners } from './tab-drag-missed-end-listeners'

/** Global side effects a tab drag holds while in flight: the window-level
 *  fallback for a missed drag end (plus a no-op webview passthrough slot kept
 *  for the drag-state machine's contract). Both refs are injected so this stays
 *  independent of the drag state machine. */
export function useTabDragGestureLifecycle({
  clearDragStateRef,
  tabDragActiveRef
}: {
  clearDragStateRef: RefObject<() => void>
  tabDragActiveRef: RefObject<boolean>
}): {
  acquireWebviewDragPassthrough: () => void
  installMissedEndFallback: () => void
  releaseMissedEndFallback: () => void
  releaseWebviewDragPassthrough: () => void
  setDragRootNode: (node: HTMLDivElement | null) => void
} {
  const releaseWebviewDragPassthroughRef = useRef<(() => void) | null>(null)
  const releaseMissedEndFallbackRef = useRef<(() => void) | null>(null)

  const releaseWebviewDragPassthrough = useCallback(() => {
    releaseWebviewDragPassthroughRef.current?.()
    releaseWebviewDragPassthroughRef.current = null
  }, [])

  const releaseMissedEndFallback = useCallback(() => {
    releaseMissedEndFallbackRef.current?.()
    releaseMissedEndFallbackRef.current = null
  }, [])

  const installMissedEndFallback = useCallback(() => {
    releaseMissedEndFallback()
    releaseMissedEndFallbackRef.current = installTabDragMissedEndListeners(() => {
      if (tabDragActiveRef.current) {
        clearDragStateRef.current()
      }
    })
  }, [clearDragStateRef, releaseMissedEndFallback, tabDragActiveRef])

  const acquireWebviewDragPassthrough = useCallback(() => {
    // Why no-op: this existed to put browser <webview> guests in pointer passthrough
    // during drags; with the embedded browser gone there is nothing to passthrough.
  }, [])

  const setDragRootNode = useCallback(
    (node: HTMLDivElement | null): void => {
      if (node) {
        return
      }
      // Why: this root owns the dnd-kit gesture that installs global fallback listeners.
      releaseWebviewDragPassthrough()
      releaseMissedEndFallback()
    },
    [releaseMissedEndFallback, releaseWebviewDragPassthrough]
  )

  return {
    acquireWebviewDragPassthrough,
    installMissedEndFallback,
    releaseMissedEndFallback,
    releaseWebviewDragPassthrough,
    setDragRootNode
  }
}
