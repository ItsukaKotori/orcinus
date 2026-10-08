import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import {
  PR_CHECKS_ROLLUP_QUERY,
  conclusionMap,
  createPRChecksClient,
  getPendingApprovalCheckSuiteName,
  mapCheckConclusion,
  mapCheckRunRESTConclusion,
  mapCheckRunRESTStatus,
  mapCheckStatus,
  mapCommitStatusRESTConclusion,
  mapCommitStatusRESTStatus
} from './pr-checks'
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

const GRAPHQL_ARGV = [
  'api',
  'graphql',
  '--cache',
  '60s',
  '-f',
  'owner=org',
  '-f',
  'repo=repo',
  '-F',
  'pr=7',
  '-f',
  `query=${PR_CHECKS_ROLLUP_QUERY}`
]

const GRAPHQL_CHECKS_RESPONSE = {
  data: {
    repository: {
      pullRequest: {
        headRefOid: 'head-oid',
        commits: {
          nodes: [
            {
              commit: {
                statusCheckRollup: {
                  contexts: {
                    nodes: [
                      {
                        __typename: 'CheckRun',
                        databaseId: 88,
                        name: 'build',
                        status: 'COMPLETED',
                        conclusion: 'SUCCESS',
                        detailsUrl: 'https://github.com/org/repo/actions/runs/5',
                        url: 'https://github.com/org/repo/runs/88',
                        checkSuite: { databaseId: 1000, workflowRun: { databaseId: 5 } }
                      },
                      {
                        __typename: 'StatusContext',
                        context: 'ci/legacy',
                        state: 'PENDING',
                        targetUrl: 'https://jenkins.example.com/job/1'
                      },
                      {
                        __typename: 'StatusContext',
                        context: 'build',
                        state: 'SUCCESS',
                        targetUrl: 'https://example.com/duplicate-build'
                      }
                    ]
                  }
                },
                checkSuites: {
                  nodes: [
                    {
                      databaseId: 1000,
                      status: 'COMPLETED',
                      conclusion: 'ACTION_REQUIRED',
                      url: null,
                      app: { name: 'GitHub Actions', slug: 'github-actions' }
                    },
                    {
                      databaseId: 1001,
                      status: 'COMPLETED',
                      conclusion: 'ACTION_REQUIRED',
                      url: null,
                      app: { name: 'GitHub Actions', slug: 'github-actions' }
                    }
                  ]
                }
              }
            }
          ]
        }
      }
    }
  }
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
  const checks = createPRChecksClient({
    client: createGhExecClient(executor),
    identity: {
      resolveCandidates: vi.fn(async () => ({ candidates: [ORG_REPO], headRepo: ORG_REPO }))
    }
  })
  return { checks, executor }
}

describe('pr checks client', () => {
  beforeEach(() => {
    vi.spyOn(console, 'warn').mockImplementation(() => undefined)
  })

  afterEach(() => {
    vi.restoreAllMocks()
    vi.useRealTimers()
  })

  it('maps the cached GraphQL rollup into check rows', async () => {
    const { checks, executor } = createHarness({ steps: [ghOk(GRAPHQL_CHECKS_RESPONSE)] })

    const result = await checks.getPRChecks({ repo: ORG_REPO, prNumber: 7, headSha: 'head-oid' })

    expect(executor).toHaveBeenCalledWith(GRAPHQL_ARGV, expect.anything())
    expect(executor).toHaveBeenCalledTimes(1)
    expect(result).toEqual([
      {
        name: 'build',
        status: 'completed',
        conclusion: 'success',
        url: 'https://github.com/org/repo/actions/runs/5',
        checkRunId: 88,
        workflowRunId: 5
      },
      {
        name: 'ci/legacy',
        status: 'queued',
        conclusion: 'pending',
        url: 'https://jenkins.example.com/job/1'
      },
      {
        name: 'GitHub Actions #1001',
        status: 'completed',
        conclusion: 'action_required',
        url: 'https://github.com/org/repo/commits/head-oid/checks#check-suite-1001'
      }
    ])
    expect('workflowRunId' in result[1]).toBe(false)
  })

  it('omits --cache when noCache is set', async () => {
    const { checks, executor } = createHarness({
      steps: [
        ghOk({
          data: {
            repository: {
              pullRequest: { headRefOid: 'head-oid', commits: { nodes: [{ commit: {} }] } }
            }
          }
        })
      ]
    })

    await checks.getPRChecks({ repo: ORG_REPO, prNumber: 7, noCache: true })

    const graphQLArgs = executor.mock.calls[0]?.[0] as string[]
    expect(graphQLArgs).toEqual([
      'api',
      'graphql',
      '-f',
      'owner=org',
      '-f',
      'repo=repo',
      '-F',
      'pr=7',
      '-f',
      `query=${PR_CHECKS_ROLLUP_QUERY}`
    ])
    expect(graphQLArgs).not.toContain('--cache')
  })

  it('synthesizes suite links with the repository host', async () => {
    const ghesRepo: GitHubRepoIdentity = { owner: 'org', repo: 'repo', host: 'ghe.example.com' }
    const { checks } = createHarness({
      steps: [
        ghOk({
          data: {
            repository: {
              pullRequest: {
                headRefOid: 'head-oid',
                commits: {
                  nodes: [
                    {
                      commit: {
                        checkSuites: {
                          nodes: [
                            {
                              databaseId: 1001,
                              status: 'COMPLETED',
                              conclusion: 'ACTION_REQUIRED',
                              url: null,
                              app: { name: 'GitHub Actions', slug: 'github-actions' }
                            }
                          ]
                        }
                      }
                    }
                  ]
                }
              }
            }
          }
        })
      ]
    })

    const result = await checks.getPRChecks({ repo: ghesRepo, prNumber: 7 })

    expect(result).toEqual([
      {
        name: 'GitHub Actions #1001',
        status: 'completed',
        conclusion: 'action_required',
        url: 'https://ghe.example.com/org/repo/commits/head-oid/checks#check-suite-1001'
      }
    ])
  })

  it('falls back to the REST trio when the GraphQL rollup fails', async () => {
    const { checks, executor } = createHarness({
      steps: [
        new Error('GraphQL rollup failed'),
        ghOk({
          check_runs: [
            {
              id: 88,
              name: 'Summary',
              status: 'completed',
              conclusion: 'success',
              html_url: 'https://github.com/org/repo/actions/runs/88',
              details_url: null
            }
          ]
        }),
        ghOk({
          statuses: [
            {
              context: 'Summary',
              state: 'success',
              target_url: 'https://example.com/duplicate-summary'
            },
            {
              context: 'ci/legacy',
              state: 'pending',
              target_url: 'https://jenkins.example.com/job/1'
            }
          ]
        }),
        ghOk({
          check_suites: [
            {
              id: 1001,
              status: 'completed',
              conclusion: 'action_required',
              app: { name: 'GitHub Actions', slug: 'github-actions' }
            },
            { id: 1002, status: 'completed', conclusion: 'success', app: null }
          ]
        })
      ]
    })

    const result = await checks.getPRChecks({ repo: ORG_REPO, prNumber: 7, headSha: 'head-oid' })

    expect(executor).toHaveBeenNthCalledWith(
      2,
      ['api', '--cache', '60s', 'repos/org/repo/commits/head-oid/check-runs?per_page=100'],
      expect.anything()
    )
    expect(executor).toHaveBeenNthCalledWith(
      3,
      ['api', '--cache', '60s', 'repos/org/repo/commits/head-oid/status?per_page=100'],
      expect.anything()
    )
    expect(executor).toHaveBeenNthCalledWith(
      4,
      ['api', '--cache', '60s', 'repos/org/repo/commits/head-oid/check-suites?per_page=100'],
      expect.anything()
    )
    expect(result).toEqual([
      {
        name: 'Summary',
        status: 'completed',
        conclusion: 'success',
        url: 'https://github.com/org/repo/actions/runs/88',
        checkRunId: 88,
        workflowRunId: 88
      },
      {
        name: 'ci/legacy',
        status: 'queued',
        conclusion: 'pending',
        url: 'https://jenkins.example.com/job/1'
      },
      {
        name: 'GitHub Actions #1001',
        status: 'completed',
        conclusion: 'action_required',
        url: 'https://github.com/org/repo/commits/head-oid/checks#check-suite-1001'
      }
    ])
  })

  it('keeps check-run rows when status and suite enrichment fail', async () => {
    const { checks } = createHarness({
      steps: [
        new Error('GraphQL rollup failed'),
        ghOk({
          check_runs: [
            {
              id: 88,
              name: 'Summary',
              status: 'completed',
              conclusion: 'success',
              html_url: 'https://github.com/org/repo/actions/runs/88',
              details_url: null
            }
          ]
        }),
        new Error('status enrichment failed'),
        new Error('suite enrichment failed')
      ]
    })

    const result = await checks.getPRChecks({ repo: ORG_REPO, prNumber: 7, headSha: 'head-oid' })

    expect(result).toEqual([
      {
        name: 'Summary',
        status: 'completed',
        conclusion: 'success',
        url: 'https://github.com/org/repo/actions/runs/88',
        checkRunId: 88,
        workflowRunId: 88
      }
    ])
  })

  it('falls through to REST when the rollup has no pull request', async () => {
    const { checks } = createHarness({
      steps: [
        ghOk({ data: { repository: { pullRequest: null } } }),
        ghOk({
          check_runs: [
            {
              id: 5,
              name: 'lint',
              status: 'queued',
              conclusion: null,
              html_url: 'https://github.com/org/repo/runs/5',
              details_url: null
            }
          ]
        }),
        ghOk({ statuses: [] }),
        ghOk({ check_suites: [] })
      ]
    })

    const result = await checks.getPRChecks({ repo: ORG_REPO, prNumber: 7, headSha: 'head-oid' })

    expect(result).toEqual([
      {
        name: 'lint',
        status: 'queued',
        conclusion: 'pending',
        url: 'https://github.com/org/repo/runs/5',
        checkRunId: 5
      }
    ])
  })

  it('returns an empty list for a rollup without checks and does not degrade', async () => {
    const { checks, executor } = createHarness({
      steps: [
        ghOk({
          data: {
            repository: {
              pullRequest: { headRefOid: 'head-oid', commits: { nodes: [{ commit: {} }] } }
            }
          }
        })
      ]
    })

    const result = await checks.getPRChecks({ repo: ORG_REPO, prNumber: 7, headSha: 'head-oid' })

    expect(result).toEqual([])
    expect(executor).toHaveBeenCalledTimes(1)
  })

  it('falls back to gh pr checks when GraphQL and REST are unavailable', async () => {
    const { checks, executor } = createHarness({
      steps: [
        new Error('GraphQL rollup failed'),
        new Error('REST details failed'),
        ghOk([{ name: 'lint', state: 'PASS', link: 'https://github.com/org/repo/actions/runs/5' }])
      ]
    })

    const result = await checks.getPRChecks({ repo: ORG_REPO, prNumber: 7, headSha: 'head-oid' })

    expect(executor).toHaveBeenNthCalledWith(
      3,
      ['pr', 'checks', '7', '--json', 'name,state,link', '--repo', 'org/repo'],
      expect.anything()
    )
    expect(result).toEqual([
      {
        name: 'lint',
        status: 'completed',
        conclusion: 'success',
        url: 'https://github.com/org/repo/actions/runs/5',
        workflowRunId: 5
      }
    ])
  })

  it('treats gh pr checks "no checks reported" as an empty list', async () => {
    const { checks } = createHarness({
      steps: [
        new Error('GraphQL rollup failed'),
        ghFail("no checks reported on the 'feature' branch\n")
      ]
    })

    const result = await checks.getPRChecks({ repo: ORG_REPO, prNumber: 7 })

    expect(result).toEqual([])
  })

  it('rethrows unexpected gh pr checks fallback failures', async () => {
    const { checks } = createHarness({
      steps: [
        new Error('GraphQL rollup failed'),
        ghFail('GraphQL: Could not resolve to a PullRequest')
      ]
    })

    await expect(checks.getPRChecks({ repo: ORG_REPO, prNumber: 7 })).rejects.toThrow(
      'GraphQL: Could not resolve to a PullRequest'
    )
  })

  it('maps gh pr checks state strings through the verbatim tables', () => {
    expect(mapCheckStatus('PENDING')).toBe('queued')
    expect(mapCheckStatus('QUEUED')).toBe('queued')
    expect(mapCheckStatus('IN_PROGRESS')).toBe('in_progress')
    expect(mapCheckStatus('COMPLETED')).toBe('completed')
    expect(mapCheckStatus('PASS')).toBe('completed')

    expect(mapCheckConclusion('SUCCESS')).toBe('success')
    expect(mapCheckConclusion('PASS')).toBe('success')
    expect(mapCheckConclusion('FAILURE')).toBe('failure')
    expect(mapCheckConclusion('FAIL')).toBe('failure')
    expect(mapCheckConclusion('ACTION_REQUIRED')).toBe('action_required')
    expect(mapCheckConclusion('STALE')).toBe('failure')
    expect(mapCheckConclusion('STARTUP_FAILURE')).toBe('failure')
    expect(mapCheckConclusion('CANCELLED')).toBe('cancelled')
    expect(mapCheckConclusion('TIMED_OUT')).toBe('timed_out')
    expect(mapCheckConclusion('SKIPPED')).toBe('skipped')
    expect(mapCheckConclusion('PENDING')).toBe('pending')
    expect(mapCheckConclusion('QUEUED')).toBe('pending')
    expect(mapCheckConclusion('IN_PROGRESS')).toBe('pending')
    expect(mapCheckConclusion('NEUTRAL')).toBe('neutral')
    expect(mapCheckConclusion('UNKNOWN')).toBeNull()
  })

  it('maps REST status and conclusion fields through the verbatim tables', () => {
    expect(conclusionMap).toEqual({
      success: 'success',
      failure: 'failure',
      cancelled: 'cancelled',
      timed_out: 'timed_out',
      skipped: 'skipped',
      neutral: 'neutral',
      action_required: 'action_required',
      stale: 'failure',
      startup_failure: 'failure'
    })
    expect(mapCheckRunRESTStatus('queued')).toBe('queued')
    expect(mapCheckRunRESTStatus('in_progress')).toBe('in_progress')
    expect(mapCheckRunRESTStatus('completed')).toBe('completed')
    expect(mapCheckRunRESTConclusion('completed', 'success')).toBe('success')
    expect(mapCheckRunRESTConclusion('completed', 'stale')).toBe('failure')
    expect(mapCheckRunRESTConclusion('completed', 'startup_failure')).toBe('failure')
    expect(mapCheckRunRESTConclusion('completed', 'action_required')).toBe('action_required')
    expect(mapCheckRunRESTConclusion('in_progress', null)).toBe('pending')
    expect(mapCheckRunRESTConclusion('completed', null)).toBeNull()
    expect(mapCommitStatusRESTStatus('pending')).toBe('queued')
    expect(mapCommitStatusRESTStatus('success')).toBe('completed')
    expect(mapCommitStatusRESTConclusion('success')).toBe('success')
    expect(mapCommitStatusRESTConclusion('failure')).toBe('failure')
    expect(mapCommitStatusRESTConclusion('error')).toBe('failure')
    expect(mapCommitStatusRESTConclusion('pending')).toBe('pending')
    expect(mapCommitStatusRESTConclusion('unknown')).toBeNull()
  })

  it('names pending approval suites with app, suite id, or head fallbacks', () => {
    expect(
      getPendingApprovalCheckSuiteName(
        { databaseId: 1001, app: { name: 'GitHub Actions', slug: 'github-actions' } },
        'head-oid',
        0
      )
    ).toBe('GitHub Actions #1001')
    expect(getPendingApprovalCheckSuiteName({ databaseId: 1001, app: null }, 'head-oid', 0)).toBe(
      '#1001'
    )
    expect(
      getPendingApprovalCheckSuiteName({ databaseId: null, app: { slug: 'codecov' } }, 'head-oid', 0)
    ).toBe('codecov')
    expect(getPendingApprovalCheckSuiteName({ databaseId: null, app: null }, 'head-oid', 2)).toBe(
      'head-oid:3'
    )
  })
})
