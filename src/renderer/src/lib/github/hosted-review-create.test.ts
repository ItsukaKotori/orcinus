import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { GitStatusEntry, GitUpstreamStatus } from '../../../../shared/git-status-types'
import type {
  CreateHostedReviewArgs,
  HostedReviewCreationBlockedReason,
  HostedReviewCreationEligibility,
  HostedReviewInfo
} from '../../../../shared/hosted-review'
import type { GhExecResult } from './gh-exec-client'
import { GitReadError } from './git-read-client'
import {
  baseRefExistsOnRemote,
  blockedEligibilityToCreateResult,
  createHostedReviewCreation,
  getDefaultBaseRef
} from './hosted-review-create'
import type {
  HostedReviewCreationEligibilityInput,
  HostedReviewCreationReadTemplate
} from './hosted-review-create'
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
  currentBranch?: string
  status?: { entries: readonly GitStatusEntry[] } | Error
  upstream?: GitUpstreamStatus | Error
  template?: HostedReviewCreationReadTemplate
  createResult?: GhExecResult | Error
  listResult?: GhExecResult | Error
}

const PR_URL = 'https://github.com/org/repo/pull/7'

const CREATED_JSON: GhExecResult = {
  stdout: JSON.stringify({ number: 7, url: PR_URL }),
  stderr: '',
  code: 0
}

const CREATE_ARGS: CreateHostedReviewArgs = {
  repoPath: '/repo',
  worktreePath: '/worktree',
  provider: 'github',
  base: 'main',
  title: 'Add feature',
  body: 'Body'
}

function createHarness(options: HarnessOptions = {}) {
  const makeRunGitPaths: string[] = []
  const runGit = vi.fn<RunGit>(
    options.runGit ??
      (async (args) => {
        if (args[0] === 'rev-parse' && args[1] === '--abbrev-ref') {
          return { stdout: `${options.currentBranch ?? 'feature'}\n` }
        }
        return { stdout: '' }
      })
  )
  const getRepoSlug = vi.fn(async () => (options.slug === undefined ? ORG_REPO : options.slug))
  const forBranch = vi.fn(async () => {
    if (options.reviewError) {
      throw options.reviewError
    }
    return options.review ?? null
  })
  const invalidate = vi.fn()
  const run = vi.fn(async (args: string[]) => {
    if (args[0] === 'auth') {
      if (options.authError) {
        throw options.authError
      }
      return options.authResult ?? ACTIVE_AUTH
    }
    if (args[0] === 'pr' && args[1] === 'create') {
      if (options.createResult instanceof Error) {
        throw options.createResult
      }
      return options.createResult ?? CREATED_JSON
    }
    if (args[0] === 'pr' && args[1] === 'list') {
      if (options.listResult instanceof Error) {
        throw options.listResult
      }
      return options.listResult ?? { stdout: '[]', stderr: '', code: 0 }
    }
    throw new Error(`unexpected gh call: ${args.join(' ')}`)
  })
  const readStatus = vi.fn(async () => {
    if (options.status instanceof Error) {
      throw options.status
    }
    return options.status ?? { entries: [] }
  })
  const readUpstream = vi.fn(async () => {
    if (options.upstream instanceof Error) {
      throw options.upstream
    }
    return options.upstream ?? { hasUpstream: true, ahead: 0, behind: 0 }
  })
  const readTemplate = vi.fn<HostedReviewCreationReadTemplate>(
    options.template ?? (async () => null)
  )
  const creation = createHostedReviewCreation({
    client: { run },
    identity: { getRepoSlug },
    reviewLookup: { forBranch, invalidate },
    makeRunGit: (worktreePath) => {
      makeRunGitPaths.push(worktreePath)
      return runGit
    },
    readStatus,
    readUpstream,
    readTemplate,
    now: () => NOW
  })
  return {
    creation,
    runGit,
    getRepoSlug,
    forBranch,
    run,
    makeRunGitPaths,
    invalidate,
    readStatus,
    readUpstream,
    readTemplate
  }
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

describe('create', () => {
  beforeEach(() => {
    vi.spyOn(console, 'warn').mockImplementation(() => {})
  })
  afterEach(() => {
    vi.restoreAllMocks()
  })

  describe('preflight', () => {
    it('blocks creation when the checked-out branch differs from the selected head', async () => {
      const { creation, run, runGit, makeRunGitPaths } = createHarness({ currentBranch: 'other' })
      const result = await creation.create({ ...CREATE_ARGS, head: 'feature' })
      expect(result).toEqual({
        ok: false,
        code: 'validation',
        error:
          'Create PR failed: switch back to the selected branch before creating a pull request.'
      })
      expect(makeRunGitPaths).toEqual(['/worktree'])
      expect(runGit).toHaveBeenCalledWith(['rev-parse', '--abbrev-ref', 'HEAD'])
      expect(run).not.toHaveBeenCalled()
    })

    it('blocks creation from a dirty worktree', async () => {
      const { creation } = createHarness({
        status: { entries: [{ path: 'a.ts', status: 'modified', area: 'unstaged' }] }
      })
      const result = await creation.create(CREATE_ARGS)
      expect(result).toEqual({
        ok: false,
        code: 'validation',
        error:
          'Create PR failed: commit or discard local changes before creating a pull request.'
      })
    })

    it('blocks creation without an upstream', async () => {
      const { creation } = createHarness({
        upstream: { hasUpstream: false, ahead: 0, behind: 0 }
      })
      const result = await creation.create(CREATE_ARGS)
      expect(result).toEqual({
        ok: false,
        code: 'validation',
        error: 'Create PR failed: publish this branch before creating a pull request.'
      })
    })

    it('refuses creation when the review lookup is unavailable', async () => {
      const { creation } = createHarness({ reviewError: new Error('GitHub PR lookup failed') })
      const result = await creation.create(CREATE_ARGS)
      expect(result).toEqual({
        ok: false,
        code: 'validation',
        error:
          'Create PR failed: Orca could not confirm whether this branch already has a pull request. Retry once the GitHub lookup succeeds.'
      })
    })

    it('reports a validation failure when branch status cannot be read', async () => {
      const { creation } = createHarness({ status: new Error('git status failed') })
      const result = await creation.create(CREATE_ARGS)
      expect(result).toEqual({
        ok: false,
        code: 'validation',
        error:
          'Create PR failed: could not verify branch status. Refresh source control and try again.'
      })
    })
  })

  describe('argv and assignment', () => {
    it('runs gh pr create with the verbatim argv and pipes the body to stdin', async () => {
      const { creation, run, invalidate, readStatus, readUpstream } = createHarness()
      const result = await creation.create(CREATE_ARGS)
      expect(run).toHaveBeenCalledWith(
        [
          'pr',
          'create',
          '--repo',
          'org/repo',
          '--base',
          'main',
          '--title',
          'Add feature',
          '--body-file',
          '-'
        ],
        { timeoutMs: 60_000, stdin: 'Body', retry: false }
      )
      expect(readStatus).toHaveBeenCalledWith('/worktree')
      expect(readUpstream).toHaveBeenCalledWith('/worktree')
      expect(result).toEqual({ ok: true, number: 7, url: PR_URL })
      expect(invalidate).toHaveBeenCalledTimes(1)
      expect(invalidate).toHaveBeenCalledWith('/repo')
    })

    it('appends --head and --draft when selected', async () => {
      const { creation, run } = createHarness()
      await creation.create({ ...CREATE_ARGS, head: 'feature', draft: true })
      expect(run).toHaveBeenCalledWith(
        [
          'pr',
          'create',
          '--repo',
          'org/repo',
          '--base',
          'main',
          '--title',
          'Add feature',
          '--body-file',
          '-',
          '--head',
          'feature',
          '--draft'
        ],
        { timeoutMs: 60_000, stdin: 'Body', retry: false }
      )
    })

    it('normalizes the base, head, and title before building the argv', async () => {
      const { creation, run } = createHarness()
      await creation.create({
        ...CREATE_ARGS,
        base: 'origin/main',
        head: 'refs/heads/feature',
        title: '  Add feature  '
      })
      expect(run).toHaveBeenCalledWith(
        [
          'pr',
          'create',
          '--repo',
          'org/repo',
          '--base',
          'main',
          '--title',
          'Add feature',
          '--body-file',
          '-',
          '--head',
          'feature'
        ],
        { timeoutMs: 60_000, stdin: 'Body', retry: false }
      )
    })

    it('sends an empty stdin when there is no body', async () => {
      const { creation, run } = createHarness()
      await creation.create({ ...CREATE_ARGS, body: undefined })
      expect(run).toHaveBeenCalledWith(expect.any(Array), {
        timeoutMs: 60_000,
        stdin: '',
        retry: false
      })
    })

    it('rejects a missing base or title before calling gh pr create', async () => {
      const { creation, run } = createHarness()
      await expect(creation.create({ ...CREATE_ARGS, title: '   ' })).resolves.toEqual({
        ok: false,
        code: 'validation',
        error: 'Create PR failed: base branch and title are required.'
      })
      await expect(creation.create({ ...CREATE_ARGS, body: undefined, base: '' })).resolves.toEqual({
        ok: false,
        code: 'validation',
        error: 'Create PR failed: base branch and title are required.'
      })
      expect(run.mock.calls.filter(([argv]) => argv.includes('create'))).toEqual([])
    })
  })

  describe('template body', () => {
    it('reads the first conventional template path and pipes it to stdin', async () => {
      const readTemplate = vi.fn<HostedReviewCreationReadTemplate>(async (_path, relativePath) =>
        relativePath === '.github/pull_request_template.md' ? { content: 'Template body' } : null
      )
      const { creation, run } = createHarness({ template: readTemplate })
      await creation.create({ ...CREATE_ARGS, body: undefined, useTemplate: true })
      expect(readTemplate).toHaveBeenCalledWith('/worktree', '.github/pull_request_template.md')
      expect(run).toHaveBeenCalledWith(expect.any(Array), {
        timeoutMs: 60_000,
        stdin: 'Template body',
        retry: false
      })
    })

    it('tries the six conventional paths in order and falls back to an empty body', async () => {
      const readTemplate = vi.fn<HostedReviewCreationReadTemplate>(async () => null)
      const { creation, run } = createHarness({ template: readTemplate })
      await creation.create({ ...CREATE_ARGS, body: '', useTemplate: true })
      expect(readTemplate.mock.calls.map(([, relativePath]) => relativePath)).toEqual([
        '.github/pull_request_template.md',
        '.github/PULL_REQUEST_TEMPLATE.md',
        'pull_request_template.md',
        'PULL_REQUEST_TEMPLATE.md',
        'docs/pull_request_template.md',
        'docs/PULL_REQUEST_TEMPLATE.md'
      ])
      expect(run).toHaveBeenCalledWith(expect.any(Array), {
        timeoutMs: 60_000,
        stdin: '',
        retry: false
      })
    })

    it('skips binary templates', async () => {
      const readTemplate = vi.fn<HostedReviewCreationReadTemplate>(async (_path, relativePath) => {
        if (relativePath === '.github/pull_request_template.md') {
          return { content: 'binary', isBinary: true }
        }
        if (relativePath === '.github/PULL_REQUEST_TEMPLATE.md') {
          return { content: 'Markdown body' }
        }
        return null
      })
      const { creation, run } = createHarness({ template: readTemplate })
      await creation.create({ ...CREATE_ARGS, body: undefined, useTemplate: true })
      expect(run).toHaveBeenCalledWith(expect.any(Array), {
        timeoutMs: 60_000,
        stdin: 'Markdown body',
        retry: false
      })
    })

    it('prefers an explicit body over the template', async () => {
      const readTemplate = vi.fn<HostedReviewCreationReadTemplate>(async () => ({
        content: 'Template body'
      }))
      const { creation, run } = createHarness({ template: readTemplate })
      await creation.create({ ...CREATE_ARGS, useTemplate: true })
      expect(readTemplate).not.toHaveBeenCalled()
      expect(run).toHaveBeenCalledWith(expect.any(Array), {
        timeoutMs: 60_000,
        stdin: 'Body',
        retry: false
      })
    })
  })

  describe('stdout parsing and fallback', () => {
    it('parses a pull request URL from stdout on any host', async () => {
      const { creation } = createHarness({
        createResult: {
          stdout: 'https://ghe.corp.example/org/repo/pull/12\n',
          stderr: '',
          code: 0
        }
      })
      await expect(creation.create(CREATE_ARGS)).resolves.toEqual({
        ok: true,
        number: 12,
        url: 'https://ghe.corp.example/org/repo/pull/12'
      })
    })

    it('falls back to gh pr list when stdout has no parsable payload', async () => {
      const { creation, run, invalidate } = createHarness({
        createResult: {
          stdout: 'Creating pull request for feature into main\n',
          stderr: '',
          code: 0
        },
        listResult: {
          stdout: JSON.stringify([{ number: 5, url: 'https://github.com/org/repo/pull/5' }]),
          stderr: '',
          code: 0
        }
      })
      await expect(creation.create({ ...CREATE_ARGS, head: 'feature' })).resolves.toEqual({
        ok: true,
        number: 5,
        url: 'https://github.com/org/repo/pull/5'
      })
      expect(run).toHaveBeenCalledWith([
        'pr',
        'list',
        '--repo',
        'org/repo',
        '--head',
        'feature',
        '--base',
        'main',
        '--state',
        'open',
        '--limit',
        '2',
        '--json',
        'number,url'
      ])
      expect(invalidate).toHaveBeenCalledTimes(1)
    })

    it('reports unknown_completion when the fallback finds no open review', async () => {
      const { creation, invalidate } = createHarness({
        createResult: { stdout: 'noise', stderr: '', code: 0 },
        listResult: { stdout: '[]', stderr: '', code: 0 }
      })
      await expect(creation.create({ ...CREATE_ARGS, head: 'feature' })).resolves.toEqual({
        ok: false,
        code: 'unknown_completion',
        error: 'PR creation may have completed. Refreshing branch review state...'
      })
      expect(invalidate).not.toHaveBeenCalled()
    })

    it('reports unknown_completion when the fallback finds more than one review', async () => {
      const { creation } = createHarness({
        createResult: { stdout: 'noise', stderr: '', code: 0 },
        listResult: {
          stdout: JSON.stringify([
            { number: 5, url: 'https://github.com/org/repo/pull/5' },
            { number: 6, url: 'https://github.com/org/repo/pull/6' }
          ]),
          stderr: '',
          code: 0
        }
      })
      await expect(creation.create({ ...CREATE_ARGS, head: 'feature' })).resolves.toEqual({
        ok: false,
        code: 'unknown_completion',
        error: 'PR creation may have completed. Refreshing branch review state...'
      })
    })

    it('does not query gh pr list without a head branch', async () => {
      const { creation, run } = createHarness({
        createResult: { stdout: 'noise', stderr: '', code: 0 }
      })
      await expect(creation.create(CREATE_ARGS)).resolves.toEqual({
        ok: false,
        code: 'unknown_completion',
        error: 'PR creation may have completed. Refreshing branch review state...'
      })
      expect(run.mock.calls.filter(([argv]) => argv[1] === 'list')).toEqual([])
    })
  })

  describe('error classification', () => {
    it('classifies an auth failure verbatim', async () => {
      const { creation, invalidate } = createHarness({
        createResult: {
          stdout: '',
          stderr: 'You are not logged into any GitHub hosts. To log in, run: gh auth login',
          code: 1
        }
      })
      await expect(creation.create(CREATE_ARGS)).resolves.toEqual({
        ok: false,
        code: 'auth_required',
        error:
          'Create PR failed: GitHub is not authenticated. Next step: run gh auth login in this environment.'
      })
      expect(invalidate).not.toHaveBeenCalled()
    })

    it('classifies a failure printed on stdout', async () => {
      const { creation } = createHarness({
        createResult: { stdout: 'HTTP 401: Unauthorized', stderr: '', code: 1 }
      })
      await expect(creation.create(CREATE_ARGS)).resolves.toEqual({
        ok: false,
        code: 'auth_required',
        error:
          'Create PR failed: GitHub is not authenticated. Next step: run gh auth login in this environment.'
      })
    })

    it('resolves already_exists through the fallback review', async () => {
      const { creation, run } = createHarness({
        createResult: {
          stdout: '',
          stderr: 'a pull request already exists for branch feature',
          code: 1
        },
        listResult: {
          stdout: JSON.stringify([{ number: 9, url: 'https://github.com/org/repo/pull/9' }]),
          stderr: '',
          code: 0
        }
      })
      await expect(creation.create({ ...CREATE_ARGS, head: 'feature' })).resolves.toEqual({
        ok: false,
        code: 'already_exists',
        error: 'A pull request already exists for this branch.',
        existingReview: { number: 9, url: 'https://github.com/org/repo/pull/9' }
      })
      expect(run).toHaveBeenCalledWith([
        'pr',
        'list',
        '--repo',
        'org/repo',
        '--head',
        'feature',
        '--base',
        'main',
        '--state',
        'open',
        '--limit',
        '2',
        '--json',
        'number,url'
      ])
    })

    it('keeps already_exists without a fallback hit', async () => {
      const { creation } = createHarness({
        createResult: {
          stdout: '',
          stderr: 'a pull request already exists for branch feature',
          code: 1
        }
      })
      const result = await creation.create({ ...CREATE_ARGS, head: 'feature' })
      expect(result).toEqual({
        ok: false,
        code: 'already_exists',
        error: 'A pull request already exists for this branch.'
      })
      expect(result).not.toHaveProperty('existingReview')
    })

    it('resolves unknown_completion through the fallback review after a timeout', async () => {
      const { creation } = createHarness({
        createResult: { stdout: '', stderr: 'command timed out after 60s', code: null },
        listResult: {
          stdout: JSON.stringify([{ number: 9, url: 'https://github.com/org/repo/pull/9' }]),
          stderr: '',
          code: 0
        }
      })
      await expect(creation.create({ ...CREATE_ARGS, head: 'feature' })).resolves.toEqual({
        ok: false,
        code: 'already_exists',
        error: 'A pull request already exists for this branch.',
        existingReview: { number: 9, url: 'https://github.com/org/repo/pull/9' }
      })
    })

    it('keeps unknown_completion when a timeout finds no fallback review', async () => {
      const { creation } = createHarness({
        createResult: { stdout: '', stderr: 'command timed out after 60s', code: null }
      })
      await expect(creation.create({ ...CREATE_ARGS, head: 'feature' })).resolves.toEqual({
        ok: false,
        code: 'unknown_completion',
        error: 'PR creation may have completed. Refreshing branch review state...'
      })
    })

    it('classifies a validation failure verbatim', async () => {
      const { creation } = createHarness({
        createResult: {
          stdout: '',
          stderr: 'GraphQL: Validation failed (HTTP 422)',
          code: 1
        }
      })
      await expect(creation.create(CREATE_ARGS)).resolves.toEqual({
        ok: false,
        code: 'validation',
        error:
          'Create PR failed: GitHub rejected the pull request. Check the base branch and branch state, then try again.'
      })
    })

    it('classifies anything else as unknown', async () => {
      const { creation } = createHarness({
        createResult: { stdout: '', stderr: 'something exploded', code: 1 }
      })
      await expect(creation.create(CREATE_ARGS)).resolves.toEqual({
        ok: false,
        code: 'unknown',
        error:
          'Create PR failed: GitHub could not create the pull request. Try again in a moment.'
      })
    })
  })

  describe('provider guard', () => {
    it('rejects a provider without creation support before any lookup', async () => {
      const { creation, run, forBranch } = createHarness()
      await expect(creation.create({ ...CREATE_ARGS, provider: 'unsupported' })).resolves.toEqual({
        ok: false,
        code: 'unsupported_provider',
        error: 'Creating reviews for this provider is not supported yet.'
      })
      expect(run).not.toHaveBeenCalled()
      expect(forBranch).not.toHaveBeenCalled()
    })

    it('requires a GitHub origin remote', async () => {
      const { creation, getRepoSlug } = createHarness({ slug: null })
      await expect(creation.create(CREATE_ARGS)).resolves.toEqual({
        ok: false,
        code: 'unsupported_provider',
        error: 'Creating pull requests requires a GitHub remote.'
      })
      expect(getRepoSlug).toHaveBeenCalledWith('/worktree')
    })

    it('rejects a non-default GitHub host', async () => {
      const { creation } = createHarness({
        slug: { owner: 'org', repo: 'repo', host: 'ghe.corp.example' }
      })
      await expect(creation.create(CREATE_ARGS)).resolves.toEqual({
        ok: false,
        code: 'unsupported_provider',
        error: 'Creating pull requests requires a GitHub remote.'
      })
    })

    it('rejects a mismatched provider with its own copy', async () => {
      const { creation } = createHarness()
      await expect(creation.create({ ...CREATE_ARGS, provider: 'gitlab' })).resolves.toEqual({
        ok: false,
        code: 'unsupported_provider',
        error: 'Creating merge requests requires a GitLab remote.'
      })
    })
  })
})

function eligibilityFor(
  blockedReason: HostedReviewCreationBlockedReason,
  overrides: Partial<HostedReviewCreationEligibility> = {}
): HostedReviewCreationEligibility {
  return {
    provider: 'github',
    review: null,
    canCreate: false,
    blockedReason,
    nextAction: null,
    reviewLookupOutcome: 'not_found',
    ...overrides
  }
}

const BLOCKED_COPY_ROWS = [
  {
    reason: 'auth_required',
    code: 'auth_required',
    error:
      'Create PR failed: GitHub is not authenticated. Next step: Run gh auth login in this environment.'
  },
  {
    reason: 'unsupported_provider',
    code: 'unsupported_provider',
    error: 'Creating pull requests requires a GitHub remote.'
  },
  {
    reason: 'dirty',
    code: 'validation',
    error:
      'Create PR failed: commit or discard local changes before creating a pull request.'
  },
  {
    reason: 'detached_head',
    code: 'validation',
    error: 'Create PR failed: switch to a branch before creating a pull request.'
  },
  {
    reason: 'default_branch',
    code: 'validation',
    error: 'Create PR failed: choose a feature branch before creating a pull request.'
  },
  {
    reason: 'no_upstream',
    code: 'validation',
    error: 'Create PR failed: publish this branch before creating a pull request.'
  },
  {
    reason: 'needs_push',
    code: 'validation',
    error: 'Create PR failed: push this branch before creating a pull request.'
  },
  {
    reason: 'needs_sync',
    code: 'validation',
    error: 'Create PR failed: sync this branch before creating a pull request.'
  },
  {
    reason: 'fork_head_unsupported',
    code: 'validation',
    error: 'Create PR failed: refresh source control status and try again.'
  },
  {
    reason: 'base_not_on_remote',
    code: 'validation',
    error:
      'Create PR failed: the base branch "release" hasn\'t been pushed to the remote. Choose a pushed base or push it first.'
  }
] as const satisfies readonly {
  reason: NonNullable<HostedReviewCreationBlockedReason>
  code: string
  error: string
}[]

describe('blockedEligibilityToCreateResult', () => {
  for (const row of BLOCKED_COPY_ROWS) {
    it(`maps ${row.reason} to ${row.code} with verbatim copy`, () => {
      expect(blockedEligibilityToCreateResult(eligibilityFor(row.reason), 'release')).toEqual({
        ok: false,
        code: row.code,
        error: row.error
      })
    })
  }

  it('returns null when creation is allowed', () => {
    expect(blockedEligibilityToCreateResult(eligibilityFor(null, { canCreate: true }))).toBeNull()
  })

  it('maps an empty blocker to the refresh copy', () => {
    expect(blockedEligibilityToCreateResult(eligibilityFor(null))).toEqual({
      ok: false,
      code: 'validation',
      error: 'Create PR failed: refresh source control status and try again.'
    })
  })

  it('maps an existing review to already_exists with the review summary', () => {
    const review = { number: 7, url: PR_URL }
    expect(blockedEligibilityToCreateResult(eligibilityFor('existing_review', { review }))).toEqual({
      ok: false,
      code: 'already_exists',
      error: 'A pull request already exists for this branch.',
      existingReview: review
    })
  })

  it('uses the provider copy for a non-GitHub provider', () => {
    expect(
      blockedEligibilityToCreateResult(eligibilityFor('unsupported_provider', { provider: 'gitlab' }))
    ).toEqual({
      ok: false,
      code: 'unsupported_provider',
      error: 'Creating merge requests requires a GitLab remote.'
    })
  })
})
