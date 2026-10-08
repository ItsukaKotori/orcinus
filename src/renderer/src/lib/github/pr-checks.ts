import type { PRCheckDetail } from '../../../../shared/github/check-types'
import { GhRunError } from './gh-exec-client'
import {
  createPRCheckDetailsClient,
  nullableNumber,
  nullableString
} from './pr-check-details'
import type { PRCheckDetailsClient } from './pr-check-details'
import type { GhExecClient, GitHubRepoIdentity, RepoIdentityResolver } from './repo-identity'

// ── orca check/pr-checks-graphql-query.ts:1-133 (verbatim) ───────────

export const PR_CHECKS_ROLLUP_QUERY = `
query($owner: String!, $repo: String!, $pr: Int!) {
  repository(owner: $owner, name: $repo) {
    pullRequest(number: $pr) {
      headRefOid
      commits(last: 1) {
        nodes {
          commit {
            statusCheckRollup {
              contexts(first: 100) {
                nodes {
                  __typename
                  ... on CheckRun {
                    databaseId
                    name
                    status
                    conclusion
                    detailsUrl
                    url
                    checkSuite {
                      databaseId
                      workflowRun {
                        databaseId
                      }
                    }
                  }
                  ... on StatusContext {
                    context
                    state
                    targetUrl
                  }
                }
              }
            }
            checkSuites(first: 100) {
              nodes {
                databaseId
                status
                conclusion
                url
                app {
                  name
                  slug
                }
              }
            }
          }
        }
      }
    }
  }
}
`

export type GraphQLPRChecksResponse = {
  data?: {
    repository?: {
      pullRequest?: {
        headRefOid?: string | null
        commits?: {
          nodes?: { commit?: GraphQLPRChecksCommit | null }[] | null
        } | null
      } | null
    } | null
  } | null
}

export type GraphQLPRChecksCommit = {
  statusCheckRollup?: {
    contexts?: {
      nodes?: GraphQLStatusCheckContext[] | null
    } | null
  } | null
  checkSuites?: {
    nodes?: GraphQLCheckSuite[] | null
  } | null
}

export type GraphQLCheckRunContext = {
  __typename: 'CheckRun'
  databaseId?: number | null
  name?: string | null
  status?: string | null
  conclusion?: string | null
  detailsUrl?: string | null
  url?: string | null
  checkSuite?: {
    databaseId?: number | null
    workflowRun?: { databaseId?: number | null } | null
  } | null
}

export type GraphQLStatusContext = {
  __typename: 'StatusContext'
  context?: string | null
  state?: string | null
  targetUrl?: string | null
}

export type GraphQLStatusCheckContext =
  | GraphQLCheckRunContext
  | GraphQLStatusContext
  | { __typename?: string | null }

export type GraphQLCheckSuite = {
  databaseId?: number | null
  status?: string | null
  conclusion?: string | null
  url?: string | null
  app?: { name?: string | null; slug?: string | null } | null
}

export type RestCheckRun = {
  id?: number
  name: string
  status: string
  conclusion: string | null
  html_url: string
  details_url: string | null
}

export type RestCommitStatus = {
  context?: string
  state?: string
  target_url?: string | null
}

export type RestCheckSuite = {
  id?: number | null
  status: string | null
  conclusion: string | null
  app?: { name?: string | null; slug?: string | null } | null
}

// ── orca mappers.ts:9-110 (verbatim; conclusionMap exported for tests) ──

export function mapCheckRunRESTStatus(status: string): PRCheckDetail['status'] {
  const s = status?.toLowerCase()
  if (s === 'queued') {
    return 'queued'
  }
  if (s === 'in_progress') {
    return 'in_progress'
  }
  return 'completed'
}

export const conclusionMap: Record<string, PRCheckDetail['conclusion']> = {
  success: 'success',
  failure: 'failure',
  cancelled: 'cancelled',
  timed_out: 'timed_out',
  skipped: 'skipped',
  neutral: 'neutral',
  action_required: 'action_required',
  stale: 'failure',
  startup_failure: 'failure'
}

export function mapCheckRunRESTConclusion(
  status: string,
  conclusion: string | null
): PRCheckDetail['conclusion'] {
  if (status?.toLowerCase() !== 'completed') {
    return 'pending'
  }
  if (!conclusion) {
    return null
  }
  return conclusionMap[conclusion.toLowerCase()] ?? null
}

export function mapCommitStatusRESTStatus(state: string): PRCheckDetail['status'] {
  const s = state?.toLowerCase()
  return s === 'pending' ? 'queued' : 'completed'
}

export function mapCommitStatusRESTConclusion(state: string): PRCheckDetail['conclusion'] {
  const s = state?.toLowerCase()
  if (s === 'success') {
    return 'success'
  }
  if (s === 'failure' || s === 'error') {
    return 'failure'
  }
  if (s === 'pending') {
    return 'pending'
  }
  return null
}

export function mapCheckStatus(state: string): PRCheckDetail['status'] {
  const s = state?.toUpperCase()
  if (s === 'PENDING' || s === 'QUEUED') {
    return 'queued'
  }
  if (s === 'IN_PROGRESS') {
    return 'in_progress'
  }
  return 'completed'
}

export function mapCheckConclusion(state: string): PRCheckDetail['conclusion'] {
  const s = state?.toUpperCase()
  if (s === 'SUCCESS' || s === 'PASS') {
    return 'success'
  }
  if (s === 'FAILURE' || s === 'FAIL') {
    return 'failure'
  }
  if (s === 'ACTION_REQUIRED') {
    return 'action_required'
  }
  if (s === 'STALE' || s === 'STARTUP_FAILURE') {
    return 'failure'
  }
  if (s === 'CANCELLED') {
    return 'cancelled'
  }
  if (s === 'TIMED_OUT') {
    return 'timed_out'
  }
  if (s === 'SKIPPED') {
    return 'skipped'
  }
  if (s === 'PENDING' || s === 'QUEUED' || s === 'IN_PROGRESS') {
    return 'pending'
  }
  if (s === 'NEUTRAL') {
    return 'neutral'
  }
  return null
}

// ── orca check-detail-field-mapping.ts:73-83 (verbatim) ──────────────

export function parseActionsRunId(url: string | null | undefined): number | undefined {
  if (!url) {
    return undefined
  }
  const match = /\/actions\/runs\/(\d+)(?:[/?#]|$)/.exec(url)
  if (!match) {
    return undefined
  }
  const id = Number(match[1])
  return Number.isSafeInteger(id) ? id : undefined
}

// ── orca github-repository-host.ts:9-11 (verbatim) ───────────────────

function githubRepositoryWebHost(repository: GitHubRepoIdentity): string {
  return repository.host ?? 'github.com'
}

// ── orca check/pr-checks-response-mapping.ts:19-186 (verbatim) ───────

export function isGraphQLCheckRunContext(
  context: GraphQLStatusCheckContext
): context is GraphQLCheckRunContext {
  return context.__typename === 'CheckRun'
}

export function isGraphQLStatusContext(
  context: GraphQLStatusCheckContext
): context is GraphQLStatusContext {
  return context.__typename === 'StatusContext'
}

export function mapGraphQLCheckRunContext(context: GraphQLCheckRunContext): PRCheckDetail | null {
  const name = nullableString(context.name)
  if (!name) {
    return null
  }
  const url = nullableString(context.detailsUrl) ?? nullableString(context.url)
  const checkRunId = nullableNumber(context.databaseId)
  const workflowRunId =
    nullableNumber(context.checkSuite?.workflowRun?.databaseId) ?? parseActionsRunId(url)
  return {
    name,
    status: mapCheckRunRESTStatus(context.status ?? ''),
    conclusion: mapCheckRunRESTConclusion(context.status ?? '', context.conclusion ?? null),
    url,
    ...(checkRunId !== null ? { checkRunId } : {}),
    ...(typeof workflowRunId === 'number' ? { workflowRunId } : {})
  }
}

export function mapGraphQLStatusContext(context: GraphQLStatusContext): PRCheckDetail | null {
  const name = nullableString(context.context)
  if (!name) {
    return null
  }
  const url = nullableString(context.targetUrl)
  const workflowRunId = parseActionsRunId(url)
  return {
    name,
    status: mapCommitStatusRESTStatus(context.state ?? ''),
    conclusion: mapCommitStatusRESTConclusion(context.state ?? ''),
    url,
    ...(workflowRunId !== undefined ? { workflowRunId } : {})
  }
}

export function mapRestCheckRun(checkRun: RestCheckRun): PRCheckDetail {
  const workflowRunId = parseActionsRunId(checkRun.details_url || checkRun.html_url || null)
  return {
    name: checkRun.name,
    status: mapCheckRunRESTStatus(checkRun.status),
    conclusion: mapCheckRunRESTConclusion(checkRun.status, checkRun.conclusion),
    url: checkRun.details_url || checkRun.html_url || null,
    ...(typeof checkRun.id === 'number' ? { checkRunId: checkRun.id } : {}),
    ...(workflowRunId !== undefined ? { workflowRunId } : {})
  }
}

export function mapRestCommitStatus(status: RestCommitStatus): PRCheckDetail | null {
  const name = nullableString(status.context)
  if (!name) {
    return null
  }
  const url = nullableString(status.target_url)
  const workflowRunId = parseActionsRunId(url)
  return {
    name,
    status: mapCommitStatusRESTStatus(status.state ?? ''),
    conclusion: mapCommitStatusRESTConclusion(status.state ?? ''),
    url,
    ...(workflowRunId !== undefined ? { workflowRunId } : {})
  }
}

export function getPendingApprovalCheckSuiteName(
  suite: {
    id?: number | null
    databaseId?: number | null
    app?: { name?: string | null; slug?: string | null } | null
  },
  headSha: string | null | undefined,
  index: number
): string {
  const appName = suite.app?.name ?? suite.app?.slug ?? null
  const rawSuiteId = suite.databaseId ?? suite.id
  const suiteId =
    typeof rawSuiteId === 'number' && Number.isFinite(rawSuiteId) ? `#${rawSuiteId}` : null
  if (appName && suiteId) {
    return `${appName} ${suiteId}`
  }
  if (appName) {
    return appName
  }
  if (suiteId) {
    return suiteId
  }
  return `${headSha?.slice(0, 12) ?? 'check-suite'}:${index + 1}`
}

export function getPendingApprovalCheckSuiteUrl(
  ownerRepo: GitHubRepoIdentity,
  headSha: string,
  suiteId: number | null | undefined
): string {
  const base = `https://${githubRepositoryWebHost(ownerRepo)}/${ownerRepo.owner}/${ownerRepo.repo}/commits/${headSha}/checks`
  return typeof suiteId === 'number' && Number.isFinite(suiteId)
    ? `${base}#check-suite-${suiteId}`
    : base
}

export function mapGraphQLPendingApprovalCheckSuite(
  ownerRepo: GitHubRepoIdentity,
  suite: GraphQLCheckSuite,
  headSha: string | null | undefined,
  index: number
): PRCheckDetail {
  return {
    name: getPendingApprovalCheckSuiteName(suite, headSha, index),
    status: 'completed',
    conclusion: 'action_required',
    // Why: suite-only approval blockers have no check run; link the suite page when GraphQL exposes one.
    url:
      nullableString(suite.url) ??
      (headSha ? getPendingApprovalCheckSuiteUrl(ownerRepo, headSha, suite.databaseId) : null)
  }
}

export function mapGraphQLPRChecksResponse(
  ownerRepo: GitHubRepoIdentity,
  response: GraphQLPRChecksResponse
): PRCheckDetail[] | null {
  const pullRequest = response.data?.repository?.pullRequest
  if (!pullRequest) {
    return null
  }
  const commit = pullRequest.commits?.nodes?.[0]?.commit
  if (!commit) {
    return []
  }

  const contexts = commit.statusCheckRollup?.contexts?.nodes ?? []
  const checkRunContexts = contexts.filter(isGraphQLCheckRunContext)
  const checkRuns = checkRunContexts
    .map(mapGraphQLCheckRunContext)
    .filter((check): check is PRCheckDetail => check !== null)
  const checkRunNames = new Set(checkRuns.map((check) => check.name))
  const checkSuiteIdsWithRuns = new Set(
    checkRunContexts
      .map((context) => nullableNumber(context.checkSuite?.databaseId))
      .filter((id): id is number => id !== null)
  )
  // Why: mixed-CI repos expose Jenkins/Prow/Tide as legacy status contexts in the same rollup; keep check-run metadata on name collisions.
  const legacyStatuses = contexts
    .filter(isGraphQLStatusContext)
    .map(mapGraphQLStatusContext)
    .filter((check): check is PRCheckDetail => check !== null && !checkRunNames.has(check.name))
  const pendingApprovalChecks = (commit.checkSuites?.nodes ?? [])
    .filter((suite) => suite.conclusion?.toLowerCase() === 'action_required')
    .filter((suite) => {
      const suiteId = nullableNumber(suite.databaseId)
      return suiteId === null || !checkSuiteIdsWithRuns.has(suiteId)
    })
    .map((suite, index) =>
      mapGraphQLPendingApprovalCheckSuite(ownerRepo, suite, pullRequest.headRefOid, index)
    )

  return [...checkRuns, ...legacyStatuses, ...pendingApprovalChecks]
}

export type PRChecksArgs = {
  repo: GitHubRepoIdentity
  prNumber: number
  headSha?: string
  noCache?: boolean
}

export type PRChecksClientDeps = {
  client: GhExecClient
  /**
   * Accepted for wiring parity with the plan; checks args carry an explicit repo
   * identity, so no worktree lookup happens here.
   */
  identity: Pick<RepoIdentityResolver, 'resolveCandidates'>
}

export type PRChecksClient = {
  getPRChecks: (args: PRChecksArgs) => Promise<PRCheckDetail[]>
  getPRCheckDetails: PRCheckDetailsClient['getPRCheckDetails']
}

function isNoChecksReportedError(err: unknown): boolean {
  const stderr = err instanceof GhRunError ? err.stderr : err instanceof Error ? err.message : ''
  return stderr.toLowerCase().includes('no checks reported')
}

export function createPRChecksClient(deps: PRChecksClientDeps): PRChecksClient {
  const details = createPRCheckDetailsClient({ client: deps.client })

  // ── orca get-pr-checks.ts:25-116 (rate-limit/acquire dropped) ───────

  async function getPRChecksViaRestFallback(
    repo: GitHubRepoIdentity,
    headSha: string | undefined,
    noCache?: boolean
  ): Promise<PRCheckDetail[] | null> {
    if (!headSha) {
      return null
    }
    const cacheArgs = noCache ? [] : ['--cache', '60s']
    const encodedHeadSha = encodeURIComponent(headSha)
    const repoPath = `repos/${repo.owner}/${repo.repo}/commits/${encodedHeadSha}`
    try {
      const checkRunStdout = await deps.client.runOrThrow(
        ['api', ...cacheArgs, `${repoPath}/check-runs?per_page=100`],
        {}
      )
      const checkRunData = JSON.parse(checkRunStdout) as {
        check_runs?: RestCheckRun[]
      }
      const checkRuns = (checkRunData.check_runs ?? []).map(mapRestCheckRun)
      const checkRunNames = new Set(checkRuns.map((check) => check.name))

      let legacyStatuses: PRCheckDetail[] = []
      try {
        const statusStdout = await deps.client.runOrThrow(
          ['api', ...cacheArgs, `${repoPath}/status?per_page=100`],
          {}
        )
        const statusData = JSON.parse(statusStdout) as {
          statuses?: RestCommitStatus[]
        }
        legacyStatuses = (statusData.statuses ?? [])
          .map(mapRestCommitStatus)
          .filter(
            (check): check is PRCheckDetail => check !== null && !checkRunNames.has(check.name)
          )
      } catch (err) {
        // Why: REST fallback is already degraded; keep the richer check-run rows if legacy-status enrichment fails.
        console.warn('getPRChecks REST status fallback failed:', err)
      }

      let pendingApprovalChecks: PRCheckDetail[] = []
      try {
        const suitesStdout = await deps.client.runOrThrow(
          ['api', ...cacheArgs, `${repoPath}/check-suites?per_page=100`],
          {}
        )
        const suitesData = JSON.parse(suitesStdout) as {
          check_suites?: RestCheckSuite[]
        }
        pendingApprovalChecks = (suitesData.check_suites ?? [])
          .filter((suite) => suite.conclusion?.toLowerCase() === 'action_required')
          .map((suite, index) => ({
            name: getPendingApprovalCheckSuiteName(suite, headSha, index),
            status: 'completed' as const,
            conclusion: 'action_required' as const,
            url: getPendingApprovalCheckSuiteUrl(repo, headSha, suite.id)
          }))
      } catch (err) {
        console.warn('getPRChecks REST check-suite fallback failed:', err)
      }

      const checks = [...checkRuns, ...legacyStatuses, ...pendingApprovalChecks]
      return checks.length > 0 ? checks : null
    } catch (err) {
      console.warn('getPRChecks via REST fallback failed, falling back to gh pr checks:', err)
      return null
    }
  }

  // ── orca get-pr-checks.ts:146-166 (rate-limit/acquire dropped) ─────

  async function fallbackToPRChecks(
    repo: GitHubRepoIdentity,
    prNumber: number
  ): Promise<PRCheckDetail[]> {
    const fallbackArgs = ['pr', 'checks', String(prNumber), '--json', 'name,state,link']
    fallbackArgs.push('--repo', `${repo.owner}/${repo.repo}`)
    try {
      const stdout = await deps.client.runOrThrow(fallbackArgs, {})
      const data = JSON.parse(stdout) as { name: string; state: string; link: string }[]
      return data.map((d) => {
        const workflowRunId = parseActionsRunId(d.link)
        return {
          name: d.name,
          status: mapCheckStatus(d.state),
          conclusion: mapCheckConclusion(d.state),
          url: d.link || null,
          ...(workflowRunId !== undefined ? { workflowRunId } : {})
        }
      })
    } catch (err) {
      // Why: `gh pr checks` exits non-zero when a PR has no check runs yet; treat that as empty, not a load failure.
      if (isNoChecksReportedError(err)) {
        return []
      }
      throw err
    }
  }

  async function getPRChecks(args: PRChecksArgs): Promise<PRCheckDetail[]> {
    const repo = args.repo
    try {
      // Why: --cache 60s saves rate-limit budget during polling; explicit refresh skips it for fresh data.
      const cacheArgs = args.noCache ? [] : ['--cache', '60s']
      const stdout = await deps.client.runOrThrow(
        [
          'api',
          'graphql',
          ...cacheArgs,
          '-f',
          `owner=${repo.owner}`,
          '-f',
          `repo=${repo.repo}`,
          '-F',
          `pr=${args.prNumber}`,
          '-f',
          `query=${PR_CHECKS_ROLLUP_QUERY}`
        ],
        {}
      )
      const checks = mapGraphQLPRChecksResponse(
        repo,
        JSON.parse(stdout) as GraphQLPRChecksResponse
      )
      if (checks !== null) {
        return checks
      }
    } catch (err) {
      // Why: fall back to older `gh pr checks` when GitHub's richer rollup query is unavailable.
      console.warn('getPRChecks via GraphQL rollup failed, falling back to gh pr checks:', err)
    }

    const restChecks = await getPRChecksViaRestFallback(repo, args.headSha, args.noCache)
    if (restChecks !== null) {
      return restChecks
    }

    try {
      return await fallbackToPRChecks(repo, args.prNumber)
    } catch (err) {
      console.warn('getPRChecks failed:', err)
      throw err
    }
  }

  return { getPRChecks, getPRCheckDetails: details.getPRCheckDetails }
}
