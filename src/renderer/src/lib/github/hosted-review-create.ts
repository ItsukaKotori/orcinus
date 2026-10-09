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
import {
  hostedReviewProviderSupportsDraft,
  type CreateHostedReviewArgs,
  type CreateHostedReviewResult,
  type HostedReviewCreationBlockedReason,
  type HostedReviewCreationEligibility,
  type HostedReviewCreationEligibilityArgs,
  type HostedReviewInfo,
  type HostedReviewLookupOutcome,
  type HostedReviewProvider
} from '../../../../shared/hosted-review'
import { parseAuthStatus } from './auth-diagnose'
import type { GhExecOptions, GhExecResult } from './gh-exec-client'
import { GitReadError } from './git-read-client'
import type { HostedReviewClient } from './hosted-review'
import type { GitHubRepoIdentity, RepoIdentityResolver } from './repo-identity'

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
  reviewLookup: Pick<HostedReviewClient, 'forBranch' | 'invalidate'>
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
  create: (args: CreateHostedReviewArgs) => Promise<CreateHostedReviewResult>
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

// ── orca hosted-review-creation-provider.ts:72-116 ───────────────────

type HostedReviewCopy = {
  shortLabel: 'PR' | 'MR'
  reviewLabel: 'pull request' | 'merge request'
  providerName: string
  authInstruction: string
}

function reviewCopy(provider: HostedReviewProvider): HostedReviewCopy {
  if (provider === 'gitlab') {
    return {
      shortLabel: 'MR',
      reviewLabel: 'merge request',
      providerName: 'GitLab',
      authInstruction: 'Run glab auth login'
    }
  }
  if (provider === 'azure-devops') {
    return {
      shortLabel: 'PR',
      reviewLabel: 'pull request',
      providerName: 'Azure DevOps',
      authInstruction: 'Set ORCA_AZURE_DEVOPS_TOKEN'
    }
  }
  if (provider === 'gitea') {
    return {
      shortLabel: 'PR',
      reviewLabel: 'pull request',
      providerName: 'Gitea',
      authInstruction: 'Set ORCA_GITEA_TOKEN'
    }
  }
  if (provider === 'bitbucket') {
    return {
      shortLabel: 'PR',
      reviewLabel: 'pull request',
      providerName: 'Bitbucket',
      authInstruction: 'Connect Bitbucket in Settings > Integrations'
    }
  }
  return {
    shortLabel: 'PR',
    reviewLabel: 'pull request',
    providerName: 'GitHub',
    authInstruction: 'Run gh auth login'
  }
}

// ── orca hosted-review-creation-blocking.ts:9-102 ────────────────────

function blockedCreateResultForReason(
  reason: NonNullable<HostedReviewCreationBlockedReason>,
  provider: HostedReviewProvider,
  submittedBase?: string | null
): CreateHostedReviewResult | null {
  const copy = reviewCopy(provider)
  const baseLabel = submittedBase?.trim() ? `"${submittedBase.trim()}" ` : ''
  const blockedCreateResultByReason: Partial<
    Record<NonNullable<HostedReviewCreationBlockedReason>, CreateHostedReviewResult>
  > = {
    auth_required: {
      ok: false,
      code: 'auth_required',
      error: `Create ${copy.shortLabel} failed: ${copy.providerName} is not authenticated. Next step: ${copy.authInstruction} in this environment.`
    },
    unsupported_provider: {
      ok: false,
      code: 'unsupported_provider',
      error: `Creating ${copy.reviewLabel}s requires a ${copy.providerName} remote.`
    },
    dirty: {
      ok: false,
      code: 'validation',
      error: `Create ${copy.shortLabel} failed: commit or discard local changes before creating a ${copy.reviewLabel}.`
    },
    detached_head: {
      ok: false,
      code: 'validation',
      error: `Create ${copy.shortLabel} failed: switch to a branch before creating a ${copy.reviewLabel}.`
    },
    default_branch: {
      ok: false,
      code: 'validation',
      error: `Create ${copy.shortLabel} failed: choose a feature branch before creating a ${copy.reviewLabel}.`
    },
    no_upstream: {
      ok: false,
      code: 'validation',
      error: `Create ${copy.shortLabel} failed: publish this branch before creating a ${copy.reviewLabel}.`
    },
    needs_push: {
      ok: false,
      code: 'validation',
      error: `Create ${copy.shortLabel} failed: push this branch before creating a ${copy.reviewLabel}.`
    },
    needs_sync: {
      ok: false,
      code: 'validation',
      error: `Create ${copy.shortLabel} failed: sync this branch before creating a ${copy.reviewLabel}.`
    },
    fork_head_unsupported: {
      ok: false,
      code: 'validation',
      error: `Create ${copy.shortLabel} failed: refresh source control status and try again.`
    },
    base_not_on_remote: {
      ok: false,
      code: 'validation',
      error: `Create ${copy.shortLabel} failed: the base branch ${baseLabel}hasn't been pushed to the remote. Choose a pushed base or push it first.`
    }
  }
  return blockedCreateResultByReason[reason] ?? null
}

export function blockedEligibilityToCreateResult(
  eligibility: HostedReviewCreationEligibility,
  submittedBase?: string | null
): CreateHostedReviewResult | null {
  if (eligibility.canCreate) {
    return null
  }
  if (eligibility.review?.url) {
    const copy = reviewCopy(eligibility.provider)
    return {
      ok: false,
      code: 'already_exists',
      error: `A ${copy.reviewLabel} already exists for this branch.`,
      existingReview: eligibility.review
    }
  }
  if (eligibility.blockedReason) {
    return blockedCreateResultForReason(
      eligibility.blockedReason,
      eligibility.provider,
      submittedBase
    )
  }
  const copy = reviewCopy(eligibility.provider)
  return {
    ok: false,
    code: 'validation',
    error: `Create ${copy.shortLabel} failed: refresh source control status and try again.`
  }
}

// ── orca create-pr-error-classification.ts:3-74 ──────────────────────

function classifyCreatePRError(result: { stdout: string; stderr: string }): CreateHostedReviewResult {
  const message = `${result.stderr}\n${result.stdout}`.trim()
  if (message) {
    console.warn('createGitHubPullRequest failed:', message)
  }
  const lower = message.toLowerCase()
  if (
    lower.includes('not logged') ||
    lower.includes('not authenticated') ||
    lower.includes('authentication') ||
    lower.includes('gh auth login') ||
    lower.includes('http 401')
  ) {
    return {
      ok: false,
      code: 'auth_required',
      error:
        'Create PR failed: GitHub is not authenticated. Next step: run gh auth login in this environment.'
    }
  }
  if (lower.includes('already exists') || lower.includes('a pull request already exists')) {
    return {
      ok: false,
      code: 'already_exists',
      error: 'A pull request already exists for this branch.'
    }
  }
  if (lower.includes('timed out') || lower.includes('timeout')) {
    return {
      ok: false,
      code: 'unknown_completion',
      error: 'PR creation may have completed. Refreshing branch review state...'
    }
  }
  if (lower.includes('validation failed') || lower.includes('http 422')) {
    return {
      ok: false,
      code: 'validation',
      error:
        'Create PR failed: GitHub rejected the pull request. Check the base branch and branch state, then try again.'
    }
  }
  return {
    ok: false,
    code: 'unknown',
    error: 'Create PR failed: GitHub could not create the pull request. Try again in a moment.'
  }
}

function parseCreatePRPayload(stdout: string): { number: number; url: string } | null {
  const trimmed = stdout.trim()
  if (!trimmed) {
    return null
  }
  try {
    const parsed = JSON.parse(trimmed) as { number?: unknown; url?: unknown }
    const number = Number(parsed.number)
    const url = typeof parsed.url === 'string' ? parsed.url.trim() : ''
    if (Number.isInteger(number) && number > 0 && url) {
      return { number, url }
    }
  } catch {
    // Fall through to URL parsing for older gh versions without --json support.
  }
  // Why: match any host (not just github.com) so a GHES PR URL still parses (#8312).
  const urlMatch = trimmed.match(/https?:\/\/[^\s/]+\/[^\s/]+\/[^\s/]+\/pull\/(\d+)/)
  if (!urlMatch) {
    return null
  }
  return { number: Number(urlMatch[1]), url: urlMatch[0] }
}

// ── orca pull-request-template.ts:50-83 ──────────────────────────────

const PULL_REQUEST_TEMPLATE_PATHS = [
  '.github/pull_request_template.md',
  '.github/PULL_REQUEST_TEMPLATE.md',
  'pull_request_template.md',
  'PULL_REQUEST_TEMPLATE.md',
  'docs/pull_request_template.md',
  'docs/PULL_REQUEST_TEMPLATE.md'
] as const

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

  // ── orca hosted-review-creation.ts:44-98 (preflight) ───────────────

  async function validateCurrentBranchCanCreateReview(
    args: CreateHostedReviewArgs
  ): Promise<CreateHostedReviewResult | null> {
    const worktreePath = args.worktreePath?.trim() || args.repoPath
    const requestedHead = args.head ? normalizeHostedReviewHeadRef(args.head).trim() : ''
    const runGit = deps.makeRunGit(worktreePath)
    const { stdout } = await runGit(['rev-parse', '--abbrev-ref', 'HEAD'])
    const currentBranch = normalizeHostedReviewHeadRef(stdout.trim())
    const copy = reviewCopy(args.provider)
    if (requestedHead && requestedHead !== currentBranch) {
      return {
        ok: false,
        code: 'validation',
        error: `Create ${copy.shortLabel} failed: switch back to the selected branch before creating a ${copy.reviewLabel}.`
      }
    }

    try {
      const [status, upstreamStatus] = await Promise.all([
        deps.readStatus(worktreePath),
        deps.readUpstream(worktreePath)
      ])
      const submittedBase = normalizeHostedReviewBaseRef(args.base)
      const eligibility = await getCreationEligibility({
        repoPath: args.repoPath,
        ...(args.repoId !== undefined ? { repoId: args.repoId } : {}),
        ...(args.worktreePath !== undefined ? { worktreePath: args.worktreePath } : {}),
        branch: requestedHead || currentBranch,
        base: submittedBase,
        hasUncommittedChanges: status.entries.length > 0,
        hasUpstream: upstreamStatus.hasUpstream,
        ahead: upstreamStatus.ahead,
        behind: upstreamStatus.behind,
        // Why: the create targets the submitted base verbatim, so enforce it
        // exists on the remote (orca hosted-review-creation.ts:76-77).
        enforceBaseOnRemote: true
      })
      // Why: an unavailable lookup might hide a real PR — refuse rather than
      // risk a duplicate (design invariant 8).
      if (eligibility.reviewLookupOutcome === 'unavailable') {
        return {
          ok: false,
          code: 'validation',
          error: `Create ${copy.shortLabel} failed: Orca could not confirm whether this branch already has a ${copy.reviewLabel}. Retry once the ${copy.providerName} lookup succeeds.`
        }
      }
      return blockedEligibilityToCreateResult(eligibility, submittedBase)
    } catch (error) {
      console.warn('Hosted review creation preflight failed:', error)
      return {
        ok: false,
        code: 'validation',
        error: `Create ${copy.shortLabel} failed: could not verify branch status. Refresh source control and try again.`
      }
    }
  }

  async function readPullRequestBody(worktreePath: string): Promise<string> {
    const readTemplate = deps.readTemplate
    if (!readTemplate) {
      return ''
    }
    for (const relativePath of PULL_REQUEST_TEMPLATE_PATHS) {
      try {
        const template = await readTemplate(worktreePath, relativePath)
        if (!template || template.isBinary === true) {
          continue
        }
        return template.content
      } catch {
        // Try the next conventional PR template path.
      }
    }
    return ''
  }

  // ── orca pull-request-template.ts:11-48 ────────────────────────────

  async function findOpenPRByHeadBase(
    repo: string,
    head: string,
    base: string
  ): Promise<{ number: number; url: string } | null> {
    try {
      const result = await deps.client.run([
        'pr',
        'list',
        '--repo',
        repo,
        '--head',
        head,
        '--base',
        base,
        '--state',
        'open',
        '--limit',
        '2',
        '--json',
        'number,url'
      ])
      if (result.code !== 0) {
        return null
      }
      const list = JSON.parse(result.stdout) as { number?: number; url?: string }[]
      if (!Array.isArray(list) || list.length !== 1 || !list[0]?.number || !list[0]?.url) {
        return null
      }
      return { number: list[0].number, url: list[0].url }
    } catch {
      return null
    }
  }

  // ── orca create-github-pull-request.ts:28-167 ──────────────────────

  async function createGitHubPullRequest(
    args: CreateHostedReviewArgs,
    origin: GitHubRepoIdentity,
    worktreePath: string
  ): Promise<CreateHostedReviewResult> {
    const base = normalizeHostedReviewBaseRef(args.base)
    const head = args.head ? normalizeHostedReviewHeadRef(args.head) || undefined : undefined
    const title = args.title.trim()
    if (!base || !title) {
      return {
        ok: false,
        code: 'validation',
        error: 'Create PR failed: base branch and title are required.'
      }
    }
    if (head && head.toLowerCase() === base.toLowerCase()) {
      return {
        ok: false,
        code: 'validation',
        error: 'Create PR failed: choose a different base branch before creating a pull request.'
      }
    }

    const body =
      args.useTemplate && !args.body?.trim()
        ? await readPullRequestBody(worktreePath)
        : (args.body ?? '')
    const repoArg = `${origin.owner}/${origin.repo}`
    const createArgs = ['pr', 'create', '--repo', repoArg, '--base', base, '--title', title, '--body-file', '-']
    if (head) {
      createArgs.push('--head', head)
    }
    if (args.draft && hostedReviewProviderSupportsDraft(args.provider)) {
      createArgs.push('--draft')
    }

    let result: GhExecResult
    try {
      // Why: a write that may have reached GitHub must not be re-issued
      // (orca passes idempotent: false for gh pr create).
      result = await deps.client.run(createArgs, { timeoutMs: 60_000, stdin: body, retry: false })
    } catch (error) {
      result = {
        stdout: '',
        stderr: error instanceof Error ? error.message : String(error),
        code: null
      }
    }
    if (result.code === 0) {
      const created = parseCreatePRPayload(result.stdout)
      if (created) {
        deps.reviewLookup.invalidate(args.repoPath)
        return { ok: true, number: created.number, url: created.url }
      }
      const found = head ? await findOpenPRByHeadBase(repoArg, head, base) : null
      if (found) {
        deps.reviewLookup.invalidate(args.repoPath)
        return { ok: true, number: found.number, url: found.url }
      }
      return {
        ok: false,
        code: 'unknown_completion',
        error: 'PR creation may have completed. Refreshing branch review state...'
      }
    }

    const classified = classifyCreatePRError(result)
    if (
      !classified.ok &&
      (classified.code === 'already_exists' || classified.code === 'unknown_completion') &&
      head
    ) {
      const existing = await findOpenPRByHeadBase(repoArg, head, base)
      if (existing) {
        return {
          ok: false,
          code: 'already_exists',
          error: 'A pull request already exists for this branch.',
          existingReview: existing
        }
      }
    }
    return classified
  }

  // ── orca hosted-review-creation.ts:241-287 + provider guard ────────

  async function create(args: CreateHostedReviewArgs): Promise<CreateHostedReviewResult> {
    const worktreePath = args.worktreePath?.trim() || args.repoPath
    if (!supportsHostedReviewCreation(args.provider)) {
      return {
        ok: false,
        code: 'unsupported_provider',
        error: 'Creating reviews for this provider is not supported yet.'
      }
    }
    // Why: creation targets the origin owning the unqualified head branch;
    // 2D.2 supports default-host GitHub only (spec §3.3 step 1).
    const origin = await deps.identity.getRepoSlug(worktreePath)
    if (!origin || args.provider !== 'github' || !isDefaultGitHubHost(origin.host)) {
      const copy = reviewCopy(args.provider)
      return {
        ok: false,
        code: 'unsupported_provider',
        error: `Creating ${copy.reviewLabel}s requires a ${copy.providerName} remote.`
      }
    }
    const blocked = await validateCurrentBranchCanCreateReview(args)
    if (blocked) {
      return blocked
    }
    return createGitHubPullRequest(args, origin, worktreePath)
  }

  return { getCreationEligibility, create }
}
