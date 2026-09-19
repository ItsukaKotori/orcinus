/**
 * Cold-park verdict churn damping and a safe-side circuit breaker.
 * Burst damping keeps the pane mounted before React reaches #185.
 *
 * Scope: flips are counted on the rendered verdict and the pin subtracts from
 * that same verdict (selectParkVerdictPinnedTabIds), so churn driven by any
 * parked input — worktree-level park, portal ownership, deferred activation
 * mounts — is damped, not only the cold-park candidate set.
 */

/** React's own nested-update limit (#185 threshold) this damping stays under. */
const REACT_NESTED_UPDATE_LIMIT = 50

export const TERMINAL_TAB_PARK_FLIP_WINDOW_MS = 60_000

/** Measured upper bound after the passive-effect pin engages. */
const PARK_PIN_SETTLE_COMMITS = 6
/** Worst-case commits from pane, watcher, and store work per verdict flip. */
export const TERMINAL_TAB_PARK_FLIP_COMMIT_COST = 12
/** Pin threshold derived from React's remaining commit budget. */
export const TERMINAL_TAB_PARK_FLIP_BURST_LIMIT = Math.max(
  2,
  Math.floor(
    (REACT_NESTED_UPDATE_LIMIT - PARK_PIN_SETTLE_COMMITS) / TERMINAL_TAB_PARK_FLIP_COMMIT_COST
  )
)
/** Honest cold parking cannot round-trip inside this horizon. */
export const TERMINAL_TAB_PARK_FLIP_BURST_WINDOW_MS = 1_000

export type ParkVerdictFlipRecord = {
  parked: boolean
  windowStartMs: number
  burstStartMs: number
  burstFlips: number
  /** Set when a flip burst engaged damping; the verdict stays unparked until then. */
  pinnedUntilMs?: number | null
}

// Why it leaves pinnedUntilMs alone: the pin window is 60s from the first
// flip, so clearing here would release damping early.
function resetFlipWindows(record: ParkVerdictFlipRecord, nowMs: number): void {
  record.windowStartMs = nowMs
  record.burstStartMs = nowMs
  record.burstFlips = 0
}

// Why liveness and not presence: a tab pinned while cold-park-eligible can stop
// being a candidate before its deadline, and nothing would consult it again. An
// expired pin must stop damping and stop gating breadcrumbs on its own.
function isParkVerdictPinLive(record: ParkVerdictFlipRecord, nowMs: number): boolean {
  return record.pinnedUntilMs != null && nowMs < record.pinnedUntilMs
}

/** Returns the safe-side pin deadline and re-arms an expired window. */
export function getParkVerdictUnparkPinUntilMs(args: {
  records: Map<string, ParkVerdictFlipRecord>
  tabId: string
  nowMs: number
}): number | null {
  const record = args.records.get(args.tabId)
  if (record?.pinnedUntilMs == null) {
    return null
  }
  if (!isParkVerdictPinLive(record, args.nowMs) || args.nowMs < record.windowStartMs) {
    resetFlipWindows(record, args.nowMs)
    record.pinnedUntilMs = null
    return null
  }
  return record.pinnedUntilMs
}

/** Records park-verdict churn per tab and damps repeating bursts. */
export function recordParkVerdictFlips(args: {
  records: Map<string, ParkVerdictFlipRecord>
  liveTabIds: ReadonlySet<string>
  nextParkedTabIds: ReadonlySet<string>
  nowMs: number
  flipWindowMs?: number
  burstWindowMs?: number
  burstLimit?: number
}): void {
  const {
    records,
    liveTabIds,
    nextParkedTabIds,
    nowMs,
    flipWindowMs = TERMINAL_TAB_PARK_FLIP_WINDOW_MS,
    burstWindowMs = TERMINAL_TAB_PARK_FLIP_BURST_WINDOW_MS,
    burstLimit = TERMINAL_TAB_PARK_FLIP_BURST_LIMIT
  } = args

  for (const tabId of Array.from(records.keys())) {
    if (!liveTabIds.has(tabId)) {
      records.delete(tabId)
    }
  }

  for (const tabId of liveTabIds) {
    const parked = nextParkedTabIds.has(tabId)
    const record = records.get(tabId)

    if (!record) {
      records.set(tabId, {
        parked,
        windowStartMs: nowMs,
        burstStartMs: nowMs,
        burstFlips: 0,
        pinnedUntilMs: null
      })
      continue
    }
    if (parked === record.parked) {
      continue
    }

    // Why: Date.now() jumps backwards on NTP/sleep-wake; treat any out-of-range
    // elapsed value as a fresh window rather than trusting the delta.
    const elapsedMs = nowMs - record.windowStartMs
    if (elapsedMs >= flipWindowMs || elapsedMs < 0) {
      resetFlipWindows(record, nowMs)
    }
    const burstElapsedMs = nowMs - record.burstStartMs
    if (burstElapsedMs >= burstWindowMs || burstElapsedMs < 0) {
      record.burstStartMs = nowMs
      record.burstFlips = 0
    }

    record.parked = parked
    record.burstFlips += 1

    if (!isParkVerdictPinLive(record, nowMs) && record.burstFlips >= burstLimit) {
      record.pinnedUntilMs = nowMs + flipWindowMs
    }
  }
}

export type ParkVerdictPinSelection = {
  pinnedTabIds: Set<string>
  /** Earliest live pin deadline, so a caller can wake exactly when damping lapses. */
  earliestPinExpiryMs: number | null
}

/**
 * Tab ids a flip burst pinned unparked, expiring lapsed pins in place.
 *
 * Why every live tab and not only cold-park candidates: flips are counted on
 * the rendered verdict, so the oscillating input can be one the cold-park
 * selector never sees (worktree-level park, activation-deferred mounts). A pin
 * consulted only through the cold set then damps nothing and never lapses —
 * it just silences its own breadcrumb for the window and re-arms forever.
 */
export function selectParkVerdictPinnedTabIds(args: {
  records: Map<string, ParkVerdictFlipRecord>
  tabIds: Iterable<string>
  nowMs: number
}): ParkVerdictPinSelection {
  const pinnedTabIds = new Set<string>()
  let earliestPinExpiryMs: number | null = null
  for (const tabId of args.tabIds) {
    const pinnedUntilMs = getParkVerdictUnparkPinUntilMs({
      records: args.records,
      tabId,
      nowMs: args.nowMs
    })
    if (pinnedUntilMs === null) {
      continue
    }
    pinnedTabIds.add(tabId)
    earliestPinExpiryMs =
      earliestPinExpiryMs === null ? pinnedUntilMs : Math.min(earliestPinExpiryMs, pinnedUntilMs)
  }
  return { pinnedTabIds, earliestPinExpiryMs }
}
