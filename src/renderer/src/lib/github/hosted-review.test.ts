import { describe, expect, it, vi } from 'vitest'
import type { PRRefreshOutcome } from '../../../../shared/github/pull-request-refresh-types'
import type { PRInfo } from '../../../../shared/github/pull-request-types'
import { createHostedReviewClient } from './hosted-review'
import type { PRForBranchLookup } from './pr-for-branch'
import type { GitHubRepoIdentity, RepoIdentityResolver } from './repo-identity'

const NOW = 1_700_000_000_000

const ORG_REPO: GitHubRepoIdentity = { owner: 'org', repo: 'repo' }

const FOUND_PR: PRInfo = {
  number: 42,
  title: 'Feature',
  state: 'open',
  url: 'https://github.com/org/repo/pull/42',
  checksStatus: 'success',
  updatedAt: '2026-10-07T00:00:00Z',
  mergeable: 'MERGEABLE',
  reviewDecision: 'APPROVED',
  headSha: 'head1',
  prRepo: ORG_REPO
}

const MERGED_PR: PRInfo = { ...FOUND_PR, state: 'merged', headSha: 'head1' }

const FOUND_OUTCOME: PRRefreshOutcome = { kind: 'found', pr: FOUND_PR, fetchedAt: NOW }
const MERGED_OUTCOME: PRRefreshOutcome = { kind: 'found', pr: MERGED_PR, fetchedAt: NOW }
const NO_PR_OUTCOME: PRRefreshOutcome = { kind: 'no-pr', fetchedAt: NOW }

function createHarness(options: { candidates?: GitHubRepoIdentity[] } = {}) {
  let currentNow = NOW
  const getPRForBranchOutcome = vi.fn<PRForBranchLookup['getPRForBranchOutcome']>(
    async () => FOUND_OUTCOME
  )
  const resolveCandidates = vi.fn<RepoIdentityResolver['resolveCandidates']>(async () => ({
    candidates: options.candidates ?? [ORG_REPO],
    headRepo: null
  }))
  const client = createHostedReviewClient({
    identity: { resolveCandidates },
    lookup: { getPRForBranchOutcome },
    now: () => currentNow
  })
  return {
    client,
    getPRForBranchOutcome,
    resolveCandidates,
    advance: (ms: number) => {
      currentNow += ms
    }
  }
}

describe('hosted review forBranch', () => {
  it('maps a GitHub hit through hostedReviewInfoFromGitHubPRInfo', async () => {
    const { client, getPRForBranchOutcome } = createHarness()
    const review = await client.forBranch({ repoPath: '/repo', branch: 'refs/heads/feature' })
    expect(review).toEqual({
      provider: 'github',
      number: 42,
      title: 'Feature',
      state: 'open',
      url: 'https://github.com/org/repo/pull/42',
      status: 'success',
      updatedAt: '2026-10-07T00:00:00Z',
      mergeable: 'MERGEABLE',
      reviewDecision: 'APPROVED',
      headSha: 'head1',
      githubRepository: ORG_REPO
    })
    expect(getPRForBranchOutcome).toHaveBeenCalledWith({
      worktreePath: '/repo',
      branch: 'feature',
      linkedPRNumber: null,
      fallbackPRNumber: null,
      acceptMergedFallbackPR: false,
      currentHeadOid: undefined
    })
  })

  it('passes a linked PR as the exact lookup and ignores the fallback then', async () => {
    const { client, getPRForBranchOutcome } = createHarness()
    await client.forBranch({
      repoPath: '/repo',
      branch: 'feature',
      linkedGitHubPR: 7,
      fallbackGitHubPR: 9
    })
    expect(getPRForBranchOutcome).toHaveBeenCalledWith(
      expect.objectContaining({
        linkedPRNumber: 7,
        fallbackPRNumber: null,
        acceptMergedFallbackPR: false
      })
    )
  })

  it('passes a fallback PR only when no linked PR exists and accepts merged', async () => {
    const { client, getPRForBranchOutcome } = createHarness()
    await client.forBranch({ repoPath: '/repo', branch: 'feature', fallbackGitHubPR: 9 })
    expect(getPRForBranchOutcome).toHaveBeenCalledWith(
      expect.objectContaining({
        linkedPRNumber: null,
        fallbackPRNumber: 9,
        acceptMergedFallbackPR: true
      })
    )
  })

  it('returns null for a no-pr outcome', async () => {
    const { client, getPRForBranchOutcome } = createHarness()
    getPRForBranchOutcome.mockResolvedValueOnce(NO_PR_OUTCOME)
    await expect(client.forBranch({ repoPath: '/repo', branch: 'feature' })).resolves.toBeNull()
  })

  it('returns null without resolving identity when branch and links are all absent', async () => {
    const { client, getPRForBranchOutcome, resolveCandidates } = createHarness()
    await expect(client.forBranch({ repoPath: '/repo', branch: '' })).resolves.toBeNull()
    expect(resolveCandidates).not.toHaveBeenCalled()
    expect(getPRForBranchOutcome).not.toHaveBeenCalled()
  })

  it('returns null for a non-GitHub repository without a lookup', async () => {
    const { client, getPRForBranchOutcome } = createHarness({ candidates: [] })
    await expect(client.forBranch({ repoPath: '/repo', branch: 'feature' })).resolves.toBeNull()
    expect(getPRForBranchOutcome).not.toHaveBeenCalled()
  })

  it('returns null for a non-default-host candidate without a lookup', async () => {
    const { client, getPRForBranchOutcome } = createHarness({
      candidates: [{ owner: 'org', repo: 'repo', host: 'ghe.internal:8443' }]
    })
    await expect(client.forBranch({ repoPath: '/repo', branch: 'feature' })).resolves.toBeNull()
    expect(getPRForBranchOutcome).not.toHaveBeenCalled()
  })

  it('throws the wrapped upstream error', async () => {
    const { client, getPRForBranchOutcome } = createHarness()
    getPRForBranchOutcome.mockResolvedValueOnce({
      kind: 'upstream-error',
      errorType: 'network',
      message: 'GitHub is unreachable right now. Check your network and try again.',
      fetchedAt: NOW
    })
    await expect(
      client.forBranch({ repoPath: '/repo', branch: 'feature' })
    ).rejects.toThrowError(
      'GitHub PR lookup failed (network): GitHub is unreachable right now. Check your network and try again.'
    )
  })

  it('caches a found review for 60s', async () => {
    const { client, getPRForBranchOutcome, advance } = createHarness()
    const first = await client.forBranch({ repoPath: '/repo', branch: 'feature' })
    advance(59_000)
    await expect(client.forBranch({ repoPath: '/repo', branch: 'feature' })).resolves.toBe(first)
    expect(getPRForBranchOutcome).toHaveBeenCalledTimes(1)

    advance(2_000)
    await client.forBranch({ repoPath: '/repo', branch: 'feature' })
    expect(getPRForBranchOutcome).toHaveBeenCalledTimes(2)
  })

  it('caches a no-review answer for 15min when inactive', async () => {
    const { client, getPRForBranchOutcome, advance } = createHarness()
    getPRForBranchOutcome.mockResolvedValue(NO_PR_OUTCOME)
    await client.forBranch({ repoPath: '/repo', branch: 'feature' })
    advance(14 * 60_000)
    await expect(client.forBranch({ repoPath: '/repo', branch: 'feature' })).resolves.toBeNull()
    expect(getPRForBranchOutcome).toHaveBeenCalledTimes(1)

    advance(61_000)
    await client.forBranch({ repoPath: '/repo', branch: 'feature' })
    expect(getPRForBranchOutcome).toHaveBeenCalledTimes(2)
  })

  it('caches a no-review answer for 60s when active', async () => {
    const { client, getPRForBranchOutcome, advance } = createHarness()
    getPRForBranchOutcome.mockResolvedValue(NO_PR_OUTCOME)
    await client.forBranch({ repoPath: '/repo', branch: 'feature', active: true })
    advance(59_000)
    await client.forBranch({ repoPath: '/repo', branch: 'feature', active: true })
    expect(getPRForBranchOutcome).toHaveBeenCalledTimes(1)

    advance(2_000)
    await client.forBranch({ repoPath: '/repo', branch: 'feature', active: true })
    expect(getPRForBranchOutcome).toHaveBeenCalledTimes(2)
  })

  it('re-queries a cached merged review when the head oid changed', async () => {
    const { client, getPRForBranchOutcome, advance } = createHarness()
    getPRForBranchOutcome.mockResolvedValue(MERGED_OUTCOME)
    await client.forBranch({ repoPath: '/repo', branch: 'feature', currentHeadOid: 'head1' })
    advance(10_000)
    await client.forBranch({ repoPath: '/repo', branch: 'feature', currentHeadOid: 'head1' })
    expect(getPRForBranchOutcome).toHaveBeenCalledTimes(1)

    await client.forBranch({ repoPath: '/repo', branch: 'feature', currentHeadOid: 'head2' })
    expect(getPRForBranchOutcome).toHaveBeenCalledTimes(2)
  })

  it('keys the cache by branch', async () => {
    const { client, getPRForBranchOutcome } = createHarness()
    await client.forBranch({ repoPath: '/repo', branch: 'feature' })
    await client.forBranch({ repoPath: '/repo', branch: 'other' })
    expect(getPRForBranchOutcome).toHaveBeenCalledTimes(2)
  })
})

describe('hosted review invalidate', () => {
  it('re-queries the branch after invalidating its repo path', async () => {
    const { client, getPRForBranchOutcome } = createHarness()
    await client.forBranch({ repoPath: '/repo', branch: 'feature' })
    client.invalidate('/repo')
    await client.forBranch({ repoPath: '/repo', branch: 'feature' })
    expect(getPRForBranchOutcome).toHaveBeenCalledTimes(2)
  })

  it('leaves entries for other repo paths cached', async () => {
    const { client, getPRForBranchOutcome } = createHarness()
    await client.forBranch({ repoPath: '/repo', branch: 'feature' })
    await client.forBranch({ repoPath: '/other', branch: 'feature' })
    client.invalidate('/repo')
    await client.forBranch({ repoPath: '/other', branch: 'feature' })
    expect(getPRForBranchOutcome).toHaveBeenCalledTimes(2)
    await client.forBranch({ repoPath: '/repo', branch: 'feature' })
    expect(getPRForBranchOutcome).toHaveBeenCalledTimes(3)
  })

  it('does not clear a repo path that only shares a prefix', async () => {
    const { client, getPRForBranchOutcome } = createHarness()
    await client.forBranch({ repoPath: '/repo', branch: 'feature' })
    client.invalidate('/rep')
    await client.forBranch({ repoPath: '/repo', branch: 'feature' })
    expect(getPRForBranchOutcome).toHaveBeenCalledTimes(1)
  })
})
