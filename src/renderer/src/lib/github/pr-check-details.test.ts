import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { createPRCheckDetailsClient } from './pr-check-details'
import { createGhExecClient } from './gh-exec-client'
import type { GhExecResult, GhExecutor } from './gh-exec-client'
import type { GitHubRepoIdentity } from './repo-identity'

type GhStep = GhExecResult | Error

function ghOk(stdout: unknown): GhExecResult {
  return {
    stdout: typeof stdout === 'string' ? stdout : JSON.stringify(stdout),
    stderr: '',
    code: 0
  }
}

function ghFail(stderr: string): GhExecResult {
  return { stdout: '', stderr, code: 1 }
}

const ORG_REPO: GitHubRepoIdentity = { owner: 'org', repo: 'repo' }
const TIMEOUT_OPTIONS = expect.objectContaining({ timeoutMs: 25_000 })

const CHECK_RUN = {
  id: 42,
  name: 'build',
  status: 'completed',
  conclusion: 'failure',
  html_url: 'https://github.com/org/repo/runs/42',
  details_url: 'https://github.com/org/repo/actions/runs/7',
  started_at: '2026-10-08T10:00:00Z',
  completed_at: '2026-10-08T10:05:00Z',
  output: { title: 'Build failed', summary: 'one test failed', text: 'details text' },
  check_suite: { workflow_run: { id: 7 } }
}

const ANNOTATIONS = [
  {
    path: 'src/a.ts',
    start_line: 3,
    end_line: 4,
    annotation_level: 'failure',
    title: 'lint',
    message: 'unused var',
    raw_details: 'raw'
  }
]

const SINGLE_JOB = {
  jobs: [
    {
      id: 100,
      name: 'build',
      status: 'completed',
      conclusion: 'failure',
      started_at: '2026-10-08T10:00:00Z',
      completed_at: '2026-10-08T10:05:00Z',
      html_url: 'https://github.com/org/repo/jobs/100',
      steps: [
        {
          name: 'compile',
          status: 'completed',
          conclusion: 'failure',
          started_at: '2026-10-08T10:00:10Z',
          completed_at: '2026-10-08T10:04:00Z'
        }
      ]
    }
  ]
}

const TWO_JOBS = {
  jobs: [
    ...SINGLE_JOB.jobs,
    {
      id: 101,
      name: 'release',
      status: 'completed',
      conclusion: 'success',
      started_at: '2026-10-08T10:05:00Z',
      completed_at: '2026-10-08T10:06:00Z',
      html_url: 'https://github.com/org/repo/jobs/101',
      steps: []
    }
  ]
}

function createHarness(options: { steps?: GhStep[] } = {}) {
  const steps = [...(options.steps ?? [])]
  const executor = vi.fn<GhExecutor>(async (args) => {
    const step = steps.shift()
    if (!step) {
      throw new Error(`unexpected gh call: ${args.join(' ')}`)
    }
    if (step instanceof Error) {
      throw step
    }
    return step
  })
  const details = createPRCheckDetailsClient({ client: createGhExecClient(executor) })
  return { details, executor }
}

describe('pr check details client', () => {
  beforeEach(() => {
    vi.spyOn(console, 'warn').mockImplementation(() => undefined)
  })

  afterEach(() => {
    vi.restoreAllMocks()
    vi.useRealTimers()
  })

  it('maps a check run with annotations and workflow jobs', async () => {
    const { details, executor } = createHarness({
      steps: [ghOk(CHECK_RUN), ghOk(ANNOTATIONS), ghOk(SINGLE_JOB)]
    })

    const result = await details.getPRCheckDetails({ repo: ORG_REPO, checkRunId: 42 })

    expect(executor).toHaveBeenNthCalledWith(
      1,
      ['api', 'repos/org/repo/check-runs/42'],
      TIMEOUT_OPTIONS
    )
    expect(executor).toHaveBeenNthCalledWith(
      2,
      ['api', 'repos/org/repo/check-runs/42/annotations?per_page=20'],
      TIMEOUT_OPTIONS
    )
    expect(executor).toHaveBeenNthCalledWith(
      3,
      ['api', 'repos/org/repo/actions/runs/7/jobs?per_page=100'],
      TIMEOUT_OPTIONS
    )
    expect(result).toEqual({
      name: 'build',
      status: 'completed',
      conclusion: 'failure',
      url: 'https://github.com/org/repo/runs/42',
      detailsUrl: 'https://github.com/org/repo/actions/runs/7',
      startedAt: '2026-10-08T10:00:00Z',
      completedAt: '2026-10-08T10:05:00Z',
      title: 'Build failed',
      summary: 'one test failed',
      text: 'details text',
      annotations: [
        {
          path: 'src/a.ts',
          startLine: 3,
          endLine: 4,
          annotationLevel: 'failure',
          title: 'lint',
          message: 'unused var',
          rawDetails: 'raw'
        }
      ],
      jobs: [
        {
          id: 100,
          name: 'build',
          status: 'completed',
          conclusion: 'failure',
          startedAt: '2026-10-08T10:00:00Z',
          completedAt: '2026-10-08T10:05:00Z',
          url: 'https://github.com/org/repo/jobs/100',
          logTail: null,
          steps: [
            {
              name: 'compile',
              status: 'completed',
              conclusion: 'failure',
              startedAt: '2026-10-08T10:00:10Z',
              completedAt: '2026-10-08T10:04:00Z'
            }
          ]
        }
      ]
    })
  })

  it('keeps jobs when the annotations fetch fails', async () => {
    const { details } = createHarness({
      steps: [ghOk(CHECK_RUN), ghFail('annotations unavailable'), ghOk(SINGLE_JOB)]
    })

    const result = await details.getPRCheckDetails({ repo: ORG_REPO, checkRunId: 42 })

    expect(result?.annotations).toEqual([])
    expect(result?.jobs).toHaveLength(1)
    expect(result?.jobs[0]?.logTail).toBeNull()
  })

  it('loads jobs directly when only a workflow run id is given', async () => {
    const { details, executor } = createHarness({ steps: [ghOk(SINGLE_JOB)] })

    const result = await details.getPRCheckDetails({
      repo: ORG_REPO,
      workflowRunId: 9,
      checkName: 'build'
    })

    expect(executor).toHaveBeenCalledTimes(1)
    expect(executor).toHaveBeenCalledWith(
      ['api', 'repos/org/repo/actions/runs/9/jobs?per_page=100'],
      TIMEOUT_OPTIONS
    )
    expect(result?.name).toBe('build')
    expect(result?.jobs).toHaveLength(1)
  })

  it('returns only the job exactly matching checkName', async () => {
    const { details } = createHarness({
      steps: [ghOk(CHECK_RUN), ghOk(ANNOTATIONS), ghOk(TWO_JOBS)]
    })

    const result = await details.getPRCheckDetails({
      repo: ORG_REPO,
      checkRunId: 42,
      checkName: 'build'
    })

    expect(result?.jobs.map((job) => job.name)).toEqual(['build'])
  })

  it('returns every job when no job matches checkName exactly', async () => {
    const { details } = createHarness({
      steps: [ghOk(CHECK_RUN), ghOk(ANNOTATIONS), ghOk(TWO_JOBS)]
    })

    const result = await details.getPRCheckDetails({
      repo: ORG_REPO,
      checkRunId: 42,
      checkName: 'missing'
    })

    expect(result?.jobs.map((job) => job.name)).toEqual(['build', 'release'])
  })

  it('throws the exact timeout message when the host deadline expires', async () => {
    vi.useFakeTimers()
    const never = new Promise<GhExecResult>(() => {})
    const executor = vi.fn<GhExecutor>(async () => never)
    const details = createPRCheckDetailsClient({ client: createGhExecClient(executor) })

    const promise = details.getPRCheckDetails({ repo: ORG_REPO, checkRunId: 42 })
    const expectation = expect(promise).rejects.toThrow('Timed out loading check details.')
    await vi.advanceTimersByTimeAsync(25_000)

    await expectation
    expect(executor).toHaveBeenCalledWith(
      ['api', 'repos/org/repo/check-runs/42'],
      expect.objectContaining({ timeoutMs: 25_000 })
    )
  })
})
