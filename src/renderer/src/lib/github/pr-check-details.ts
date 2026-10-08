import type { PRCheckRunDetails } from '../../../../shared/github/check-types'
import {
  GITHUB_CHECK_DETAILS_HOST_TIMEOUT_MS,
  GITHUB_CHECK_DETAILS_TIMEOUT_MESSAGE
} from '../../../../shared/github/check-details-deadline'
import type { GhExecOptions } from './gh-exec-client'
import type { GhExecClient, GitHubRepoIdentity } from './repo-identity'

// ── orca check-detail-field-mapping.ts:2-25 (verbatim) ───────────────

export function nullableString(value: unknown): string | null {
  return typeof value === 'string' && value.length > 0 ? value : null
}

export function nullableNumber(value: unknown): number | null {
  return typeof value === 'number' && Number.isFinite(value) ? value : null
}

export function mapCheckAnnotations(raw: unknown): PRCheckRunDetails['annotations'] {
  if (!Array.isArray(raw)) {
    return []
  }
  return raw
    .filter((annotation): annotation is Record<string, unknown> => Boolean(annotation))
    .map((annotation) => ({
      path: nullableString(annotation.path),
      startLine: nullableNumber(annotation.start_line),
      endLine: nullableNumber(annotation.end_line),
      annotationLevel: nullableString(annotation.annotation_level),
      title: nullableString(annotation.title),
      message: nullableString(annotation.message) ?? '',
      rawDetails: nullableString(annotation.raw_details)
    }))
}

export function mapWorkflowJobs(raw: unknown, checkName?: string): PRCheckRunDetails['jobs'] {
  if (!raw || typeof raw !== 'object' || !Array.isArray((raw as { jobs?: unknown }).jobs)) {
    return []
  }
  const jobs = (raw as { jobs: unknown[] }).jobs
    .filter((job): job is Record<string, unknown> => Boolean(job))
    .map((job) => ({
      id: nullableNumber(job.id),
      name: nullableString(job.name) ?? 'Unnamed job',
      status: nullableString(job.status),
      conclusion: nullableString(job.conclusion),
      startedAt: nullableString(job.started_at),
      completedAt: nullableString(job.completed_at),
      url: nullableString(job.html_url),
      logTail: null,
      steps: Array.isArray(job.steps)
        ? job.steps
            .filter((step): step is Record<string, unknown> => Boolean(step))
            .map((step) => ({
              name: nullableString(step.name) ?? 'Unnamed step',
              status: nullableString(step.status),
              conclusion: nullableString(step.conclusion),
              startedAt: nullableString(step.started_at),
              completedAt: nullableString(step.completed_at)
            }))
        : []
    }))
  const exactMatches = checkName ? jobs.filter((job) => job.name === checkName) : []
  return exactMatches.length > 0 ? exactMatches : jobs
}

export function getWorkflowRunIdFromCheckRun(
  checkRun: Record<string, unknown> | null
): number | undefined {
  const checkSuite = checkRun?.check_suite
  if (!checkSuite || typeof checkSuite !== 'object') {
    return undefined
  }
  const workflowRun = (checkSuite as { workflow_run?: unknown }).workflow_run
  if (!workflowRun || typeof workflowRun !== 'object') {
    return undefined
  }
  const id = (workflowRun as { id?: unknown }).id
  return typeof id === 'number' && Number.isSafeInteger(id) ? id : undefined
}

export type PRCheckDetailsArgs = {
  repo: GitHubRepoIdentity
  checkRunId?: number
  workflowRunId?: number
  checkName?: string
  url?: string
}

export type PRCheckDetailsClientDeps = {
  client: GhExecClient
}

export type PRCheckDetailsClient = {
  getPRCheckDetails: (args: PRCheckDetailsArgs) => Promise<PRCheckRunDetails | null>
}

/**
 * Renderer-side twin of orca's host deadline (get-pr-check-details.ts:38-41): the
 * exact timeout message must survive the race regardless of which gh call hung.
 */
export function withCheckDetailsHostDeadline<T>(work: () => Promise<T>): Promise<T> {
  return new Promise<T>((resolve, reject) => {
    const timer = setTimeout(
      () => reject(new Error(GITHUB_CHECK_DETAILS_TIMEOUT_MESSAGE)),
      GITHUB_CHECK_DETAILS_HOST_TIMEOUT_MS
    )
    work().then(
      (value) => {
        clearTimeout(timer)
        resolve(value)
      },
      (error: unknown) => {
        clearTimeout(timer)
        reject(error)
      }
    )
  })
}

export function createPRCheckDetailsClient(deps: PRCheckDetailsClientDeps): PRCheckDetailsClient {
  async function load(args: PRCheckDetailsArgs): Promise<PRCheckRunDetails> {
    const ghOptions: GhExecOptions = { timeoutMs: GITHUB_CHECK_DETAILS_HOST_TIMEOUT_MS }
    const repoPath = `repos/${args.repo.owner}/${args.repo.repo}`

    // ── orca get-pr-check-details.ts:55-95 (log tails dropped, spec §2.2) ──

    let checkRun: Record<string, unknown> | null = null
    let annotations: PRCheckRunDetails['annotations'] = []
    if (args.checkRunId) {
      const stdout = await deps.client.runOrThrow(
        ['api', `${repoPath}/check-runs/${args.checkRunId}`],
        ghOptions
      )
      checkRun = JSON.parse(stdout) as Record<string, unknown>
      try {
        const annotationsResult = await deps.client.runOrThrow(
          ['api', `${repoPath}/check-runs/${args.checkRunId}/annotations?per_page=20`],
          ghOptions
        )
        annotations = mapCheckAnnotations(JSON.parse(annotationsResult))
      } catch (err) {
        // Why: annotations are enrichment; a failure must not hide the check run or its jobs.
        console.warn('getPRCheckDetails annotations fetch failed:', err)
      }
    }

    const workflowRunId = args.workflowRunId ?? getWorkflowRunIdFromCheckRun(checkRun)
    let jobs: PRCheckRunDetails['jobs'] = []
    if (workflowRunId) {
      try {
        const jobsResult = await deps.client.runOrThrow(
          ['api', `${repoPath}/actions/runs/${workflowRunId}/jobs?per_page=100`],
          ghOptions
        )
        jobs = mapWorkflowJobs(JSON.parse(jobsResult), args.checkName)
      } catch (err) {
        console.warn('getPRCheckDetails workflow jobs fetch failed:', err)
      }
    }

    const output =
      checkRun?.output && typeof checkRun.output === 'object'
        ? (checkRun.output as Record<string, unknown>)
        : null
    return {
      name: nullableString(checkRun?.name) ?? args.checkName ?? 'Check',
      status: nullableString(checkRun?.status),
      conclusion: nullableString(checkRun?.conclusion),
      url: nullableString(checkRun?.html_url) ?? args.url ?? null,
      detailsUrl: nullableString(checkRun?.details_url) ?? args.url ?? null,
      startedAt: nullableString(checkRun?.started_at),
      completedAt: nullableString(checkRun?.completed_at),
      title: nullableString(output?.title),
      summary: nullableString(output?.summary),
      text: nullableString(output?.text),
      annotations,
      jobs
    }
  }

  async function getPRCheckDetails(
    args: PRCheckDetailsArgs
  ): Promise<PRCheckRunDetails | null> {
    try {
      return await withCheckDetailsHostDeadline(() => load(args))
    } catch (err) {
      console.warn('getPRCheckDetails failed:', err)
      throw err
    }
  }

  return { getPRCheckDetails }
}
