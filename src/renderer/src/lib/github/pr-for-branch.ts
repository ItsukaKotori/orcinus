import type { PRRefreshOutcome } from '../../../../shared/github/pull-request-refresh-types'
import type {
  GitHubPRMergeMethodSettings,
  GitHubPRStack,
  PRConflictSummary,
  PRInfo,
  PRMergeableState,
  PRReviewDecision
} from '../../../../shared/github/pull-request-types'
import { derivePRCheckStatusFromRollup } from '../../../../shared/pr-check-status'
import { isDefaultGitHubHost } from '../../../../shared/github/repository-identity-key'
import { classifyPRRefreshError, safePRRefreshErrorMessage } from './gh-error-classification'
import type { GhExecClient, GitHubRepoIdentity, RepoIdentityResolver } from './repo-identity'

export const PR_LOOKUP_JSON_FIELDS =
  'number,title,state,url,statusCheckRollup,updatedAt,isDraft,mergeable,reviewDecision,mergeStateStatus,autoMergeRequest,baseRefName,headRefName,baseRefOid,headRefOid'

export const PR_BRANCH_LIST_JSON_FIELDS =
  'number,title,state,url,statusCheckRollup,updatedAt,isDraft,mergeable,baseRefName,headRefName,baseRefOid,headRefOid'

export type PullRequestLookupData = {
  number: number
  title: string
  state: string
  url: string
  statusCheckRollup: unknown[]
  updatedAt: string
  isDraft?: boolean
  mergeable: string
  reviewDecision?: PRReviewDecision | null
  autoMergeRequest?: unknown
  autoMergeEnabled?: boolean
  autoMergeAllowed?: boolean | null
  mergeQueueRequired?: boolean | null
  mergeMethodSettings?: GitHubPRMergeMethodSettings
  mergeStateStatus?: string | null
  baseRefName?: string
  headRefName?: string
  baseRefOid?: string
  headRefOid?: string
  stack?: GitHubPRStack
  stackMetadataChecked?: boolean
}

export type RestPullRequest = {
  number: number
  title: string
  state: string
  html_url?: string
  url?: string
  updated_at?: string
  draft?: boolean
  merged_at?: string | null
  mergeable?: boolean | null
  mergeable_state?: string | null
  base?: { ref?: string; sha?: string }
  head?: { ref?: string; sha?: string }
  stack?: {
    number?: number
    position?: number
    size?: number
    base?: { ref?: string; sha?: string }
  } | null
}

export type PRForBranchLookupArgs = {
  worktreePath: string
  branch: string
  linkedPRNumber?: number | null
  fallbackPRNumber?: number | null
  acceptMergedFallbackPR?: boolean
  currentHeadOid?: string | null
}

export type PRConflictSummaryInput = {
  worktreePath: string
  baseRefName: string
  baseRefOid: string
  headRefOid: string
}

export type PRForBranchLookupDeps = {
  client: GhExecClient
  identity: Pick<RepoIdentityResolver, 'resolveCandidates'>
  /**
   * Local git-backed conflict snapshot. The renderer port has no git executor,
   * so 2D.1 leaves this unset and the conflict summary is omitted.
   */
  getConflictSummary?: (args: PRConflictSummaryInput) => Promise<PRConflictSummary | undefined>
}

export type PRForBranchLookup = {
  getPRForBranch: (args: PRForBranchLookupArgs) => Promise<PRInfo | null>
  getPRForBranchOutcome: (args: PRForBranchLookupArgs) => Promise<PRRefreshOutcome>
}

// Why: Task 3's port has no classified gh error union; only the not-found arm
// of orca's `classifyGhError` is observable here.
function extractExecError(err: unknown): { stderr: string; stdout: string } {
  if (err && typeof err === 'object') {
    const e = err as { stderr?: unknown; stdout?: unknown; message?: unknown }
    const stderr = typeof e.stderr === 'string' ? e.stderr : ''
    const stdout = typeof e.stdout === 'string' ? e.stdout : ''
    if (stderr || stdout) {
      return { stderr, stdout }
    }
    if (typeof e.message === 'string') {
      return { stderr: e.message, stdout: '' }
    }
  }
  return { stderr: String(err), stdout: '' }
}

export function isNoPullRequestError(err: unknown): boolean {
  const message = err instanceof Error ? err.message : String(err)
  const { stderr, stdout } = extractExecError(err)
  return /no pull requests? found|could not find.*pull request/i.test(
    `${message}\n${stderr}\n${stdout}`
  )
}

export function isNotFoundGhError(err: unknown): boolean {
  const stderr = extractExecError(err).stderr.toLowerCase()
  return stderr.includes('http 404') || stderr.includes('could not resolve to a repository')
}

export function shouldStopAfterExactLookupError(err: unknown): boolean {
  return !isNotFoundGhError(err)
}

/**
 * Detect a Retry-After hint in gh stderr and return the suggested delay in ms,
 * or null when the response includes no Retry-After. Ported verbatim from orca
 * `src/main/git/exec-error.ts:53-111`.
 */
export function parseRetryAfterMs(stderr: string): number | null {
  const raw = findRetryAfterHeaderValue(stderr)
  if (raw === null) {
    return null
  }
  if (/^\d+$/.test(raw)) {
    const seconds = Number(raw)
    return Number.isFinite(seconds) ? seconds * 1000 : null
  }
  const ts = Date.parse(raw)
  if (Number.isNaN(ts)) {
    return null
  }
  return Math.max(0, ts - Date.now())
}

function findRetryAfterHeaderValue(stderr: string): string | null {
  const headerIndex = indexOfAsciiIgnoreCase(stderr, 'retry-after:', 0)
  if (headerIndex === -1) {
    return null
  }
  let valueStart = headerIndex + 'retry-after:'.length
  while (valueStart < stderr.length) {
    const code = stderr.charCodeAt(valueStart)
    if (code !== 9 && code !== 32) {
      break
    }
    valueStart++
  }
  let valueEnd = valueStart
  while (valueEnd < stderr.length) {
    const code = stderr.charCodeAt(valueEnd)
    if (code === 10 || code === 13) {
      break
    }
    valueEnd++
  }
  const value = stderr.slice(valueStart, valueEnd).trim()
  return value.length > 0 ? value : null
}

function indexOfAsciiIgnoreCase(value: string, search: string, fromIndex: number): number {
  const lastStart = value.length - search.length
  for (let index = Math.max(0, fromIndex); index <= lastStart; index++) {
    let matches = true
    for (let offset = 0; offset < search.length; offset++) {
      const code = value.charCodeAt(index + offset)
      const normalizedCode = code >= 65 && code <= 90 ? code + 32 : code
      if (normalizedCode !== search.charCodeAt(offset)) {
        matches = false
        break
      }
    }
    if (matches) {
      return index
    }
  }
  return -1
}

export function prRefreshUpstreamError(
  err: unknown
): Extract<PRRefreshOutcome, { kind: 'upstream-error' }> {
  const errorType = classifyPRRefreshError(err)
  const outcome: Extract<PRRefreshOutcome, { kind: 'upstream-error' }> = {
    kind: 'upstream-error',
    errorType,
    message: safePRRefreshErrorMessage(errorType),
    fetchedAt: Date.now()
  }
  // Why: a Retry-After is a real cooldown — surface it as the retry schedule so the renderer doesn't retry into another 429.
  if (errorType === 'rate_limited') {
    const retryAfterMs = parseRetryAfterMs(extractExecError(err).stderr)
    if (retryAfterMs !== null && retryAfterMs > 0) {
      const retryAt = Date.now() + retryAfterMs
      outcome.nextAutoRetryAt = retryAt
      outcome.retryDisabledUntil = retryAt
    }
  }
  return outcome
}

// ── orca mappers.ts:112-124 (verbatim) ───────────────────────────────

export function mapPRState(state: string, isDraft?: boolean): PRInfo['state'] {
  const s = state?.toUpperCase()
  if (s === 'MERGED') {
    return 'merged'
  }
  if (s === 'CLOSED') {
    return 'closed'
  }
  if (isDraft) {
    return 'draft'
  }
  return 'open'
}

// ── orca work-item-field-coercion.ts:166-185 (verbatim) ──────────────

export function normalizePRMergeable(value: unknown): PRMergeableState | undefined {
  const raw = typeof value === 'string' ? value.toUpperCase() : ''
  if (raw === 'MERGEABLE' || raw === 'CONFLICTING' || raw === 'UNKNOWN') {
    return raw
  }
  if (typeof value === 'boolean') {
    return value ? 'MERGEABLE' : 'CONFLICTING'
  }
  return undefined
}

export function normalizeReviewDecision(value: unknown): PRReviewDecision | null {
  return value === 'APPROVED' || value === 'CHANGES_REQUESTED' || value === 'REVIEW_REQUIRED'
    ? value
    : null
}

export function isAutoMergeEnabled(value: unknown): boolean {
  return typeof value === 'object' && value !== null
}

// ── orca pull-request-lookup-data.ts:77-182 (verbatim; getCurrentHeadOid dropped) ──

export function mapRestPRMergeable(pr: RestPullRequest): PRMergeableState {
  const mergeableState = pr.mergeable_state?.toLowerCase()
  if (mergeableState === 'dirty') {
    return 'CONFLICTING'
  }
  if (mergeableState === 'clean' || pr.mergeable === true) {
    return 'MERGEABLE'
  }
  return 'UNKNOWN'
}

export function derivePullRequestMergeable(data: PullRequestLookupData): PRMergeableState {
  const mergeable = normalizePRMergeable(data.mergeable)
  if (mergeable === 'CONFLICTING' || data.mergeStateStatus === 'DIRTY') {
    return 'CONFLICTING'
  }
  return mergeable ?? 'UNKNOWN'
}

export function mapRestPullRequest(pr: RestPullRequest): PullRequestLookupData {
  const stack =
    typeof pr.stack?.number === 'number' &&
    typeof pr.stack.position === 'number' &&
    typeof pr.stack.size === 'number' &&
    typeof pr.stack.base?.ref === 'string'
      ? {
          number: pr.stack.number,
          position: pr.stack.position,
          size: pr.stack.size,
          baseRefName: pr.stack.base.ref,
          ...(typeof pr.stack.base.sha === 'string' ? { baseSha: pr.stack.base.sha } : {})
        }
      : undefined
  return {
    number: pr.number,
    title: pr.title,
    state: pr.merged_at ? 'MERGED' : pr.state,
    url: pr.html_url ?? pr.url ?? '',
    statusCheckRollup: [],
    updatedAt: pr.updated_at ?? '',
    isDraft: pr.draft,
    mergeable: mapRestPRMergeable(pr),
    baseRefName: pr.base?.ref,
    headRefName: pr.head?.ref,
    baseRefOid: pr.base?.sha,
    headRefOid: pr.head?.sha,
    stackMetadataChecked: true,
    ...(stack ? { stack } : {})
  }
}

export function isMergedImplicitPR(
  data: PullRequestLookupData,
  linkedPRNumber?: number | null
): boolean {
  // Why: a merged PR without an explicit link is just a historical branch match, not implicit review context.
  return typeof linkedPRNumber !== 'number' && mapPRState(data.state, data.isDraft) === 'merged'
}

export function shouldHideMergedImplicitPR(
  data: PullRequestLookupData | null,
  linkedPRNumber: number | null | undefined,
  currentHeadOid: string | null
): boolean {
  if (!data || !isMergedImplicitPR(data, linkedPRNumber)) {
    return false
  }
  // Why: keep hiding historical merged branch matches, but preserve the merged PR for the exact commit currently checked out.
  return !currentHeadOid || data.headRefOid !== currentHeadOid
}

export function normalizePullRequestLookupData(data: PullRequestLookupData): PullRequestLookupData {
  return {
    ...data,
    reviewDecision:
      data.reviewDecision !== undefined ? normalizeReviewDecision(data.reviewDecision) : undefined,
    autoMergeEnabled:
      data.autoMergeEnabled ??
      ('autoMergeRequest' in data ? isAutoMergeEnabled(data.autoMergeRequest) : undefined)
  }
}

export function createPRForBranchLookup(deps: PRForBranchLookupDeps): PRForBranchLookup {
  async function getRestPRForBranch(
    prRepo: GitHubRepoIdentity,
    headOwner: string,
    branchName: string
  ): Promise<PullRequestLookupData | null> {
    const head = encodeURIComponent(`${headOwner}:${branchName}`)
    const stdout = await deps.client.runOrThrow(
      ['api', `repos/${prRepo.owner}/${prRepo.repo}/pulls?head=${head}&state=all&per_page=1`],
      {}
    )
    const list = JSON.parse(stdout) as RestPullRequest[]
    const pr = list[0]
    return pr ? mapRestPullRequest(pr) : null
  }

  async function getFallbackPRListForBranch(
    prRepo: GitHubRepoIdentity,
    branchName: string
  ): Promise<PullRequestLookupData | null> {
    const stdout = await deps.client.runOrThrow(
      [
        'pr',
        'list',
        '--repo',
        `${prRepo.owner}/${prRepo.repo}`,
        '--head',
        branchName,
        '--state',
        'all',
        '--limit',
        '1',
        '--json',
        PR_BRANCH_LIST_JSON_FIELDS
      ],
      {}
    )
    const list = JSON.parse(stdout) as PullRequestLookupData[]
    return list[0] ?? null
  }

  async function getRestPRByNumber(
    ownerRepo: GitHubRepoIdentity,
    number: number
  ): Promise<PullRequestLookupData> {
    const stdout = await deps.client.runOrThrow(
      ['api', `repos/${ownerRepo.owner}/${ownerRepo.repo}/pulls/${number}`],
      {}
    )
    const parsed = JSON.parse(stdout) as unknown
    if (typeof parsed !== 'object' || parsed === null || Array.isArray(parsed)) {
      throw new Error('invalid response shape')
    }
    return mapRestPullRequest(parsed as RestPullRequest)
  }

  async function getPRByNumber(
    ownerRepo: GitHubRepoIdentity,
    number: number
  ): Promise<PullRequestLookupData | null> {
    try {
      const stdout = await deps.client.runOrThrow(
        [
          'pr',
          'view',
          String(number),
          '--repo',
          `${ownerRepo.owner}/${ownerRepo.repo}`,
          '--json',
          PR_LOOKUP_JSON_FIELDS
        ],
        {}
      )
      return normalizePullRequestLookupData(JSON.parse(stdout) as PullRequestLookupData)
    } catch (err) {
      // Why: deleted/edited linked PR metadata falls back to branch discovery; quota/auth/network failures get one cheaper REST exact lookup.
      if (isNotFoundGhError(err)) {
        return null
      }
      try {
        const restData = await getRestPRByNumber(ownerRepo, number)
        return normalizePullRequestLookupData(restData)
      } catch (restErr) {
        if (isNotFoundGhError(restErr)) {
          return null
        }
        if (!shouldStopAfterExactLookupError(restErr)) {
          return null
        }
        throw restErr
      }
    }
  }

  async function hydrateBranchLookupWithExactPR(
    ownerRepo: GitHubRepoIdentity,
    branchData: PullRequestLookupData | null
  ): Promise<PullRequestLookupData | null> {
    if (!branchData) {
      return null
    }
    try {
      return (await getPRByNumber(ownerRepo, branchData.number)) ?? branchData
    } catch {
      return branchData
    }
  }

  async function lookupPRByNumber(args: {
    candidates: GitHubRepoIdentity[]
    number: number
  }): Promise<{ data: PullRequestLookupData | null; dataRepo: GitHubRepoIdentity | null }> {
    for (const candidate of args.candidates) {
      try {
        const linkedData = await getPRByNumber(candidate, args.number)
        if (!linkedData) {
          continue
        }
        return { data: linkedData, dataRepo: candidate }
      } catch (err) {
        if (shouldStopAfterExactLookupError(err)) {
          throw err
        }
        // Candidate probing is best-effort; another repo may own the PR.
      }
    }
    return { data: null, dataRepo: null }
  }

  async function lookupPRByBranchName(args: {
    candidates: GitHubRepoIdentity[]
    headRepo: GitHubRepoIdentity | null
    branchName: string
  }): Promise<{
    data: PullRequestLookupData | null
    dataRepo: GitHubRepoIdentity | null
    pendingError?: unknown
  }> {
    let pendingError: unknown
    let hasPendingError = false
    for (const candidate of args.candidates) {
      try {
        // Why: with an unknown head owner, `gh pr list --head` matches fork PRs
        // by branch name where REST `owner:branch` cannot guess the owner.
        const branchData = args.headRepo
          ? await getRestPRForBranch(candidate, args.headRepo.owner, args.branchName)
          : await getFallbackPRListForBranch(candidate, args.branchName)
        // Why: REST branch lookup identifies the PR cheaply; exact `gh pr view` carries review and auto-merge state.
        const data = await hydrateBranchLookupWithExactPR(candidate, branchData)
        if (data) {
          return { data, dataRepo: candidate }
        }
      } catch (err) {
        if (args.headRepo) {
          throw err
        }
        if (!hasPendingError) {
          pendingError = err
          hasPendingError = true
        }
        try {
          const branchData = await getRestPRForBranch(candidate, candidate.owner, args.branchName)
          const data = await hydrateBranchLookupWithExactPR(candidate, branchData)
          if (data) {
            return { data, dataRepo: candidate }
          }
        } catch (retryErr) {
          if (!hasPendingError) {
            pendingError = retryErr
            hasPendingError = true
          }
        }
      }
    }
    // Why: branch-list failures are ambiguous for fork discovery; give exact fallback-number recovery a chance before surfacing the error.
    return hasPendingError
      ? { data: null, dataRepo: null, pendingError }
      : { data: null, dataRepo: null }
  }

  async function derivePRRefreshData(
    data: PullRequestLookupData,
    worktreePath: string
  ): Promise<{ mergeable: PRMergeableState; conflictSummary: PRConflictSummary | undefined }> {
    const mergeable = derivePullRequestMergeable(data)
    const conflictSummary =
      mergeable === 'CONFLICTING' && data.baseRefName && data.baseRefOid && data.headRefOid
        ? await deps.getConflictSummary?.({
            worktreePath,
            baseRefName: data.baseRefName,
            baseRefOid: data.baseRefOid,
            headRefOid: data.headRefOid
          })
        : undefined
    return { mergeable, conflictSummary }
  }

  function assemblePRRefreshFoundOutcome(args: {
    data: PullRequestLookupData
    dataRepo: GitHubRepoIdentity | null
    dataHeadRepo: GitHubRepoIdentity | null
    mergeable: PRMergeableState
    conflictSummary: PRConflictSummary | undefined
  }): PRRefreshOutcome {
    const { data, dataRepo, dataHeadRepo, mergeable, conflictSummary } = args
    return {
      kind: 'found',
      fetchedAt: Date.now(),
      pr: {
        number: data.number,
        title: data.title,
        state: mapPRState(data.state, data.isDraft),
        url: data.url,
        checksStatus: derivePRCheckStatusFromRollup(data.statusCheckRollup),
        updatedAt: data.updatedAt,
        mergeable,
        ...(data.reviewDecision !== undefined ? { reviewDecision: data.reviewDecision } : {}),
        ...(data.autoMergeEnabled !== undefined ? { autoMergeEnabled: data.autoMergeEnabled } : {}),
        ...(data.autoMergeAllowed !== undefined ? { autoMergeAllowed: data.autoMergeAllowed } : {}),
        ...(data.mergeQueueRequired !== undefined
          ? { mergeQueueRequired: data.mergeQueueRequired }
          : {}),
        ...(data.mergeMethodSettings !== undefined
          ? { mergeMethodSettings: data.mergeMethodSettings }
          : {}),
        ...(data.mergeStateStatus !== undefined ? { mergeStateStatus: data.mergeStateStatus } : {}),
        headSha: data.headRefOid,
        ...(data.baseRefName ? { baseRefName: data.baseRefName } : {}),
        ...(data.headRefName ? { headRefName: data.headRefName } : {}),
        prRepo: dataRepo ?? undefined,
        headRepo: dataHeadRepo ?? undefined,
        conflictSummary
      }
    }
  }

  async function resolveOutcome(args: PRForBranchLookupArgs): Promise<PRRefreshOutcome> {
    const branchName = args.branch.replace(/^refs\/heads\//, '')
    const linkedPRNumber = args.linkedPRNumber
    const fallbackPRNumber = args.fallbackPRNumber
    // Why: detached HEAD can't use branch lookup, but an exact linked/fallback PR number is still safe to query and keeps review state visible.
    if (!branchName && typeof linkedPRNumber !== 'number' && typeof fallbackPRNumber !== 'number') {
      return { kind: 'no-pr', fetchedAt: Date.now() }
    }
    const resolved = await deps.identity.resolveCandidates(args.worktreePath)
    // Why: GHES is identity-only in 2D.1 — gh_exec has no host parameter, so a
    // non-default-host candidate must never be queried as github.com (and its
    // owner must not be used as a github.com head owner).
    const candidates = resolved.candidates.filter((candidate) =>
      isDefaultGitHubHost(candidate.host)
    )
    const headRepo =
      resolved.headRepo && isDefaultGitHubHost(resolved.headRepo.host)
        ? resolved.headRepo
        : null
    let data: PullRequestLookupData | null = null
    let dataRepo: GitHubRepoIdentity | null = null
    const dataHeadRepo: GitHubRepoIdentity | null = headRepo
    let pendingBranchLookupError: unknown
    let hasPendingBranchLookupError = false
    let mergedBranchLookupNumber: number | null = null
    const explicitCurrentHeadOid =
      typeof args.currentHeadOid === 'string' && args.currentHeadOid.trim().length > 0
        ? args.currentHeadOid.trim()
        : null

    if (typeof linkedPRNumber === 'number') {
      const exactLookup = await lookupPRByNumber({ candidates, number: linkedPRNumber })
      data = exactLookup.data
      dataRepo = exactLookup.dataRepo
    } else if (branchName) {
      const branchLookup = await lookupPRByBranchName({ candidates, headRepo, branchName })
      data = branchLookup.data
      dataRepo = branchLookup.dataRepo
      if ('pendingError' in branchLookup) {
        pendingBranchLookupError = branchLookup.pendingError
        hasPendingBranchLookupError = true
      }
    }
    if (shouldHideMergedImplicitPR(data, linkedPRNumber, explicitCurrentHeadOid)) {
      mergedBranchLookupNumber = data?.number ?? null
      data = null
      dataRepo = null
    }
    if (!data && typeof linkedPRNumber !== 'number' && typeof fallbackPRNumber === 'number') {
      const fallbackLookup = await lookupPRByNumber({ candidates, number: fallbackPRNumber })
      data = fallbackLookup.data
      dataRepo = fallbackLookup.dataRepo
    }
    if (!data) {
      if (hasPendingBranchLookupError) {
        return prRefreshUpstreamError(pendingBranchLookupError)
      }
      return { kind: 'no-pr', fetchedAt: Date.now() }
    }
    const fallbackConfirmedMergedBranch =
      typeof fallbackPRNumber === 'number' &&
      mergedBranchLookupNumber === fallbackPRNumber &&
      data.number === fallbackPRNumber
    const explicitHeadHidesMergedImplicitPR =
      explicitCurrentHeadOid !== null &&
      shouldHideMergedImplicitPR(data, linkedPRNumber, explicitCurrentHeadOid)
    const shouldPreserveMergedFallback =
      !explicitHeadHidesMergedImplicitPR &&
      (fallbackConfirmedMergedBranch || args.acceptMergedFallbackPR === true)
    if (
      shouldHideMergedImplicitPR(data, linkedPRNumber, explicitCurrentHeadOid) &&
      !shouldPreserveMergedFallback
    ) {
      return { kind: 'no-pr', fetchedAt: Date.now() }
    }

    const { mergeable, conflictSummary } = await derivePRRefreshData(data, args.worktreePath)
    return assemblePRRefreshFoundOutcome({
      data,
      dataRepo,
      dataHeadRepo,
      mergeable,
      conflictSummary
    })
  }

  async function getPRForBranchOutcome(args: PRForBranchLookupArgs): Promise<PRRefreshOutcome> {
    try {
      return await resolveOutcome(args)
    } catch (err) {
      return prRefreshUpstreamError(err)
    }
  }

  async function getPRForBranch(args: PRForBranchLookupArgs): Promise<PRInfo | null> {
    const outcome = await getPRForBranchOutcome(args)
    return outcome.kind === 'found' ? outcome.pr : null
  }

  return { getPRForBranch, getPRForBranchOutcome }
}
