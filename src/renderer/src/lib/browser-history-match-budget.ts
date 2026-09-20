/**
 * Checked-in performance budget for the shared browser-history matcher, measured
 * against the synthetic corpus in `browser-history-match.performance.test.ts`.
 * These are ceilings for catching order-of-magnitude regressions, not targets —
 * the measured numbers on a developer machine sit far under each one.
 *
 * Raising any value requires a fresh measurement recorded in the PR.
 *
 * 2026-09-20 re-measurement (prepareP95Ms 2 -> 4): isolated p95 stayed at
 * 0.08–0.23 ms across 20 batches, but GC pauses under full-suite worker
 * parallelism pushed the p95 to 2.245 ms. 4 ms still holds a >17x margin over
 * the isolated p95, and a real preparation regression remains an
 * order-of-magnitude miss.
 */
export const BROWSER_HISTORY_MATCH_BUDGET = {
  /** Entries prepared in one omnibox open. Tracks MAX_BROWSER_HISTORY_ENTRIES. */
  candidateCount: 200,
  /** p95 ms to prepare the whole corpus once (cold open). */
  prepareP95Ms: 4,
  /** p95 ms to match the prepared corpus against one query. */
  matchP95Ms: 2
} as const
