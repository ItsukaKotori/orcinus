import { describe, expect, it, vi } from 'vitest'
import type { HostedReviewInfo } from '../../../../shared/hosted-review'
import type { GhExecResult } from './gh-exec-client'
import { GitReadError } from './git-read-client'
import {
  baseRefExistsOnRemote,
  createHostedReviewCreation,
  getDefaultBaseRef
} from './hosted-review-create'
import type { HostedReviewCreationEligibilityInput } from './hosted-review-create'
import type { GitHubRepoIdentity } from './repo-identity'

type RunGit = (args: string[]) => Promise<{ stdout: string }>

const ORG_REPO: GitHubRepoIdentity = { owner: 'org', repo: 'repo' }
const NOW = 1_700_000_000_000

const FOUND_REVIEW: HostedReviewInfo = {
  provider: 'github',
  number: 7,
  title: 'Feature',
  state: 'open',
  url: 'https://github.com/org/repo/pull/7',
  status: 'success',
  updatedAt: '2026-10-08T00:00:00Z',
  mergeable: 'MERGEABLE'
}

const ACTIVE_AUTH: GhExecResult = {
  stdout: 'Logged in to github.com account octocat (keyring)\n  - Active account: true\n',
  stderr: '',
  code: 0
}

const NOT_AUTHENTICATED: GhExecResult = {
  stdout: '',
  stderr: 'You are not logged into any GitHub hosts. To log in, run: gh auth login\n',
  code: 1
}

const NO_MATCH = (): never => {
  throw new GitReadError({ stdout: '', stderr: '', code: 1 })
}

const BASE_ARGS: HostedReviewCreationEligibilityInput = {
  repoPath: '/repo',
  repoId: 'repo-1',
  worktreePath: '/worktree',
  branch: 'feature',
  hasUncommittedChanges: false,
  hasUpstream: true,
  ahead: 0,
  behind: 0
}

type HarnessOptions = {
  slug?: GitHubRepoIdentity | null
  review?: HostedReviewInfo | null
  reviewError?: Error
  authResult?: GhExecResult
  authError?: Error
  runGit?: RunGit
}

function createHarness(options: HarnessOptions = {}) {
  const makeRunGitPaths: string[] = []
  const runGit = vi.fn<RunGit>(options.runGit ?? (async () => ({ stdout: '' })))
  const getRepoSlug = vi.fn(async () => (options.slug === undefined ? ORG_REPO : options.slug))
  const forBranch = vi.fn(async () => {
    if (options.reviewError) {
      throw options.reviewError
    }
    return options.review ?? null
  })
  const run = vi.fn(async () => {
    if (options.authError) {
      throw options.authError
    }
    return options.authResult ?? ACTIVE_AUTH
  })
  const creation = createHostedReviewCreation({
    client: { run },
    identity: { getRepoSlug },
    reviewLookup: { forBranch },
    makeRunGit: (worktreePath) => {
      makeRunGitPaths.push(worktreePath)
      return runGit
    },
    readStatus: async () => ({ entries: [] }),
    readUpstream: async () => ({ hasUpstream: true, ahead: 0, behind: 0 }),
    now: () => NOW
  })
  return { creation, runGit, getRepoSlug, forBranch, run, makeRunGitPaths }
}

describe('getCreationEligibility blockers', () => {
  it('blocks a detached HEAD first', async () => {
    const { creation } = createHarness()
    const result = await creation.getCreationEligibility({ ...BASE_ARGS, branch: 'HEAD' })
    expect(result).toMatchObject({
      canCreate: false,
      blockedReason: 'detached_head',
      nextAction: null
    })
  })

  it('blocks an empty branch name', async () => {
    const { creation } = createHarness()
    const result = await creation.getCreationEligibility({ ...BASE_ARGS, branch: '' })
    expect(result).toMatchObject({
      canCreate: false,
      blockedReason: 'detached_head',
      nextAction: null,
      head: null
    })
  })

  it('blocks an existing review with the open action', async () => {
    const { creation, forBranch } = createHarness({ review: FOUND_REVIEW })
    const result = await creation.getCreationEligibility(BASE_ARGS)
    expect(result).toMatchObject({
      canCreate: false,
      blockedReason: 'existing_review',
      nextAction: 'open_existing_review',
      review: { number: 7, url: FOUND_REVIEW.url },
      reviewLookupOutcome: 'found'
    })
    expect(forBranch).toHaveBeenCalledWith({
      repoPath: '/worktree',
      branch: 'feature',
      currentHeadOid: null,
      active: true,
      linkedGitHubPR: null,
      fallbackGitHubPR: null
    })
  })

  it('prefers an existing review over an unsupported provider', async () => {
    const { creation } = createHarness({ slug: null, review: FOUND_REVIEW })
    const result = await creation.getCreationEligibility(BASE_ARGS)
    expect(result.blockedReason).toBe('existing_review')
  })

  it('blocks when the repo identity is missing', async () => {
    const { creation } = createHarness({ slug: null })
    const result = await creation.getCreationEligibility(BASE_ARGS)
    expect(result).toMatchObject({
      provider: 'unsupported',
      canCreate: false,
      blockedReason: 'unsupported_provider',
      nextAction: null
    })
  })

  it('blocks a non-default host (GHES)', async () => {
    const { creation } = createHarness({
      slug: { owner: 'org', repo: 'repo', host: 'ghe.corp.example' }
    })
    const result = await creation.getCreationEligibility(BASE_ARGS)
    expect(result).toMatchObject({
      provider: 'unsupported',
      canCreate: false,
      blockedReason: 'unsupported_provider',
      nextAction: null
    })
  })

  it('blocks the default branch case-insensitively', async () => {
    const { creation } = createHarness()
    const result = await creation.getCreationEligibility({ ...BASE_ARGS, branch: 'Main' })
    expect(result).toMatchObject({
      canCreate: false,
      blockedReason: 'default_branch',
      nextAction: null,
      defaultBaseRef: 'origin/main'
    })
  })

  it('blocks a dirty worktree with the commit action', async () => {
    const { creation } = createHarness()
    const result = await creation.getCreationEligibility({
      ...BASE_ARGS,
      hasUncommittedChanges: true
    })
    expect(result).toMatchObject({
      canCreate: false,
      blockedReason: 'dirty',
      nextAction: 'commit'
    })
  })

  it('blocks a branch without an upstream with the publish action', async () => {
    const { creation } = createHarness()
    const result = await creation.getCreationEligibility({ ...BASE_ARGS, hasUpstream: false })
    expect(result).toMatchObject({
      canCreate: false,
      blockedReason: 'no_upstream',
      nextAction: 'publish'
    })
  })

  it('returns an unusable retryable result when upstream is unknown', async () => {
    const { creation } = createHarness()
    const result = await creation.getCreationEligibility({
      ...BASE_ARGS,
      hasUpstream: undefined
    })
    expect(result).toMatchObject({
      canCreate: false,
      blockedReason: null,
      nextAction: null,
      reviewLookupOutcome: 'not_found'
    })
  })

  it('blocks a behind branch with the sync action', async () => {
    const { creation } = createHarness()
    const result = await creation.getCreationEligibility({ ...BASE_ARGS, behind: 2 })
    expect(result).toMatchObject({
      canCreate: false,
      blockedReason: 'needs_sync',
      nextAction: 'sync'
    })
  })

  it('blocks an unauthenticated gh with the authenticate action', async () => {
    const { creation } = createHarness({ authResult: NOT_AUTHENTICATED })
    const result = await creation.getCreationEligibility(BASE_ARGS)
    expect(result).toMatchObject({
      canCreate: false,
      blockedReason: 'auth_required',
      nextAction: 'authenticate'
    })
  })

  it('blocks an ahead branch with the push action', async () => {
    const { creation } = createHarness()
    const result = await creation.getCreationEligibility({ ...BASE_ARGS, ahead: 1 })
    expect(result).toMatchObject({
      canCreate: false,
      blockedReason: 'needs_push',
      nextAction: 'push'
    })
  })

  it('blocks a local-only base when the preflight enforces it', async () => {
    const { creation } = createHarness({
      runGit: async () => NO_MATCH()
    })
    const result = await creation.getCreationEligibility({
      ...BASE_ARGS,
      base: 'feature-parent',
      enforceBaseOnRemote: true
    })
    expect(result).toMatchObject({
      canCreate: false,
      blockedReason: 'base_not_on_remote',
      nextAction: null,
      defaultBaseRef: 'feature-parent'
    })
  })

  it('allows creation when every gate passes', async () => {
    const { creation } = createHarness()
    const result = await creation.getCreationEligibility(BASE_ARGS)
    expect(result).toMatchObject({
      provider: 'github',
      review: null,
      canCreate: true,
      blockedReason: null,
      nextAction: null,
      reviewLookupOutcome: 'not_found',
      defaultBaseRef: 'origin/main',
      head: 'feature'
    })
    expect(result).not.toHaveProperty('stackedCreationSupported')
  })
})

describe('getCreationEligibility wiring', () => {
  it('resolves the worktree path, strips the branch prefix, and forwards linked PR hints', async () => {
    const { creation, getRepoSlug, forBranch, makeRunGitPaths } = createHarness()
    const result = await creation.getCreationEligibility({
      ...BASE_ARGS,
      branch: 'refs/heads/feature',
      currentHeadOid: 'abc123',
      linkedGitHubPR: 7,
      fallbackGitHubPR: 9
    })
    expect(getRepoSlug).toHaveBeenCalledWith('/worktree')
    expect(makeRunGitPaths).toEqual(['/worktree'])
    expect(forBranch).toHaveBeenCalledWith({
      repoPath: '/worktree',
      branch: 'feature',
      currentHeadOid: 'abc123',
      active: true,
      linkedGitHubPR: 7,
      fallbackGitHubPR: 9
    })
    expect(result.head).toBe('feature')
  })

  it('falls back to repoPath when no worktree path is given', async () => {
    const { creation, getRepoSlug, makeRunGitPaths } = createHarness()
    const { worktreePath: _worktreePath, ...noWorktree } = BASE_ARGS
    void _worktreePath
    await creation.getCreationEligibility(noWorktree)
    expect(getRepoSlug).toHaveBeenCalledWith('/repo')
    expect(makeRunGitPaths).toEqual(['/repo'])
  })
})

describe('getDefaultBaseRef', () => {
  it('returns the verified origin/HEAD symbolic ref', async () => {
    const runGit = vi.fn<RunGit>(async (args) => {
      if (args[0] === 'symbolic-ref') {
        return { stdout: 'refs/remotes/origin/develop\n' }
      }
      if (args[0] === 'rev-parse') {
        return { stdout: 'abc\n' }
      }
      throw new Error(`unexpected ${args.join(' ')}`)
    })
    await expect(getDefaultBaseRef(runGit)).resolves.toBe('origin/develop')
    expect(runGit.mock.calls).toEqual([
      [['symbolic-ref', '--quiet', 'refs/remotes/origin/HEAD']],
      [['rev-parse', '--verify', '--quiet', 'refs/remotes/origin/develop']]
    ])
  })

  it('falls back to the probe order when the symbolic ref is missing', async () => {
    const runGit = vi.fn<RunGit>(async (args) => {
      if (args[0] === 'symbolic-ref') {
        return NO_MATCH()
      }
      if (args[0] === 'rev-parse' && args[3] === 'refs/remotes/origin/main') {
        return { stdout: 'abc\n' }
      }
      return NO_MATCH()
    })
    await expect(getDefaultBaseRef(runGit)).resolves.toBe('origin/main')
    expect(runGit.mock.calls).toEqual([
      [['symbolic-ref', '--quiet', 'refs/remotes/origin/HEAD']],
      [['rev-parse', '--verify', '--quiet', 'refs/remotes/origin/main']]
    ])
  })

  it('returns null when every probe misses', async () => {
    const runGit = vi.fn<RunGit>(async () => NO_MATCH())
    await expect(getDefaultBaseRef(runGit)).resolves.toBeNull()
    expect(runGit.mock.calls.map(([args]) => args)).toEqual([
      ['symbolic-ref', '--quiet', 'refs/remotes/origin/HEAD'],
      ['rev-parse', '--verify', '--quiet', 'refs/remotes/origin/main'],
      ['rev-parse', '--verify', '--quiet', 'refs/remotes/origin/master'],
      ['rev-parse', '--verify', '--quiet', 'refs/heads/main'],
      ['rev-parse', '--verify', '--quiet', 'refs/heads/master']
    ])
  })
})

describe('baseRefExistsOnRemote', () => {
  it('normalizes origin/ and probes the exact tracking ref', async () => {
    const runGit = vi.fn<RunGit>(async (args) => {
      if (args[0] === 'show-ref' && args[4] === 'refs/remotes/origin/main') {
        return { stdout: '' }
      }
      return NO_MATCH()
    })
    await expect(baseRefExistsOnRemote(runGit, 'origin/main')).resolves.toBe(true)
    expect(runGit.mock.calls).toEqual([
      [['show-ref', '--verify', '--quiet', '--', 'refs/remotes/origin/main']]
    ])
  })

  it('rejects an unsafe base without running git', async () => {
    const runGit = vi.fn<RunGit>(async () => ({ stdout: '' }))
    await expect(baseRefExistsOnRemote(runGit, 'feature..branch')).resolves.toBe(false)
    await expect(baseRefExistsOnRemote(runGit, 'feature*')).resolves.toBe(false)
    expect(runGit).not.toHaveBeenCalled()
  })

  it('probes a qualified base as a complete tracking ref first', async () => {
    const runGit = vi.fn<RunGit>(async (args) => {
      if (args[0] === 'show-ref' && args[4] === 'refs/remotes/fork/release/1.0') {
        return { stdout: '' }
      }
      return NO_MATCH()
    })
    await expect(baseRefExistsOnRemote(runGit, 'fork/release/1.0')).resolves.toBe(true)
    expect(runGit.mock.calls).toEqual([
      [['show-ref', '--verify', '--quiet', '--', 'refs/remotes/fork/release/1.0']]
    ])
  })

  it('finds a single-segment remote suffix with the show-ref scan', async () => {
    const runGit = vi.fn<RunGit>(async (args) => {
      if (args[0] === 'show-ref' && args[1] === '--') {
        return {
          stdout: 'aaa refs/remotes/origin/main\nbbb refs/remotes/origin/feature/main\n'
        }
      }
      return NO_MATCH()
    })
    await expect(baseRefExistsOnRemote(runGit, 'main')).resolves.toBe(true)
    expect(runGit.mock.calls.at(-1)).toEqual([['show-ref', '--', 'main']])
  })

  it('ignores suffix matches whose remote component spans segments', async () => {
    const runGit = vi.fn<RunGit>(async (args) => {
      if (args[0] === 'show-ref' && args[1] === '--') {
        return { stdout: 'aaa refs/remotes/origin/feature/main\n' }
      }
      return NO_MATCH()
    })
    await expect(baseRefExistsOnRemote(runGit, 'main')).resolves.toBe(false)
  })

  it('ignores a remote symbolic HEAD in the suffix scan', async () => {
    const runGit = vi.fn<RunGit>(async (args) => {
      if (args[0] === 'show-ref' && args[1] === '--') {
        return { stdout: 'aaa refs/remotes/origin/HEAD\n' }
      }
      return NO_MATCH()
    })
    await expect(baseRefExistsOnRemote(runGit, 'HEAD')).resolves.toBe(false)
  })

  it('fails open when a probe throws an unexpected error', async () => {
    const runGit = vi.fn<RunGit>(async () => {
      throw new Error('ssh dropped')
    })
    await expect(baseRefExistsOnRemote(runGit, 'main')).resolves.toBe(true)
  })

  it('fails open on a fatal (non exit-1) GitReadError', async () => {
    const runGit = vi.fn<RunGit>(async () => {
      throw new GitReadError({ stdout: '', stderr: 'fatal: not a git repository', code: 128 })
    })
    await expect(baseRefExistsOnRemote(runGit, 'main')).resolves.toBe(true)
  })

  it('fails open on an exit-1 GitReadError that still printed stderr', async () => {
    const runGit = vi.fn<RunGit>(async () => {
      throw new GitReadError({ stdout: '', stderr: 'error: transport dropped', code: 1 })
    })
    await expect(baseRefExistsOnRemote(runGit, 'main')).resolves.toBe(true)
  })
})

describe('defaultBaseRef semantics', () => {
  it('keeps the candidate base when it exists on a remote', async () => {
    const { creation, runGit } = createHarness({
      runGit: async (args) => {
        if (args[0] === 'show-ref' && args[4] === 'refs/remotes/origin/release') {
          return { stdout: '' }
        }
        return NO_MATCH()
      }
    })
    const result = await creation.getCreationEligibility({ ...BASE_ARGS, base: 'origin/release' })
    expect(result.defaultBaseRef).toBe('origin/release')
    expect(runGit.mock.calls.some(([args]) => args[0] === 'symbolic-ref')).toBe(false)
  })

  it('falls back to the repo default when the candidate is absent', async () => {
    const { creation } = createHarness({
      runGit: async (args) => {
        if (args[0] === 'rev-parse' && args[3] === 'refs/remotes/origin/main') {
          return { stdout: 'abc\n' }
        }
        return NO_MATCH()
      }
    })
    const result = await creation.getCreationEligibility({ ...BASE_ARGS, base: 'local-parent' })
    expect(result.defaultBaseRef).toBe('origin/main')
  })

  it('keeps the candidate when no repo default resolves', async () => {
    const { creation } = createHarness({ runGit: async () => NO_MATCH() })
    const result = await creation.getCreationEligibility({ ...BASE_ARGS, base: 'local-parent' })
    expect(result.defaultBaseRef).toBe('local-parent')
    expect(result.canCreate).toBe(true)
  })
})

describe('auth probe', () => {
  it('accepts a non-zero auth status that parses an active account', async () => {
    const { creation, run } = createHarness({
      authResult: {
        stdout:
          'Logged in to github.com account octocat (keyring)\n  - Active account: true\n',
        stderr: '',
        code: 1
      }
    })
    const result = await creation.getCreationEligibility(BASE_ARGS)
    expect(run).toHaveBeenCalledWith(['auth', 'status', '--hostname', 'github.com'])
    expect(result.canCreate).toBe(true)
  })

  it('blocks a parsed inactive account', async () => {
    const { creation } = createHarness({
      authResult: {
        stdout: 'Logged in to github.com account octocat (keyring)\n  - Active account: false\n',
        stderr: '',
        code: 0
      }
    })
    const result = await creation.getCreationEligibility(BASE_ARGS)
    expect(result.blockedReason).toBe('auth_required')
  })

  it('blocks with auth_required when the gh client cannot spawn', async () => {
    const { creation } = createHarness({ authError: new Error('gh: command not found') })
    const result = await creation.getCreationEligibility(BASE_ARGS)
    expect(result).toMatchObject({
      canCreate: false,
      blockedReason: 'auth_required',
      nextAction: 'authenticate'
    })
  })
})

describe('review lookup outcomes', () => {
  it('marks the lookup unavailable and refuses creation when the lookup throws', async () => {
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => {})
    try {
      const { creation } = createHarness({
        reviewError: new Error('GitHub PR lookup failed (network): boom')
      })
      const result = await creation.getCreationEligibility(BASE_ARGS)
      expect(result).toMatchObject({
        review: null,
        reviewLookupOutcome: 'unavailable',
        canCreate: false,
        blockedReason: null,
        nextAction: null
      })
    } finally {
      warn.mockRestore()
    }
  })
})
