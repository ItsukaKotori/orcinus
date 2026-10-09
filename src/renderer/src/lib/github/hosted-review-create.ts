/**
 * Hosted review creation eligibility — GitHub-only port for the renderer.
 *
 * Why: the renderer decides whether the composer can offer Create PR, and it
 * must reproduce main's blocker order exactly so UI guidance never outruns what
 * the create path will accept. Ported from orca
 * `src/main/source-control/hosted-review-creation.ts:100-239` (decision order +
 * fields), `hosted-review-creation-git-state.ts:169-249` (default base ref and
 * remote-tracking probe via `repo-default-base-ref.ts:95-115`), and
 * `hosted-review-creation-provider.ts:22-47` (auth probe). Execution-host
 * routing is dropped: this surface only acts on local worktrees.
 */
import type { GitStatusEntry, GitUpstreamStatus } from '../../../../shared/git-status-types'
import { isSafeGitRefName } from '../../../../shared/git-status-upstream-ref'
import { isDefaultGitHubHost } from '../../../../shared/github/repository-identity-key'
import { supportsHostedReviewCreation } from '../../../../shared/hosted-review-creation-providers'
import {
  isRemoteHeadRef,
  normalizeHostedReviewBaseRef,
  normalizeHostedReviewHeadRef
} from '../../../../shared/hosted-review-refs'
import type {
  HostedReviewCreationEligibility,
  HostedReviewCreationEligibilityArgs,
  HostedReviewInfo,
  HostedReviewLookupOutcome,
  HostedReviewProvider
} from '../../../../shared/hosted-review'
import { parseAuthStatus } from './auth-diagnose'
import type { GhExecOptions, GhExecResult } from './gh-exec-client'
import { GitReadError } from './git-read-client'
import type { HostedReviewClient } from './hosted-review'
import type { RepoIdentityResolver } from './repo-identity'

export type RunGit = (args: string[]) => Promise<{ stdout: string }>

export type HostedReviewCreationReadStatus = (
  worktreePath: string
) => Promise<{ entries: readonly GitStatusEntry[] }>

export type HostedReviewCreationReadUpstream = (worktreePath: string) => Promise<GitUpstreamStatus>

export type HostedReviewCreationReadTemplate = (
  worktreePath: string,
  relativePath: string
) => Promise<{ content: string; isBinary?: boolean | null } | null>

export type HostedReviewCreationDeps = {
  client: { run: (args: string[], options?: GhExecOptions) => Promise<GhExecResult> }
  identity: Pick<RepoIdentityResolver, 'getRepoSlug'>
  reviewLookup: Pick<HostedReviewClient, 'forBranch'>
  makeRunGit: (worktreePath: string) => RunGit
  // Why: only the create path (Task 6) consumes these; eligibility runs on the
  // renderer's already-loaded status/upstream args.
  readStatus: HostedReviewCreationReadStatus
  readUpstream: HostedReviewCreationReadUpstream
  readTemplate?: HostedReviewCreationReadTemplate
  now?: () => number
}

// `enforceBaseOnRemote` is set only by the create-time preflight; the renderer
// probe leaves it unset so a local-only parent can auto-correct (orca
// hosted-review-creation.ts:40-42).
export type HostedReviewCreationEligibilityInput = HostedReviewCreationEligibilityArgs & {
  enforceBaseOnRemote?: boolean
  currentHeadOid?: string | null
}

export type HostedReviewCreation = {
  getCreationEligibility: (
    args: HostedReviewCreationEligibilityInput
  ) => Promise<HostedReviewCreationEligibility>
}

// orca `git/repo-default-base-ref.ts:21-26`
const DEFAULT_BASE_REF_PROBES: readonly { ref: string; returnAs: string }[] = [
  { ref: 'refs/remotes/origin/main', returnAs: 'origin/main' },
  { ref: 'refs/remotes/origin/master', returnAs: 'origin/master' },
  { ref: 'refs/heads/main', returnAs: 'main' },
  { ref: 'refs/heads/master', returnAs: 'master' }
]

const CONVENTIONAL_REMOTES: readonly string[] = ['origin', 'upstream']

async function hasGitRef(runGit: RunGit, ref: string): Promise<boolean> {
  try {
    await runGit(['rev-parse', '--verify', '--quiet', ref])
    return true
  } catch {
    return false
  }
}

async function resolveVerifiedOriginHeadBaseRef(runGit: RunGit): Promise<string | null> {
  try {
    const { stdout } = await runGit(['symbolic-ref', '--quiet', 'refs/remotes/origin/HEAD'])
    const ref = stdout.trim()
    if (!ref || !(await hasGitRef(runGit, ref))) {
      return null
    }
    return ref.replace(/^refs\/remotes\//, '')
  } catch {
    return null
  }
}

/** Resolve the default base ref without inventing a fallback branch. */
export async function getDefaultBaseRef(runGit: RunGit): Promise<string | null> {
  const originHeadBaseRef = await resolveVerifiedOriginHeadBaseRef(runGit)
  if (originHeadBaseRef) {
    return originHeadBaseRef
  }
  for (const { ref, returnAs } of DEFAULT_BASE_REF_PROBES) {
    if (await hasGitRef(runGit, ref)) {
      return returnAs
    }
  }
  return null
}

// Why (orca `git/exact-ref-probe.ts:25-40`): a missing ref is a numeric exit 1
// with no stderr; every other failure is inconclusive so probes fail open.
function isDefiniteNoMatch(error: unknown): boolean {
  return error instanceof GitReadError && error.code === 1 && error.stderr.trim().length === 0
}

async function probeAnyExactRef(
  runGit: RunGit,
  refs: readonly string[]
): Promise<{ found: boolean; unknown: boolean }> {
  let unknown = false
  for (const ref of refs) {
    try {
      await runGit(['show-ref', '--verify', '--quiet', '--', ref])
      return { found: true, unknown }
    } catch (error) {
      if (!isDefiniteNoMatch(error)) {
        unknown = true
      }
    }
  }
  return { found: false, unknown }
}

function* iterateGitOutputLines(output: string): Generator<string> {
  let lineStart = 0
  for (let index = 0; index < output.length; index++) {
    const code = output.charCodeAt(index)
    if (code !== 10 && code !== 13) {
      continue
    }
    yield output.slice(lineStart, index)
    if (code === 13 && output.charCodeAt(index + 1) === 10) {
      index++
    }
    lineStart = index + 1
  }
  if (lineStart <= output.length) {
    yield output.slice(lineStart)
  }
}

// Ported from orca `hosted-review-creation-git-state.ts:64-104`: `show-ref --`
// matches a suffix at any depth, so require the remote component to be exactly
// one segment; otherwise a branch named `origin/feature/main` answers `main`.
function parseSuffixRemoteRefs(
  output: string,
  base: string,
  remotes: readonly string[]
): string[] {
  const refs = new Set<string>()
  for (const line of iterateGitOutputLines(output)) {
    const separator = line.indexOf(' ')
    if (separator === -1) {
      continue
    }
    const fullRef = line.slice(separator + 1).trim()
    if (!fullRef.startsWith('refs/remotes/') || !isSafeGitRefName(fullRef)) {
      continue
    }
    const shortRef = fullRef.slice('refs/remotes/'.length)
    if (!shortRef.includes('/')) {
      continue
    }
    const isSingleRemoteSegmentMatch =
      shortRef.endsWith(`/${base}`) && shortRef.split('/').length === base.split('/').length + 1
    if (
      isRemoteHeadRef(shortRef, remotes) ||
      (base === 'HEAD' && shortRef.endsWith('/HEAD')) ||
      (shortRef !== base && !isSingleRemoteSegmentMatch)
    ) {
      continue
    }
    refs.add(shortRef)
    if (refs.size >= 2) {
      break
    }
  }
  return [...refs]
}

async function listSuffixRemoteBaseRefs(
  runGit: RunGit,
  base: string,
  remotes: readonly string[]
): Promise<{ refs: string[]; unknown: boolean }> {
  try {
    const { stdout } = await runGit(['show-ref', '--', base])
    return { refs: parseSuffixRemoteRefs(stdout, base, remotes), unknown: false }
  } catch (error) {
    // Unlike --verify, a pattern query exits 1 when it simply has no matches.
    return { refs: [], unknown: !isDefiniteNoMatch(error) }
  }
}

/**
 * Whether the candidate base resolves to a remote-tracking branch.
 *
 * Why: matches under the conventional remotes (a stale tracking ref survives a
 * removed remote) and reads the local tracking snapshot, not the live remote.
 */
export async function baseRefExistsOnRemote(runGit: RunGit, candidate: string): Promise<boolean> {
  const base = normalizeHostedReviewBaseRef(candidate).trim()
  if (!base) {
    return false
  }
  // Validate the complete tracking ref before interpolating user metadata into
  // Git arguments: `*`, `?`, or control bytes must never become a namespace scan.
  if (!isSafeGitRefName(`refs/remotes/${base}`)) {
    return false
  }
  const candidateRefs = new Set<string>()
  if (base.includes('/')) {
    // A qualified candidate (e.g. `fork/main`) is itself a complete tracking
    // ref and must remain discoverable even when `fork` is no longer configured.
    candidateRefs.add(`refs/remotes/${base}`)
  }
  for (const remote of CONVENTIONAL_REMOTES) {
    const ref = `refs/remotes/${remote}/${base}`
    if (isSafeGitRefName(ref)) {
      candidateRefs.add(ref)
    }
  }
  try {
    const exactResult = await probeAnyExactRef(runGit, [...candidateRefs])
    if (exactResult.found || exactResult.unknown) {
      return true
    }
    const suffixResult = await listSuffixRemoteBaseRefs(runGit, base, CONVENTIONAL_REMOTES)
    return suffixResult.refs.length > 0 || suffixResult.unknown
  } catch {
    // An unexpected ref-probe failure is inconclusive, so preserve the candidate.
    return true
  }
}

export function createHostedReviewCreation(
  deps: HostedReviewCreationDeps
): HostedReviewCreation {
  // Why (orca hosted-review-creation-provider.ts:22-47): a non-zero exit can
  // still carry a parsed active account, so parse stdout+stderr rather than
  // trusting the exit code; a spawn failure is not authenticated.
  async function isGitHubAuthenticated(): Promise<boolean> {
    let result: GhExecResult
    try {
      result = await deps.client.run(['auth', 'status', '--hostname', 'github.com'])
    } catch {
      return false
    }
    const accounts = parseAuthStatus(`${result.stdout}\n${result.stderr}`)
    return accounts.some((account) => account.active)
  }

  async function getCreationEligibility(
    args: HostedReviewCreationEligibilityInput
  ): Promise<HostedReviewCreationEligibility> {
    const branch = normalizeHostedReviewHeadRef(args.branch).trim()
    const worktreePath = args.worktreePath?.trim() || args.repoPath
    const runGit = deps.makeRunGit(worktreePath)

    const slug = await deps.identity.getRepoSlug(worktreePath)
    const provider: HostedReviewProvider =
      slug && isDefaultGitHubHost(slug.host) ? 'github' : 'unsupported'

    // The base is only a candidate; fall back to the repo default so a
    // local-only parent targets a remote-resolvable ref.
    const candidateBase = args.base?.trim() || null
    const candidateBaseOnRemote =
      candidateBase !== null && (await baseRefExistsOnRemote(runGit, candidateBase))
    let defaultBaseRef: string | null
    if (candidateBase && candidateBaseOnRemote) {
      defaultBaseRef = candidateBase
    } else {
      defaultBaseRef = (await getDefaultBaseRef(runGit)) ?? candidateBase
    }
    const baseBranch = defaultBaseRef ? normalizeHostedReviewBaseRef(defaultBaseRef) : null

    let review: HostedReviewInfo | null = null
    // Why: track lookup failure so a swallowed error is never mistaken for
    // authoritative no-review evidence.
    let lookupFailed = false
    try {
      review = await deps.reviewLookup.forBranch({
        repoPath: worktreePath,
        branch,
        currentHeadOid: args.currentHeadOid ?? null,
        active: true,
        linkedGitHubPR: args.linkedGitHubPR ?? null,
        fallbackGitHubPR: args.fallbackGitHubPR ?? null
      })
    } catch (error) {
      lookupFailed = true
      console.warn('Hosted review lookup failed; treating existing-review as unavailable:', error)
    }
    const reviewLookupOutcome: HostedReviewLookupOutcome = review
      ? 'found'
      : lookupFailed
        ? 'unavailable'
        : 'not_found'
    const baseResult = {
      provider,
      review: review ? { number: review.number, url: review.url } : null,
      reviewLookupOutcome,
      defaultBaseRef,
      head: branch || null
    }

    if (!branch || branch === 'HEAD') {
      return { ...baseResult, canCreate: false, blockedReason: 'detached_head', nextAction: null }
    }
    if (review) {
      return {
        ...baseResult,
        canCreate: false,
        blockedReason: 'existing_review',
        nextAction: 'open_existing_review'
      }
    }
    if (!supportsHostedReviewCreation(provider)) {
      return {
        ...baseResult,
        canCreate: false,
        blockedReason: 'unsupported_provider',
        nextAction: null
      }
    }
    if (baseBranch && branch.toLowerCase() === baseBranch.toLowerCase()) {
      return { ...baseResult, canCreate: false, blockedReason: 'default_branch', nextAction: null }
    }
    if (args.hasUncommittedChanges) {
      return { ...baseResult, canCreate: false, blockedReason: 'dirty', nextAction: 'commit' }
    }
    if (args.hasUpstream === false) {
      return {
        ...baseResult,
        canCreate: false,
        blockedReason: 'no_upstream',
        nextAction: 'publish'
      }
    }
    if (args.hasUpstream !== true) {
      return { ...baseResult, canCreate: false, blockedReason: null, nextAction: null }
    }
    if ((args.behind ?? 0) > 0) {
      return { ...baseResult, canCreate: false, blockedReason: 'needs_sync', nextAction: 'sync' }
    }
    if (!(await isGitHubAuthenticated())) {
      return {
        ...baseResult,
        canCreate: false,
        blockedReason: 'auth_required',
        nextAction: 'authenticate'
      }
    }
    if ((args.ahead ?? 0) > 0) {
      return { ...baseResult, canCreate: false, blockedReason: 'needs_push', nextAction: 'push' }
    }
    // Why: providers target the submitted base verbatim; block a local-only
    // base here with actionable copy.
    if (args.enforceBaseOnRemote && candidateBase && !candidateBaseOnRemote) {
      return {
        ...baseResult,
        canCreate: false,
        blockedReason: 'base_not_on_remote',
        nextAction: null
      }
    }
    return {
      ...baseResult,
      canCreate: lookupFailed ? false : Boolean(baseBranch),
      blockedReason: null,
      nextAction: null
    }
  }

  return { getCreationEligibility }
}
