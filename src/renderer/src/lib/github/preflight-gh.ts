/**
 * gh CLI readiness probe (`installed` / `authenticated`) for preflight UI.
 *
 * Why: the preflight badge must distinguish "gh is not on PATH" from "gh is
 * installed but nobody is logged in" without surfacing raw command output.
 * `gh auth status` exits non-zero when logged out, so its stdout/stderr are
 * parsed regardless of the exit code; only a spawn-class launch failure proves
 * that gh is missing.
 */
import { parseAuthStatus } from './auth-diagnose'
import type { GhExecClient } from './repo-identity'

export const GH_READINESS_CACHE_TTL_MS = 60_000

export type GhReadiness = { installed: boolean; authenticated: boolean }

export type GhReadinessProbeDeps = {
  client: GhExecClient
  now?: () => number
}

export type GhReadinessProbe = () => Promise<GhReadiness>

/**
 * The Rust `gh_exec` bridge reports a missing binary as a throw with one of
 * these spawn-class messages (`commands/gh.rs` resolve/spawn paths).
 */
export function isGhMissingError(err: unknown): boolean {
  const message = err instanceof Error ? err.message : String(err)
  return /gh: command not found|spawn gh enoent|'gh' is not recognized/i.test(message)
}

export function createGhReadinessProbe(deps: GhReadinessProbeDeps): GhReadinessProbe {
  const now = deps.now ?? (() => Date.now())
  let cached: { at: number; result: GhReadiness } | null = null

  return async function probe(): Promise<GhReadiness> {
    if (cached && now() - cached.at < GH_READINESS_CACHE_TTL_MS) {
      return cached.result
    }
    let result: GhReadiness
    try {
      const runResult = await deps.client.run(['auth', 'status'])
      // Why: `gh auth status` writes to stderr by default (stdout in some
      // versions) and exits non-zero when logged out — parse either way.
      const accounts = parseAuthStatus(`${runResult.stdout}\n${runResult.stderr}`)
      result = { installed: true, authenticated: accounts.some((account) => account.active) }
    } catch (err) {
      // Why: only a spawn-class launch failure proves gh is absent; a timeout
      // or transient runner failure must not claim "not installed".
      result = { installed: !isGhMissingError(err), authenticated: false }
    }
    cached = { at: now(), result }
    return result
  }
}
