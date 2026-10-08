import { describe, expect, it, vi } from 'vitest'
import {
  PR_BRANCH_LIST_JSON_FIELDS,
  PR_LOOKUP_JSON_FIELDS,
  createPRForBranchLookup
} from './pr-for-branch'
import type { PRForBranchLookupDeps } from './pr-for-branch'
import { createGhExecClient } from './gh-exec-client'
import type { GhExecResult, GhExecutor } from './gh-exec-client'
import type { GitHubRepoIdentity } from './repo-identity'
import type { PRConflictSummary } from '../../../../shared/github/pull-request-types'

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
const FORK: GitHubRepoIdentity = { owner: 'me', repo: 'fork' }

const PR_LIST_ARGV: string[] = [
  'pr',
  'list',
  '--repo',
  'org/repo',
  '--head',
  'feature',
  '--state',
  'all',
  '--limit',
  '1',
  '--json',
  PR_BRANCH_LIST_JSON_FIELDS
]

const SUCCESS_PR = {
  number: 42,
  title: 'Feature',
  state: 'OPEN',
  url: 'https://github.com/org/repo/pull/42',
  statusCheckRollup: [{ status: 'COMPLETED', conclusion: 'SUCCESS' }],
  updatedAt: '2026-10-07T00:00:00Z',
  isDraft: false,
  mergeable: 'MERGEABLE',
  reviewDecision: 'APPROVED',
  mergeStateStatus: 'CLEAN',
  autoMergeRequest: null,
  baseRefName: 'main',
  headRefName: 'feature',
  baseRefOid: 'base1',
  headRefOid: 'head1'
}

const MERGED_PR = {
  ...SUCCESS_PR,
  state: 'MERGED',
  mergeable: 'UNKNOWN',
  mergeStateStatus: 'UNKNOWN',
  headRefOid: 'oldhead'
}

function createHarness(
  options: {
    candidates?: GitHubRepoIdentity[]
    headRepo?: GitHubRepoIdentity | null
    steps?: GhStep[]
  } = {}
) {
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
  const candidates = options.candidates ?? [ORG_REPO]
  const resolveCandidates = vi.fn(async () => ({
    candidates,
    headRepo: options.headRepo === undefined ? null : options.headRepo
  }))
  const getConflictSummary = vi.fn<
    NonNullable<PRForBranchLookupDeps['getConflictSummary']>
  >(async () => undefined)
  const lookup = createPRForBranchLookup({
    client: createGhExecClient(executor),
    identity: { resolveCandidates },
    getConflictSummary
  })
  return { lookup, executor, resolveCandidates, getConflictSummary }
}

describe('pr-for-branch lookup', () => {
  it('returns no-pr without spawning when branch, linked, and fallback are all absent', async () => {
    const { lookup, executor, resolveCandidates } = createHarness()
    const outcome = await lookup.getPRForBranchOutcome({ worktreePath: '/repo', branch: '' })
    expect(outcome).toMatchObject({ kind: 'no-pr' })
    expect(executor).not.toHaveBeenCalled()
    expect(resolveCandidates).not.toHaveBeenCalled()
  })

  it('resolves a linked PR number through the exact lookup and populates provenance', async () => {
    const { lookup, executor } = createHarness({
      candidates: [ORG_REPO, FORK],
      headRepo: FORK,
      steps: [ghOk(SUCCESS_PR)]
    })
    const outcome = await lookup.getPRForBranchOutcome({
      worktreePath: '/repo',
      branch: 'feature',
      linkedPRNumber: 42
    })
    expect(executor).toHaveBeenCalledWith(
      ['pr', 'view', '42', '--repo', 'org/repo', '--json', PR_LOOKUP_JSON_FIELDS],
      {}
    )
    expect(outcome).toMatchObject({
      kind: 'found',
      pr: {
        number: 42,
        state: 'open',
        checksStatus: 'success',
        headSha: 'head1',
        prRepo: ORG_REPO,
        headRepo: FORK
      }
    })
  })

  it('hydrates a REST branch hit with the exact PR lookup', async () => {
    const { lookup, executor } = createHarness({
      candidates: [ORG_REPO],
      headRepo: FORK,
      steps: [
        ghOk([
          {
            number: 42,
            title: 'Feature',
            state: 'open',
            html_url: 'https://github.com/org/repo/pull/42',
            updated_at: '2026-10-06T00:00:00Z',
            draft: false,
            merged_at: null,
            mergeable: true,
            mergeable_state: 'clean',
            base: { ref: 'main', sha: 'base1' },
            head: { ref: 'feature', sha: 'head1' }
          }
        ]),
        ghOk(SUCCESS_PR)
      ]
    })
    const outcome = await lookup.getPRForBranchOutcome({
      worktreePath: '/repo',
      branch: 'feature'
    })
    expect(executor).toHaveBeenNthCalledWith(
      1,
      ['api', 'repos/org/repo/pulls?head=me%3Afeature&state=all&per_page=1'],
      {}
    )
    expect(executor).toHaveBeenNthCalledWith(
      2,
      ['pr', 'view', '42', '--repo', 'org/repo', '--json', PR_LOOKUP_JSON_FIELDS],
      {}
    )
    expect(outcome).toMatchObject({ kind: 'found', pr: { number: 42, checksStatus: 'success' } })
  })

  it('finds a fork PR through gh pr list first when the head repo is unknown', async () => {
    const { lookup, executor } = createHarness({
      candidates: [ORG_REPO],
      steps: [ghOk([{ ...SUCCESS_PR }]), ghOk(SUCCESS_PR)]
    })
    const outcome = await lookup.getPRForBranchOutcome({
      worktreePath: '/repo',
      branch: 'feature'
    })
    expect(executor).toHaveBeenNthCalledWith(1, PR_LIST_ARGV, {})
    expect(executor).toHaveBeenNthCalledWith(
      2,
      ['pr', 'view', '42', '--repo', 'org/repo', '--json', PR_LOOKUP_JSON_FIELDS],
      {}
    )
    expect(executor).toHaveBeenCalledTimes(2)
    expect(outcome).toMatchObject({ kind: 'found', pr: { number: 42 } })
  })

  it('retries REST with the candidate owner only when gh pr list fails without a known head repo', async () => {
    const { lookup, executor } = createHarness({
      candidates: [ORG_REPO],
      steps: [
        ghFail('boom'),
        ghOk([
          {
            number: 42,
            title: 'Feature',
            state: 'open',
            html_url: 'https://github.com/org/repo/pull/42',
            updated_at: '2026-10-06T00:00:00Z',
            draft: false,
            merged_at: null,
            mergeable: true,
            mergeable_state: 'clean',
            base: { ref: 'main', sha: 'base1' },
            head: { ref: 'feature', sha: 'head1' }
          }
        ]),
        ghOk(SUCCESS_PR)
      ]
    })
    const outcome = await lookup.getPRForBranchOutcome({
      worktreePath: '/repo',
      branch: 'feature'
    })
    expect(executor).toHaveBeenNthCalledWith(1, PR_LIST_ARGV, {})
    expect(executor).toHaveBeenNthCalledWith(
      2,
      ['api', 'repos/org/repo/pulls?head=org%3Afeature&state=all&per_page=1'],
      {}
    )
    expect(executor).toHaveBeenNthCalledWith(
      3,
      ['pr', 'view', '42', '--repo', 'org/repo', '--json', PR_LOOKUP_JSON_FIELDS],
      {}
    )
    expect(outcome).toMatchObject({ kind: 'found', pr: { number: 42 } })
  })

  it('uses the fallback PR number after a branch miss', async () => {
    const { lookup, executor } = createHarness({
      candidates: [ORG_REPO],
      headRepo: FORK,
      steps: [ghOk([]), ghOk({ ...SUCCESS_PR, number: 99 })]
    })
    const outcome = await lookup.getPRForBranchOutcome({
      worktreePath: '/repo',
      branch: 'feature',
      fallbackPRNumber: 99
    })
    expect(executor).toHaveBeenNthCalledWith(
      1,
      ['api', 'repos/org/repo/pulls?head=me%3Afeature&state=all&per_page=1'],
      {}
    )
    expect(executor).toHaveBeenNthCalledWith(
      2,
      ['pr', 'view', '99', '--repo', 'org/repo', '--json', PR_LOOKUP_JSON_FIELDS],
      {}
    )
    expect(outcome).toMatchObject({ kind: 'found', pr: { number: 99 } })
  })

  it('treats a 404 exact lookup as no-pr instead of throwing', async () => {
    const { lookup, executor } = createHarness({
      candidates: [ORG_REPO],
      steps: [ghFail('HTTP 404: Not Found (https://api.github.com/repos/org/repo/pulls/7)')]
    })
    const outcome = await lookup.getPRForBranchOutcome({
      worktreePath: '/repo',
      branch: 'feature',
      linkedPRNumber: 7
    })
    expect(outcome).toMatchObject({ kind: 'no-pr' })
    expect(executor).toHaveBeenCalledTimes(1)
  })

  it('hides a merged implicit PR unless the current head matches its head OID', async () => {
    const restMerged = [
      {
        number: 42,
        title: 'Feature',
        state: 'closed',
        html_url: 'https://github.com/org/repo/pull/42',
        updated_at: '2026-09-01T00:00:00Z',
        draft: false,
        merged_at: '2026-09-01T00:00:00Z',
        mergeable: null,
        mergeable_state: 'unknown',
        base: { ref: 'main', sha: 'base1' },
        head: { ref: 'feature', sha: 'oldhead' }
      }
    ]

    const hidden = createHarness({
      candidates: [ORG_REPO],
      headRepo: FORK,
      steps: [ghOk(restMerged), ghOk(MERGED_PR)]
    })
    await expect(
      hidden.lookup.getPRForBranchOutcome({
        worktreePath: '/repo',
        branch: 'feature',
        currentHeadOid: 'newhead'
      })
    ).resolves.toMatchObject({ kind: 'no-pr' })

    const kept = createHarness({
      candidates: [ORG_REPO],
      headRepo: FORK,
      steps: [ghOk(restMerged), ghOk(MERGED_PR)]
    })
    await expect(
      kept.lookup.getPRForBranchOutcome({
        worktreePath: '/repo',
        branch: 'feature',
        currentHeadOid: 'oldhead'
      })
    ).resolves.toMatchObject({ kind: 'found', pr: { number: 42, state: 'merged' } })
  })

  it('returns no-pr without spawning when the identity resolver yields no candidates', async () => {
    const { lookup, executor } = createHarness({ candidates: [] })
    const outcome = await lookup.getPRForBranchOutcome({
      worktreePath: '/repo',
      branch: 'feature'
    })
    expect(outcome).toMatchObject({ kind: 'no-pr' })
    expect(executor).not.toHaveBeenCalled()
  })

  it('never queries gh for non-default-host candidates', async () => {
    const ghes: GitHubRepoIdentity = { owner: 'org', repo: 'repo', host: 'ghe.internal:8443' }
    const { lookup, executor } = createHarness({ candidates: [ghes], headRepo: ghes })
    const outcome = await lookup.getPRForBranchOutcome({
      worktreePath: '/repo',
      branch: 'feature'
    })
    expect(outcome).toMatchObject({ kind: 'no-pr' })
    expect(executor).not.toHaveBeenCalled()
  })

  it('classifies a permission failure as upstream-error.permission', async () => {
    const { lookup } = createHarness({
      candidates: [ORG_REPO],
      headRepo: FORK,
      steps: [ghFail('HTTP 403: resource not accessible by integration')]
    })
    const outcome = await lookup.getPRForBranchOutcome({
      worktreePath: '/repo',
      branch: 'feature'
    })
    expect(outcome).toMatchObject({
      kind: 'upstream-error',
      errorType: 'permission',
      message: 'GitHub did not allow access to this pull request.'
    })
  })

  it('classifies a 429 with Retry-After as rate_limited and schedules the retry', async () => {
    const before = Date.now()
    const { lookup } = createHarness({
      candidates: [ORG_REPO],
      headRepo: FORK,
      steps: [ghFail('HTTP 429: rate limit exceeded\nRetry-After: 60')]
    })
    const outcome = await lookup.getPRForBranchOutcome({
      worktreePath: '/repo',
      branch: 'feature'
    })
    expect(outcome).toMatchObject({ kind: 'upstream-error', errorType: 'rate_limited' })
    if (outcome.kind !== 'upstream-error') {
      throw new Error('expected upstream-error')
    }
    expect(outcome.nextAutoRetryAt).toBeGreaterThanOrEqual(before + 60_000)
    expect(outcome.nextAutoRetryAt).toBe(outcome.retryDisabledUntil)
  })

  it('unwraps the legacy getPRForBranch result to PRInfo or null', async () => {
    const found = createHarness({ steps: [ghOk(SUCCESS_PR)] })
    const pr = await found.lookup.getPRForBranch({
      worktreePath: '/repo',
      branch: 'feature',
      linkedPRNumber: 42
    })
    expect(pr).toMatchObject({ number: 42, checksStatus: 'success' })
    expect(pr).not.toHaveProperty('kind')

    const missing = createHarness({ steps: [ghFail('HTTP 404: Not Found')] })
    await expect(
      missing.lookup.getPRForBranch({
        worktreePath: '/repo',
        branch: 'feature',
        linkedPRNumber: 7
      })
    ).resolves.toBeNull()
  })

  it('derives mergeable and injects a conflict summary only for conflicting PRs', async () => {
    const conflict: PRConflictSummary = {
      baseRef: 'main',
      baseCommit: 'base1',
      commitsBehind: 2,
      files: ['src/a.ts']
    }
    const { lookup, getConflictSummary } = createHarness({
      steps: [ghOk({ ...SUCCESS_PR, mergeable: 'CONFLICTING', mergeStateStatus: 'DIRTY' })]
    })
    getConflictSummary.mockResolvedValueOnce(conflict)
    const outcome = await lookup.getPRForBranchOutcome({
      worktreePath: '/repo',
      branch: 'feature',
      linkedPRNumber: 42
    })
    expect(getConflictSummary).toHaveBeenCalledWith({
      worktreePath: '/repo',
      baseRefName: 'main',
      baseRefOid: 'base1',
      headRefOid: 'head1'
    })
    expect(outcome).toMatchObject({
      kind: 'found',
      pr: { mergeable: 'CONFLICTING', conflictSummary: conflict }
    })
  })
})
