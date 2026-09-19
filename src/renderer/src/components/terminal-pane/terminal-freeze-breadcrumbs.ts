// Renderer half of the one-paste freeze report: a bounded ring of the
// delivery-affecting transitions (gate marks, visibility trust changes,
// watchdog heals, restore markers) so a field report carries the history
// that led to the frozen state, not just a point-in-time counter snapshot.

import {
  type PtyDeliveryBreadcrumb,
  createPtyDeliveryBreadcrumbRing
} from '../../../../shared/pty-delivery-diagnostics'
import { recordTerminalWebglDiagnostic, setTerminalWebglDiagnosticRecorder } from '../../../../shared/terminal-webgl-diagnostics'
import { maybeStartTerminalRenderDesyncSentinel } from './terminal-render-desync-trigger'

const rendererDeliveryBreadcrumbs = createPtyDeliveryBreadcrumbRing()
const ATLAS_FONT_PROBE_MISMATCH = 'atlas-font-probe-mismatch'

export function recordTerminalFreezeBreadcrumb(
  kind: string,
  detail?: PtyDeliveryBreadcrumb['detail']
): void {
  rendererDeliveryBreadcrumbs.record(kind, detail)
}

// Why: lib-layer WebGL code (pane-webgl-renderer, the atlas registry) can't
// import this components-layer ring directly, so it records through a shared
// sink. Point that sink at the same ring here so context-loss and atlas-reset
// crumbs land in the one-paste report alongside delivery/visibility history.
setTerminalWebglDiagnosticRecorder((kind, detail) => {
  rendererDeliveryBreadcrumbs.record(kind, detail)
})

// Why: the sentinel is a field-diagnostic that must be armable on production
// builds; starting it from this diagnostics bootstrap keeps arming independent
// of any specific pane mounting first. No-op unless its localStorage flag is set.
maybeStartTerminalRenderDesyncSentinel()

// Sink for the patched @xterm/addon-webgl atlas font probe: the atlas cannot
// import Orca code, so it reports failed ctx.font assignments (the stuck-
// rasterizer arm of the bold-collapse family) through this global. Crumbs are
// coalesced upstream, so a rasterization storm cannot flood the report.
type AtlasFontProbeMismatch = { desired?: string; actual?: string }
;(globalThis as { __orcaAtlasFontProbe?: (mismatch: AtlasFontProbeMismatch) => void })[
  '__orcaAtlasFontProbe'
] = (mismatch) => {
  recordTerminalWebglDiagnostic(ATLAS_FONT_PROBE_MISMATCH, {
    desired: mismatch?.desired ?? null,
    actual: mismatch?.actual ?? null
  })
}

export function getTerminalFreezeBreadcrumbs(): PtyDeliveryBreadcrumb[] {
  return rendererDeliveryBreadcrumbs.snapshot()
}

export function resetTerminalFreezeBreadcrumbsForTesting(): void {
  rendererDeliveryBreadcrumbs.reset()
}
