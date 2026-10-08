/**
 * GitHub API rate-limit snapshot client.
 *
 * Why: heavy fan-out (listWorkItems × repos, org-walks) can drain the
 * core/search buckets; surfacing remaining budget lets users self-regulate
 * rather than throttle. The probe itself is exempt from rate-limit accounting
 * per GitHub docs.
 *
 * Ported from orca `src/main/github/rate-limit.ts` (cache/force/single-flight
 * from `:277-324`, `parseBucket` from `:40-55`). The Electron-side runner
 * (`ghExecFileAsync` + the shared quota breaker) is lifted out: the renderer
 * takes an injectable `GhExecClient` and returns the result envelope instead
 * of throwing. The reference pins `host: 'github.com'`, but this port's
 * `GhExecOptions` has no host field, so the probe runs unpinned (disclosed in
 * the task report).
 */
import type {
  GetRateLimitResult,
  GitHubRateLimitBucket,
  GitHubRateLimitSnapshot
} from '../../../../shared/github/rate-limit-types'
import type { GhExecClient } from './repo-identity'

// Why: GET /rate_limit is exempt from limits, so caching only avoids a gh
// subprocess per render; 30s stays live while absorbing 1/s polling.
export const RATE_LIMIT_CACHE_TTL_MS = 30_000

type GhRateLimitPayload = {
  resources?: {
    core?: { limit?: number; remaining?: number; reset?: number }
    search?: { limit?: number; remaining?: number; reset?: number }
    graphql?: { limit?: number; remaining?: number; reset?: number }
  }
}

export type RateLimitClientDeps = {
  client: GhExecClient
  now?: () => number
}

export type RateLimitClient = {
  getRateLimit: (options?: { force?: boolean }) => Promise<GetRateLimitResult>
}

// ── orca rate-limit.ts:40-55 (verbatim; fallback timestamp injected) ──

export function parseBucket(
  raw: { limit?: number; remaining?: number; reset?: number } | undefined,
  nowSeconds: number = Math.floor(Date.now() / 1000)
): GitHubRateLimitBucket {
  // Why: absent bucket → 0/0/now so the UI reads "unknown" rather than a misleading "plenty left".
  return {
    limit: typeof raw?.limit === 'number' ? raw.limit : 0,
    remaining: typeof raw?.remaining === 'number' ? raw.remaining : 0,
    resetAt: typeof raw?.reset === 'number' ? raw.reset : nowSeconds
  }
}

export function createRateLimitClient(deps: RateLimitClientDeps): RateLimitClient {
  const now = deps.now ?? (() => Date.now())
  let cached: GitHubRateLimitSnapshot | null = null
  // Why: cache failures too — a host that 404s every probe (GHES with rate limiting off) would otherwise spawn a gh subprocess per refresh.
  let probeFailure: { at: number; error: string } | null = null
  // Why: single-flight so a concurrent fan-out resolves to one probe — the TTL cache can't dedupe calls that start before the first lands.
  let probeInFlight: Promise<GetRateLimitResult> | null = null

  // ── orca rate-limit.ts:298-324 (acquire/release + host pin dropped) ──

  async function fetchRateLimitSnapshot(): Promise<GetRateLimitResult> {
    try {
      const stdout = await deps.client.runOrThrow(['api', 'rate_limit'], {})
      const parsed = JSON.parse(stdout) as GhRateLimitPayload
      const nowSeconds = Math.floor(now() / 1000)
      const snapshot: GitHubRateLimitSnapshot = {
        core: parseBucket(parsed.resources?.core, nowSeconds),
        search: parseBucket(parsed.resources?.search, nowSeconds),
        graphql: parseBucket(parsed.resources?.graphql, nowSeconds),
        fetchedAt: now()
      }
      cached = snapshot
      probeFailure = null
      return { ok: true, snapshot }
    } catch (err) {
      const message = err instanceof Error ? err.message : String(err)
      probeFailure = { at: now(), error: message }
      return { ok: false, error: message }
    }
  }

  // ── orca rate-limit.ts:277-296 (verbatim; Date.now → now()) ──

  async function getRateLimit(options?: { force?: boolean }): Promise<GetRateLimitResult> {
    if (!options?.force && cached && now() - cached.fetchedAt < RATE_LIMIT_CACHE_TTL_MS) {
      return { ok: true, snapshot: cached }
    }
    if (!options?.force && probeFailure && now() - probeFailure.at < RATE_LIMIT_CACHE_TTL_MS) {
      return { ok: false, error: probeFailure.error }
    }
    if (!options?.force && probeInFlight) {
      return probeInFlight
    }
    const probe = fetchRateLimitSnapshot()
    probeInFlight = probe
    try {
      return await probe
    } finally {
      if (probeInFlight === probe) {
        probeInFlight = null
      }
    }
  }

  return { getRateLimit }
}
