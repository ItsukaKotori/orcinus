import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { recordTerminalWebglDiagnostic } from '../../../../shared/terminal-webgl-diagnostics'
import {
  getTerminalFreezeBreadcrumbs,
  resetTerminalFreezeBreadcrumbsForTesting
} from './terminal-freeze-breadcrumbs'

// Why: lib-layer WebGL code records through the shared sink because it may not
// import the components-layer ring. Importing terminal-freeze-breadcrumbs wires
// that sink to the ring at module load; this pins that the WebGL crumbs land in
// the same one-paste report as delivery/visibility history.
describe('WebGL diagnostics → freeze breadcrumb ring', () => {
  beforeEach(() => {
    resetTerminalFreezeBreadcrumbsForTesting()
  })

  afterEach(() => {
    resetTerminalFreezeBreadcrumbsForTesting()
  })

  it('routes context-loss and atlas-reset crumbs into the freeze report ring', () => {
    recordTerminalWebglDiagnostic('webgl-context-loss', { paneId: 3 })
    recordTerminalWebglDiagnostic('webgl-atlas-reset', { managers: 1 })

    const crumbs = getTerminalFreezeBreadcrumbs()
    expect(crumbs.map((crumb) => crumb.kind)).toEqual(['webgl-context-loss', 'webgl-atlas-reset'])
    expect(crumbs[0]?.detail).toEqual({ paneId: 3 })
    expect(crumbs[1]?.detail).toEqual({ managers: 1 })
  })

  it('coalesces an atlas mismatch storm in the ring', () => {
    vi.useFakeTimers()
    try {
      for (let mismatch = 0; mismatch < 10_000; mismatch++) {
        recordTerminalWebglDiagnostic('atlas-font-probe-mismatch', {
          desired: '550',
          actual: '700 14px Menlo'
        })
      }

      expect(getTerminalFreezeBreadcrumbs()).toEqual([
        expect.objectContaining({ kind: 'atlas-font-probe-mismatch', repeats: 10_000 })
      ])

      vi.advanceTimersByTime(30_000)
      recordTerminalWebglDiagnostic('atlas-font-probe-mismatch', {
        desired: '550',
        actual: '700 14px Menlo'
      })
      // The coalesce window is 1s, so the post-storm record opens a fresh entry.
      const crumbs = getTerminalFreezeBreadcrumbs()
      expect(crumbs).toHaveLength(2)
      expect(crumbs[0]).toMatchObject({ kind: 'atlas-font-probe-mismatch', repeats: 10_000 })
      expect(crumbs[1]?.repeats).toBeUndefined()
    } finally {
      vi.useRealTimers()
    }
  })
})
