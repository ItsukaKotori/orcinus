import { describe, expect, it } from 'vitest'
import {
  TERMINAL_TAB_PARK_FLIP_BURST_LIMIT,
  TERMINAL_TAB_PARK_FLIP_BURST_WINDOW_MS,
  TERMINAL_TAB_PARK_FLIP_COMMIT_COST,
  TERMINAL_TAB_PARK_FLIP_WINDOW_MS,
  getParkVerdictUnparkPinUntilMs,
  recordParkVerdictFlips,
  selectParkVerdictPinnedTabIds,
  type ParkVerdictFlipRecord
} from './terminal-park-verdict-flip-telemetry'

const TAB = 'tab-1'
/** Slower than the burst window, so only slow-churn behavior can apply. */
const SLOW_CHURN_STEP_MS = TERMINAL_TAB_PARK_FLIP_BURST_WINDOW_MS * 4

function observe(args: {
  records: Map<string, ParkVerdictFlipRecord>
  parked: boolean
  nowMs: number
  liveTabIds?: ReadonlySet<string>
}): void {
  recordParkVerdictFlips({
    records: args.records,
    liveTabIds: args.liveTabIds ?? new Set([TAB]),
    nextParkedTabIds: args.parked ? new Set([TAB]) : new Set(),
    nowMs: args.nowMs
  })
}

// Why asserted: the whole point of the burst trigger is that it is derived from
// React's 50-commit bail, not copied from an arbitrary threshold. If the commit
// budget ever converges the damping stops firing before React throws #185.
describe('burst damping threshold', () => {
  it('stays under React NESTED_UPDATE_LIMIT at the assumed commits-per-flip cost', () => {
    expect(TERMINAL_TAB_PARK_FLIP_BURST_LIMIT * TERMINAL_TAB_PARK_FLIP_COMMIT_COST).toBeLessThan(50)
  })
})

describe('recordParkVerdictFlips', () => {
  it('does not count flips for a stable verdict', () => {
    const records = new Map<string, ParkVerdictFlipRecord>()
    for (let i = 0; i < 100; i += 1) {
      observe({ records, parked: true, nowMs: 1_000 + i * 1_000 })
    }

    expect(records.get(TAB)?.burstFlips).toBe(0)
    expect(records.get(TAB)?.pinnedUntilMs ?? null).toBeNull()
  })

  it('pins a burst that churns at render cadence', () => {
    const records = new Map<string, ParkVerdictFlipRecord>()
    for (let i = 0; i < 40; i += 1) {
      observe({ records, parked: i % 2 === 0, nowMs: 1_000 + i * 10 })
    }

    const record = records.get(TAB)
    expect(record?.pinnedUntilMs).toBe(1_000 + TERMINAL_TAB_PARK_FLIP_BURST_LIMIT * 10 + TERMINAL_TAB_PARK_FLIP_WINDOW_MS)
  })

  it('does not pin churn spread past the burst window', () => {
    // Why: slow churn must not be damped — parking it out for a minute would
    // cost a mounted pane's memory for a verdict that was never near React's bail.
    const records = new Map<string, ParkVerdictFlipRecord>()
    for (let i = 0; i < 20; i += 1) {
      observe({ records, parked: i % 2 === 0, nowMs: 1_000 + i * SLOW_CHURN_STEP_MS })
    }

    expect(records.get(TAB)?.pinnedUntilMs ?? null).toBeNull()
  })

  it('re-arms a pin after the window elapses', () => {
    const records = new Map<string, ParkVerdictFlipRecord>()
    for (let i = 0; i < 40; i += 1) {
      observe({ records, parked: i % 2 === 0, nowMs: 1_000 + i * 10 })
    }
    const firstPin = records.get(TAB)?.pinnedUntilMs ?? null
    expect(firstPin).not.toBeNull()

    const laterMs = 1_000 + TERMINAL_TAB_PARK_FLIP_WINDOW_MS * 2
    for (let i = 0; i < 40; i += 1) {
      observe({ records, parked: i % 2 === 0, nowMs: laterMs + i * 10 })
    }

    expect(records.get(TAB)?.pinnedUntilMs).not.toBe(firstPin)
  })

  // Why: an unclamped backwards jump would freeze the window and suppress the
  // damping this module exists to apply.
  it('treats a backwards clock jump as a fresh window', () => {
    const records = new Map<string, ParkVerdictFlipRecord>()
    observe({ records, parked: true, nowMs: 10_000_000 })
    observe({ records, parked: false, nowMs: 1_000 })

    expect(records.get(TAB)?.windowStartMs).toBe(1_000)
    expect(records.get(TAB)?.burstFlips).toBe(1)
  })

  // Why: >= is the boundary operator; a > regression would silently stretch the
  // window and delay every burst by one full period.
  it('treats an exactly-elapsed window as expired', () => {
    const records = new Map<string, ParkVerdictFlipRecord>()
    observe({ records, parked: true, nowMs: 1_000 })
    observe({ records, parked: false, nowMs: 1_000 + TERMINAL_TAB_PARK_FLIP_WINDOW_MS })

    expect(records.get(TAB)?.windowStartMs).toBe(1_000 + TERMINAL_TAB_PARK_FLIP_WINDOW_MS)
    expect(records.get(TAB)?.burstFlips).toBe(1)
  })

  it('honours the window, burst-window and burst-limit overrides', () => {
    const records = new Map<string, ParkVerdictFlipRecord>()
    for (let i = 0; i < 10; i += 1) {
      recordParkVerdictFlips({
        records,
        liveTabIds: new Set([TAB]),
        nextParkedTabIds: i % 2 === 0 ? new Set([TAB]) : new Set(),
        nowMs: 1_000 + i * 100,
        flipWindowMs: 5_000,
        burstWindowMs: 10,
        burstLimit: 2
      })
    }

    // The 100ms step outruns the 10ms burst window, so the burst counter resets
    // on every flip and no pin is ever set.
    expect(records.get(TAB)?.pinnedUntilMs ?? null).toBeNull()
  })

  it('keeps per-tab bursts independent', () => {
    const records = new Map<string, ParkVerdictFlipRecord>()
    const other = 'tab-2'
    for (let i = 0; i < 40; i += 1) {
      recordParkVerdictFlips({
        records,
        liveTabIds: new Set([TAB, other]),
        // Why: TAB churns every call, other stays parked throughout.
        nextParkedTabIds: i % 2 === 0 ? new Set([TAB, other]) : new Set([other]),
        nowMs: 1_000 + i * 10
      })
    }

    expect(records.get(TAB)?.pinnedUntilMs).not.toBeNull()
    expect(records.get(other)?.burstFlips).toBe(0)
  })

  it('drops records for tabs that no longer exist', () => {
    const records = new Map<string, ParkVerdictFlipRecord>()
    observe({ records, parked: true, nowMs: 1_000 })

    recordParkVerdictFlips({
      records,
      liveTabIds: new Set(),
      nextParkedTabIds: new Set(),
      nowMs: 2_000
    })

    expect(records.size).toBe(0)
  })
})

describe('getParkVerdictUnparkPinUntilMs', () => {
  function churnToBurst(records: Map<string, ParkVerdictFlipRecord>, startMs = 1_000): number {
    for (let i = 0; i < 40; i += 1) {
      observe({ records, parked: i % 2 === 0, nowMs: startMs + i * 10 })
    }
    // The first observation only seeds the record, so flip N lands one step later.
    return startMs + TERMINAL_TAB_PARK_FLIP_BURST_LIMIT * 10
  }

  it('does not pin a stable verdict', () => {
    const records = new Map<string, ParkVerdictFlipRecord>()
    observe({ records, parked: true, nowMs: 1_000 })

    expect(getParkVerdictUnparkPinUntilMs({ records, tabId: TAB, nowMs: 2_000 })).toBeNull()
    expect(getParkVerdictUnparkPinUntilMs({ records, tabId: 'missing', nowMs: 2_000 })).toBeNull()
  })

  // Why the deadline and not a boolean: the caller has to schedule a recheck at
  // it, or the pin never lifts once it has stopped the churn that woke the
  // verdict effect.
  it('reports the pin deadline one window out, then re-arms', () => {
    const records = new Map<string, ParkVerdictFlipRecord>()
    const pinnedAtMs = churnToBurst(records)
    const pinUntilMs = pinnedAtMs + TERMINAL_TAB_PARK_FLIP_WINDOW_MS

    expect(getParkVerdictUnparkPinUntilMs({ records, tabId: TAB, nowMs: pinnedAtMs + 1 })).toBe(
      pinUntilMs
    )
    expect(getParkVerdictUnparkPinUntilMs({ records, tabId: TAB, nowMs: pinUntilMs })).toBeNull()
    expect(records.get(TAB)?.burstFlips).toBe(0)
  })

  // Why: a backwards clock jump must release the pin, not strand it for a window.
  it('releases the pin when the clock jumps backwards', () => {
    const records = new Map<string, ParkVerdictFlipRecord>()
    churnToBurst(records)

    expect(getParkVerdictUnparkPinUntilMs({ records, tabId: TAB, nowMs: 5 })).toBeNull()
  })

  // Why: the pin window starts at the first flip and expires one window later;
  // an exogenous flip must not hand the pane back to the parking policy mid-damping.
  it('survives an exogenous flip that lands mid-pin', () => {
    const records = new Map<string, ParkVerdictFlipRecord>()
    const pinnedAtMs = churnToBurst(records)
    const pinUntilMs = pinnedAtMs + TERMINAL_TAB_PARK_FLIP_WINDOW_MS
    const midPinMs = 1_000 + TERMINAL_TAB_PARK_FLIP_WINDOW_MS

    expect(midPinMs).toBeLessThan(pinUntilMs)
    observe({ records, parked: true, nowMs: midPinMs })

    expect(getParkVerdictUnparkPinUntilMs({ records, tabId: TAB, nowMs: midPinMs })).toBe(
      pinUntilMs
    )
  })
})

// Why liveness and not presence: a pinned tab can stop being cold-park eligible
// before its deadline, and nothing consults getParkVerdictUnparkPinUntilMs for
// it again. A stale deadline must not silence damping forever.
describe('expired pins', () => {
  it('re-arms damping without a getParkVerdictUnparkPinUntilMs call', () => {
    const records = new Map<string, ParkVerdictFlipRecord>()
    for (let i = 0; i < 40; i += 1) {
      observe({ records, parked: i % 2 === 0, nowMs: 1_000 + i * 10 })
    }
    const firstPin = records.get(TAB)?.pinnedUntilMs ?? null
    expect(firstPin).not.toBeNull()

    // Churn resumes past the pin deadline; the pin was never read back.
    const afterPinMs = 1_000 + TERMINAL_TAB_PARK_FLIP_WINDOW_MS * 2
    for (let i = 0; i < 40; i += 1) {
      observe({ records, parked: i % 2 === 0, nowMs: afterPinMs + i * 10 })
    }

    expect(records.get(TAB)?.pinnedUntilMs).toBeGreaterThan(afterPinMs)
  })

  // Why: the pin is set from flips on the rendered verdict, so it has to be
  // readable — and expirable — for any live tab, not only cold-park candidates
  // (issue #15136: the driver was the worktree-level park prop).
  it('selects and expires pins for tabs the cold-park selector never proposed', () => {
    const records = new Map<string, ParkVerdictFlipRecord>()
    for (let i = 0; i < 40; i += 1) {
      observe({ records, parked: i % 2 === 0, nowMs: 1_000 + i * 10 })
    }

    const pinned = selectParkVerdictPinnedTabIds({ records, tabIds: [TAB], nowMs: 1_500 })
    expect(pinned.pinnedTabIds).toEqual(new Set([TAB]))
    expect(pinned.earliestPinExpiryMs).toBe(records.get(TAB)?.pinnedUntilMs)

    // Past the deadline the pin lapses in place, so damping never latches on.
    const lapsed = selectParkVerdictPinnedTabIds({
      records,
      tabIds: [TAB],
      nowMs: 1_000 + TERMINAL_TAB_PARK_FLIP_WINDOW_MS * 3
    })
    expect(lapsed.pinnedTabIds.size).toBe(0)
    expect(lapsed.earliestPinExpiryMs).toBeNull()
    expect(records.get(TAB)?.pinnedUntilMs).toBeNull()
  })
})
