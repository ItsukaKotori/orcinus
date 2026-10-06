import type { ManagedPane } from './pane-manager-types'
import { recordTerminalWebglDiagnostic } from '../../../../shared/terminal-webgl-diagnostics'
import { replayPaintDebugLog } from '../../components/terminal-pane/replay-paint-debug-log'

/**
 * Why: when devicePixelRatio changes while a pane is hidden (window moved
 * between retina/non-retina displays, then the worktree revealed), xterm's
 * WebGL renderer re-measures cell dimensions but its canvas keeps the old
 * backing-store size — the addon's own device-pixel observer misses changes
 * that land while the element has no box. The browser then composites the
 * stale-scale bitmap into the css box: half/double-size or smeared text until
 * a real resize. Proven live: backing 2160 px for a 1080 css box at dpr 1.
 * The repair is xterm's own resize path, which recomputes device dimensions
 * at the current dpr and resizes the canvas backing store.
 */
type XtermRendererInternals = {
  _canvas?: HTMLCanvasElement
  _devicePixelRatio?: number
  _gl?: { canvas?: HTMLCanvasElement }
  dimensions?: {
    device?: { canvas?: { width?: number; height?: number } }
  }
  handleDevicePixelRatioChange?: () => void
  handleResize?: (cols: number, rows: number) => void
}

export type PaneWebglCanvasDprRepairState = 'current' | 'deferred' | 'repaired'

export function repairPaneWebglCanvasDpr(pane: ManagedPane): PaneWebglCanvasDprRepairState {
  const renderer = (
    pane.terminal as unknown as {
      _core?: { _renderService?: { _renderer?: { value?: XtermRendererInternals } } }
    }
  )._core?._renderService?._renderer?.value
  const canvas = renderer?._canvas ?? renderer?._gl?.canvas
  if (!renderer || !canvas) {
    return 'current'
  }
  if (!canvas.isConnected) {
    return 'deferred'
  }
  const view = canvas.ownerDocument?.defaultView
  const expected = renderer.dimensions?.device?.canvas
  const expectedWidth = expected?.width ?? 0
  const expectedHeight = expected?.height ?? 0
  if (!view || expectedWidth <= 0 || expectedHeight <= 0) {
    return 'deferred'
  }
  const staleBackingWidth = canvas.width
  const staleBackingHeight = canvas.height
  const cachedDevicePixelRatio = renderer._devicePixelRatio
  const devicePixelRatioChanged =
    typeof cachedDevicePixelRatio === 'number' && cachedDevicePixelRatio !== view.devicePixelRatio
  // xterm rounds its CSS canvas size before ResizeObserver converts it back to
  // device pixels; allow that round trip without forcing layout on every fit.
  const roundingTolerance = Math.max(1, Math.ceil(view.devicePixelRatio / 2))
  if (
    !devicePixelRatioChanged &&
    Math.abs(staleBackingWidth - expectedWidth) <= roundingTolerance &&
    Math.abs(staleBackingHeight - expectedHeight) <= roundingTolerance
  ) {
    return 'current'
  }
  try {
    // Order matters: refresh the renderer's cached dpr/dimensions first, then
    // the resize path recreates the backing store and layer sizes from them.
    renderer.handleDevicePixelRatioChange?.()
    renderer.handleResize?.(pane.terminal.cols, pane.terminal.rows)
    pane.terminal.refresh(0, pane.terminal.rows - 1)
  } catch {
    // Pane may be mid-teardown; the next reveal/fit retries the check.
    return 'deferred'
  }
  recordTerminalWebglDiagnostic('webgl-canvas-dpr-repair', {
    paneId: pane.id,
    staleBackingWidth,
    expectedBackingWidth: expectedWidth,
    ...(cachedDevicePixelRatio === undefined ? {} : { cachedDevicePixelRatio }),
    devicePixelRatio: view.devicePixelRatio
  })
  return 'repaired'
}

export function repairPaneWebglCanvasDprMismatch(pane: ManagedPane): boolean {
  return repairPaneWebglCanvasDpr(pane) === 'repaired'
}

/**
 * Forces the active renderer to re-run its resize path against the terminal's
 * CURRENT grid, then repaints every row.
 *
 * Why: a reload-restore can leave the renderer's canvas sized for a transient
 * mount-time grid (field: 14x1408 backing + 7px css for a 43x44 box) while the
 * buffer grid is already authoritative. Every later fit early-returns because
 * proposeDimensions matches, and the DPR repair above passes because canvas ==
 * renderer.dimensions — both compare against the same stale internal state, so
 * nothing ever re-anchors the renderer to `terminal.cols/rows`. Restore owns
 * the deterministic fix: xterm's own renderer resize recomputes dimensions from
 * the live grid, resizes the backing store, and clears the model; the caller's
 * present then paints the restored rows into a correctly sized canvas.
 */
export function forcePaneRendererResize(pane: ManagedPane): boolean {
  try {
    const renderer = (
      pane.terminal as unknown as {
        _core?: { _renderService?: { _renderer?: { value?: XtermRendererInternals } } }
      }
    )._core?._renderService?._renderer?.value
    if (!renderer || typeof renderer.handleResize !== 'function') {
      // DEBUG(replay-paint): remove after diagnosis.
      replayPaintDebugLog(
        `pane=${pane.id} forceRendererResize: renderer unavailable (hasRenderer=${Boolean(renderer)})`
      )
      return false
    }
    const canvasBefore =
      renderer._canvas?.width ?? renderer._gl?.canvas?.width ?? -1
    renderer.handleResize(pane.terminal.cols, pane.terminal.rows)
    pane.terminal.refresh(0, pane.terminal.rows - 1)
    const canvasAfter = renderer._canvas?.width ?? renderer._gl?.canvas?.width ?? -1
    // DEBUG(replay-paint): remove after diagnosis — did the forced resize re-anchor the canvas?
    replayPaintDebugLog(
      `pane=${pane.id} forceRendererResize: cols=${pane.terminal.cols} rows=${pane.terminal.rows} canvas ${canvasBefore}→${canvasAfter} expected=${String(renderer.dimensions?.device?.canvas?.width)}`
    )
    return true
  } catch (error) {
    // Pane may be mid-teardown; the caller's present still guards itself.
    replayPaintDebugLog(`forceRendererResize threw: ${error}`)
    return false
  }
}
