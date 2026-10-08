/**
 * Hosted review lookup for a branch — GitHub-only port for the renderer.
 *
 * Why: callers poll `forBranch` from every worktree surface, and they share the
 * host's per-user API quota, so a branch cache sits above the provider call.
 * This port keeps orca's tiered pacing (`hosted-review-branch-cache.ts`):
 * found reviews refresh at the callers' cadence (60s), negative answers are
 * paced far slower (15min) unless the caller marks the branch active (60s),
 * and a merged answer is head-sensitive so the merged-at-head carve-out stays
 * visible only while the inspected head matches.
 *
 * The Electron-side cache's deadline/backoff/capacity/single-flight machinery
 * is not ported — 2D.1 only needs the TTL contract.
 */
import type {
  HostedReviewForBranchArgs,
  HostedReviewInfo
} from '../../../../shared/hosted-review'
import { isDefaultGitHubHost } from '../../../../shared/github/repository-identity-key'
import { hostedReviewInfoFromGitHubPRInfo } from '../../../../shared/hosted-review-github'
import type { PRForBranchLookup } from './pr-for-branch'
import type { RepoIdentityResolver } from './repo-identity'

// Why: a found review still refreshes at the callers' poll cadence; the cache
// exists to collapse concurrent clients, not to make review state go stale.
export const FOUND_REVIEW_TTL_MS = 60_000
export const ACTIVE_NO_REVIEW_TTL_MS = 60_000
export const NO_REVIEW_TTL_MS = 15 * 60_000

// Why: NUL is the one byte a repo path or branch name cannot contain, so a
// scope prefix cannot straddle a component boundary.
const KEY_SEPARATOR = '\0'

export type HostedReviewClientDeps = {
  identity: Pick<RepoIdentityResolver, 'resolveCandidates'>
  lookup: Pick<PRForBranchLookup, 'getPRForBranchOutcome'>
  now?: () => number
}

export type HostedReviewClient = {
  forBranch: (args: HostedReviewForBranchArgs) => Promise<HostedReviewInfo | null>
}

type CacheEntry = {
  review: HostedReviewInfo | null
  fetchedAt: number
  headOid: string | null
}

// ── orca hosted-review-branch-cache.ts:130-158 (TTL-only port) ───────

// Why: a merged review is the one answer that depends on the inspected head —
// the merged-at-head carve-out keeps it visible only while the head matches.
// Negative answers are deliberately head-insensitive, so a branch under active
// commits cannot defeat the long no-review interval.
function isHeadSensitive(entry: CacheEntry): boolean {
  return entry.review?.state === 'merged'
}

function refreshIntervalMs(entry: CacheEntry, active: boolean): number {
  if (entry.review !== null) {
    return FOUND_REVIEW_TTL_MS
  }
  return active ? ACTIVE_NO_REVIEW_TTL_MS : NO_REVIEW_TTL_MS
}

function isFresh(
  entry: CacheEntry,
  headOid: string | null,
  active: boolean,
  nowMs: number
): boolean {
  if (isHeadSensitive(entry) && headOid !== null && entry.headOid !== null) {
    if (headOid !== entry.headOid) {
      return false
    }
  }
  return nowMs - entry.fetchedAt < refreshIntervalMs(entry, active)
}

export function createHostedReviewClient(deps: HostedReviewClientDeps): HostedReviewClient {
  const now = deps.now ?? (() => Date.now())
  const entries = new Map<string, CacheEntry>()

  function cacheKey(args: HostedReviewForBranchArgs): string {
    return [
      args.repoPath,
      args.branch.replace(/^refs\/heads\//, ''),
      // Each linked id selects a different lookup, so it belongs in the identity.
      args.linkedGitHubPR ?? '',
      args.fallbackGitHubPR ?? ''
    ].join(KEY_SEPARATOR)
  }

  async function fetchForBranch(args: HostedReviewForBranchArgs): Promise<HostedReviewInfo | null> {
    const branch = args.branch.replace(/^refs\/heads\//, '')
    const linkedPRNumber = typeof args.linkedGitHubPR === 'number' ? args.linkedGitHubPR : null
    const fallbackPRNumber =
      linkedPRNumber === null && typeof args.fallbackGitHubPR === 'number'
        ? args.fallbackGitHubPR
        : null
    // Why: detached HEAD cannot use branch lookup, but an exact linked/fallback
    // id can still resolve the review without probing an empty branch name.
    if (!branch && linkedPRNumber === null && fallbackPRNumber === null) {
      return null
    }
    const { candidates } = await deps.identity.resolveCandidates(args.repoPath)
    // Why: GHES is identity-only in 2D.1 — hosted review is unsupported on a
    // non-default host, and gh_exec cannot route the query to it.
    if (candidates.length === 0 || !isDefaultGitHubHost(candidates[0]?.host)) {
      return null
    }
    const outcome = await deps.lookup.getPRForBranchOutcome({
      worktreePath: args.repoPath,
      branch,
      linkedPRNumber,
      fallbackPRNumber,
      acceptMergedFallbackPR: fallbackPRNumber !== null,
      currentHeadOid: args.currentHeadOid
    })
    // Why: collapsing an upstream error into a null "no review" lets a
    // transient gh failure poison the cache with a definitive miss. Surface
    // the error so callers can preserve the last known review state
    // (orca forge-provider.ts:134-141).
    if (outcome.kind === 'upstream-error') {
      throw new Error(`GitHub PR lookup failed (${outcome.errorType}): ${outcome.message}`)
    }
    return outcome.kind === 'found' ? hostedReviewInfoFromGitHubPRInfo(outcome.pr) : null
  }

  async function forBranch(args: HostedReviewForBranchArgs): Promise<HostedReviewInfo | null> {
    const key = cacheKey(args)
    const headOid = args.currentHeadOid?.trim() || null
    const nowMs = now()
    const cached = entries.get(key)
    if (cached && isFresh(cached, headOid, args.active === true, nowMs)) {
      return cached.review
    }
    const review = await fetchForBranch(args)
    entries.set(key, { review, fetchedAt: now(), headOid })
    return review
  }

  return { forBranch }
}
