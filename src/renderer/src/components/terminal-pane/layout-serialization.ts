import type {
  TerminalLayoutSnapshot,
  TerminalPaneLayoutNode,
  TerminalPaneSplitDirection
} from '../../../../shared/terminal-tab-types'
import { isTerminalLeafId } from '../../../../shared/stable-pane-id'
import {
  POST_REPLAY_MODE_RESET,
  RESET_GRAPHIC_RENDITION
} from '../../../../shared/terminal-mode-reset-profiles'
import type { PaneManager } from '@/lib/pane-manager/pane-manager'
import type { ManagedPane } from '@/lib/pane-manager/pane-manager-types'
import { safeFit } from '@/lib/pane-manager/pane-tree-ops'
import { forcePaneRendererResize } from '@/lib/pane-manager/terminal-canvas-dpr-repair'
import { flushTerminalWriteBufferSync } from '@/lib/pane-manager/terminal-write-buffer-sync-flush'
import { isTerminalWriteBufferEmpty } from '@/lib/pane-manager/terminal-write-buffer-sync-flush'
import { presentPaneViewport } from '@/lib/pane-manager/pane-webgl-renderer'
import {
  replayIntoTerminal,
  waitForTerminalReplayWritesParsed,
  type ReplayingPanesRef
} from './replay-guard'
import type { RestoredViewportBlankingPanesRef } from './terminal-restored-viewport'
import { isXtermInstanceDisposed } from '@/lib/pane-manager/xterm-instance-disposed'

import {
  getLeftmostLeafId,
  normalizeTerminalLayoutSnapshot,
  resolveRootlessTerminalLayoutLeafId
} from './terminal-layout-leaf-ids'

export {
  collectLeafIdsInOrder,
  collectLeafIdsInReplayCreationOrder,
  normalizeTerminalLayoutSnapshot
} from './terminal-layout-leaf-ids'

export const EMPTY_LAYOUT: TerminalLayoutSnapshot = {
  root: null,
  activeLeafId: null,
  expandedLeafId: null
}

// Cross-platform monospace chain: browsers skip fonts absent on the current OS, so listing all is safe.
// Nerd Fonts come last to cover PUA glyphs (U+E000–U+F8FF) from OMP/Powerline that standard monospace fonts lack.
const FALLBACK_FONTS = [
  'SF Mono', // macOS 10.12+
  'Menlo', // macOS (older)
  'Monaco', // macOS (legacy)
  'Cascadia Mono', // Windows 11+
  'Consolas', // Windows Vista+
  'DejaVu Sans Mono', // Linux (common)
  'Liberation Mono', // Linux (common)
  'Orca Nerd Font Symbols', // bundled PUA fallback for OMP/Powerline glyphs
  'Symbols Nerd Font Mono', // purpose-built Nerd Fonts symbols-only fallback
  'MesloLGS Nerd Font', // p10k's recommended font; very common on zsh setups
  'JetBrainsMono Nerd Font', // widely installed; Ghostty ships a JBM-derived font
  'Hack Nerd Font', // common Nerd Font among Linux developers
  'monospace' // ultimate generic fallback
] as const

export function buildFontFamily(fontFamily: string): string {
  const trimmed = fontFamily.trim()
  const parts = trimmed ? [`"${trimmed}"`] : []
  const lowerParts = parts.map((p) => p.toLowerCase())
  // Append each fallback unless already present (case-insensitive) to avoid duplicates.
  for (const fallback of FALLBACK_FONTS) {
    const lower = fallback.toLowerCase()
    if (!lowerParts.some((p) => p.includes(lower))) {
      // Generic keywords like "monospace" are unquoted; named fonts are quoted.
      parts.push(fallback === 'monospace' ? fallback : `"${fallback}"`)
    }
  }
  return parts.join(', ')
}

export function getLayoutChildNodes(split: HTMLElement): HTMLElement[] {
  return Array.from(split.children).filter(
    (child): child is HTMLElement =>
      child instanceof HTMLElement &&
      (child.classList.contains('pane') || child.classList.contains('pane-split'))
  )
}

export function serializePaneTree(node: HTMLElement | null): TerminalPaneLayoutNode | null {
  if (!node) {
    return null
  }

  if (node.classList.contains('pane')) {
    const leafId = node.dataset.leafId
    if (!leafId || !isTerminalLeafId(leafId)) {
      return null
    }
    return { type: 'leaf', leafId }
  }

  if (!node.classList.contains('pane-split')) {
    return null
  }
  const [first, second] = getLayoutChildNodes(node)
  const firstNode = serializePaneTree(first ?? null)
  const secondNode = serializePaneTree(second ?? null)
  if (!firstNode || !secondNode) {
    return null
  }

  // Capture the flex ratio so resized panes survive serialization round-trips.
  let ratio: number | undefined
  if (first && second) {
    const firstGrow = Number.parseFloat(first.style.flex) || 1
    const secondGrow = Number.parseFloat(second.style.flex) || 1
    const total = firstGrow + secondGrow
    if (total > 0) {
      const r = firstGrow / total
      // Only store if meaningfully different from 0.5 (default equal split)
      if (Math.abs(r - 0.5) > 0.005) {
        ratio = Math.round(r * 1000) / 1000
      }
    }
  }

  return {
    type: 'split',
    direction: node.classList.contains('is-horizontal') ? 'horizontal' : 'vertical',
    first: firstNode,
    second: secondNode,
    ...(ratio !== undefined && { ratio })
  }
}

export function serializeTerminalLayout(
  root: HTMLDivElement | null,
  activePaneId: number | null,
  expandedPaneId: number | null,
  leafIdByPaneId?: ReadonlyMap<number, string>
): TerminalLayoutSnapshot {
  const rootNode = serializePaneTree(
    root?.firstElementChild instanceof HTMLElement ? root.firstElementChild : null
  )
  const activeLeafId = activePaneId === null ? null : leafIdByPaneId?.get(activePaneId)
  const expandedLeafId = expandedPaneId === null ? null : leafIdByPaneId?.get(expandedPaneId)
  return {
    root: rootNode,
    activeLeafId: activeLeafId && isTerminalLeafId(activeLeafId) ? activeLeafId : null,
    expandedLeafId: expandedLeafId && isTerminalLeafId(expandedLeafId) ? expandedLeafId : null
  }
}

// Why the restore needs its own paint pass: a mount-time restore replays into a
// pane whose WebGL renderer is already attached (openTerminal → attachWebgl), so
// the sync-viewport-refresh gate skips it, and every fit in the mount pipeline
// (initial-fit rAF, ResizeObserver first fire, queueResizeAll) runs BEFORE the
// replay bytes parse and reflows an empty grid — their full refreshes can never
// show the restored rows. The refreshes that do follow the parse are xterm's
// debounced ones, which nothing re-kicks on a WKWebView reload (no grid-changing
// resize, and no Electron-style post-reload focus/wake pass), so the buffer sits
// unpainted until a user resize. Waiting for the replay writes to parse and then
// running one settled-frame fit + full present makes the restored buffer the
// painted frame deterministically, and is a no-op repaint for panes whose
// renderer already showed them.

function scheduleRestoredReplayPaint(pane: ManagedPane): void {
  if (typeof requestAnimationFrame !== 'function') {
    return
  }
  const runSettledPaint = (): void => {
    requestAnimationFrame(() => {
      requestAnimationFrame(() => {
        try {
          // Why fit first: the mount-time fit can be skipped while the pane box
          // is still unmeasurable; the restored grid must be authoritative
          // before the present so the rows repaint at their final wrap.
          safeFit(pane)
          // Why force the renderer resize even when the fit early-returns: a
          // reload-restore can leave the renderer canvas sized for a transient
          // mount-time grid (field: 14x1408 backing for a 43-col box), and no
          // later fit re-anchors it because proposeDimensions already matches.
          forcePaneRendererResize(pane)
        } catch {
          // Pane may be disposed mid-restore; the present below guards itself.
        }
        presentPaneViewport(pane)
      })
    })
  }
  // Why the sync-drain fast path: when the flush above already parsed every
  // replay byte, the FIFO probe below would itself depend on the starved timer
  // queue — skip it and go straight to the settled-frame paint.
  if (isTerminalWriteBufferEmpty(pane.terminal)) {
    runSettledPaint()
    return
  }
  void waitForTerminalReplayWritesParsed(pane.terminal)
    .then(() => {
      runSettledPaint()
    })
    .catch(() => {
      // Restore paint is best-effort; a disposed terminal must not surface here.
    })
}

/**
 * Write saved scrollback buffers into restored panes so the user sees prior
 * output after a restart. Exits alt-screen first if a buffer ended mid-TUI.
 */
export function restoreScrollbackBuffers(
  manager: PaneManager,
  savedBuffers: Record<string, string> | undefined,
  restoredPaneByLeafId: Map<string, number>,
  replayingPanesRef: ReplayingPanesRef,
  restoredViewportBlankingPanesRef?: RestoredViewportBlankingPanesRef
): void {
  if (!savedBuffers) {
    return
  }
  const ALT_SCREEN_ON = '\x1b[?1049h'
  const ALT_SCREEN_OFF = '\x1b[?1049l'
  for (const [oldLeafId, buffer] of Object.entries(savedBuffers)) {
    const newPaneId = restoredPaneByLeafId.get(oldLeafId)
    if (newPaneId == null || !buffer) {
      continue
    }
    const pane = manager.getPanes().find((p) => p.id === newPaneId)
    if (!pane) {
      continue
    }
    // Breadcrumb: writes into a disposed xterm are silent (no throw), the suspected source of startup zombie panes.
    if (isXtermInstanceDisposed(pane.terminal)) {

      continue
    }
    try {
      const renderOptions = {
        shouldRefreshViewportSynchronously: () => !manager.hasWebglRenderer(pane.id)
      }
      let buf = buffer
      // If the buffer ends in alt-screen (agent TUI at shutdown), exit it so the terminal is usable.
      const lastOn = buf.lastIndexOf(ALT_SCREEN_ON)
      const lastOff = buf.lastIndexOf(ALT_SCREEN_OFF)
      if (lastOn > lastOff) {
        buf = buf.slice(0, lastOn)
      }
      if (buf.length > 0) {
        // replayIntoTerminal: buffer queries (DA1/DECRQM/CPR) would auto-reply into the new shell's stdin. See replay-guard.ts.
        replayIntoTerminal(
          pane,
          replayingPanesRef,
          `${RESET_GRAPHIC_RENDITION}${buf}${RESET_GRAPHIC_RENDITION}\r\n`,
          renderOptions
        )
        // The grounded newline avoids both the prompt marker and background-color erase from the captured pen.
        // Clear mode bits the buffer replayed: the fresh shell has no TUI to consume them. See POST_REPLAY_MODE_RESET.
        replayIntoTerminal(pane, replayingPanesRef, POST_REPLAY_MODE_RESET, renderOptions)
        // Why: connection resolution runs after layout replay; only fresh-shell paths move these rows into scrollback.
        restoredViewportBlankingPanesRef?.current.add(pane.id)
        // Why: the replay must not depend on the page's timer queue. After a
        // webview reload the reloaded WKWebView page can leave xterm's
        // setTimeout-driven WriteBuffer loop starved for tens of seconds (IPC
        // and React lifecycle run; timer callbacks don't), so the replayed
        // rows sit unparsed and unpainted until user input takes xterm's
        // synchronous write fast path. Restore replay is a one-shot bounded
        // write: drain it synchronously (same primitive resize uses before
        // reflow) and let the fit/present below paint immediately.
        flushTerminalWriteBufferSync(pane.terminal)
        // Why: only panes that actually received replayed bytes need the deferred
        // paint; the fresh-spawn path (no buffer) must stay untouched.
        scheduleRestoredReplayPaint(pane)
      }
    } catch (error: unknown) {
      // Breadcrumb: this catch was silent while zombie panes went undiagnosed.

    }
  }
}

export function replayTerminalLayout(
  manager: PaneManager,
  snapshot: TerminalLayoutSnapshot | null | undefined,
  focusInitialPane: boolean
): Map<string, number> {
  const paneByLeafId = new Map<string, number>()

  const normalized = normalizeTerminalLayoutSnapshot(snapshot)
  snapshot = normalized.snapshot
  const initialLeafId = snapshot.root
    ? getLeftmostLeafId(snapshot.root)
    : (resolveRootlessTerminalLayoutLeafId(snapshot) ?? undefined)
  const initialPane = manager.createInitialPane({ focus: focusInitialPane, leafId: initialLeafId })
  if (!snapshot?.root) {
    paneByLeafId.set(initialPane.leafId, initialPane.id)
    return paneByLeafId
  }

  const restoreNode = (node: TerminalPaneLayoutNode, paneId: number): void => {
    if (node.type === 'leaf') {
      paneByLeafId.set(node.leafId, paneId)
      return
    }

    const createdPane = manager.splitPane(paneId, node.direction as TerminalPaneSplitDirection, {
      ratio: node.ratio,
      leafId: getLeftmostLeafId(node.second)
    })
    if (!createdPane) {
      restoreNode(node.first, paneId)
      return
    }

    restoreNode(node.first, paneId)
    restoreNode(node.second, createdPane.id)
  }

  restoreNode(snapshot.root, initialPane.id)
  return paneByLeafId
}
