import { invoke } from '@tauri-apps/api/core'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { createGhRealApi } from './gh'

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }))

const invokeMock = vi.mocked(invoke)

const AUTH_STATUS = [
  'github.com',
  '  ✓ Logged in to github.com account alice (keyring)',
  '  - Active account: true',
  "  - Token scopes: 'project', 'read:org', 'repo'",
  ''
].join('\n')

const RATE_LIMIT_PAYLOAD = JSON.stringify({
  resources: {
    core: { limit: 5000, remaining: 4999, reset: 1700000000 },
    search: { limit: 30, remaining: 30, reset: 1700000000 },
    graphql: { limit: 5000, remaining: 4999, reset: 1700000000 }
  }
})

const EMPTY_CHECKS_RESPONSE = JSON.stringify({
  data: {
    repository: {
      pullRequest: {
        headRefOid: 'head-sha',
        commits: {
          nodes: [
            {
              commit: {
                statusCheckRollup: { contexts: { nodes: [] } },
                checkSuites: { nodes: [] }
              }
            }
          ]
        }
      }
    }
  }
})

const PR_VIEW_42_DATA = {
  number: 42,
  title: 'Add widget',
  state: 'OPEN',
  url: 'https://github.com/acme/widgets/pull/42',
  statusCheckRollup: [],
  updatedAt: '2026-01-01T00:00:00Z',
  isDraft: false,
  mergeable: 'MERGEABLE',
  reviewDecision: 'APPROVED',
  mergeStateStatus: 'CLEAN',
  autoMergeRequest: null,
  baseRefName: 'main',
  headRefName: 'feature',
  baseRefOid: 'base-sha',
  headRefOid: 'head-sha'
}

const PR_VIEW_42 = JSON.stringify(PR_VIEW_42_DATA)

const MERGED_PR_VIEW_42 = JSON.stringify({ ...PR_VIEW_42_DATA, state: 'MERGED' })

function ghExecResult(stdout: string, code = 0) {
  return { stdout, stderr: '', code }
}

function ghExecArgs(payload: unknown): string[] {
  return (payload as { args: { args: string[] } }).args.args
}

const originRemote = { name: 'origin', url: 'https://github.com/acme/widgets.git' }

beforeEach(() => {
  invokeMock.mockReset()
})

describe('gh real api', () => {
  it('routes diagnoseAuth through gh auth status', async () => {
    invokeMock.mockImplementation(async (command: string) => {
      if (command === 'gh_env_probe') return { token: null }
      if (command === 'gh_exec') return ghExecResult(AUTH_STATUS)
      throw new Error(`unexpected ${command}`)
    })
    const api = createGhRealApi()
    await expect(api.diagnoseAuth()).resolves.toMatchObject({
      ghAvailable: true,
      activeAccount: { user: 'alice' }
    })
    expect(invokeMock.mock.calls.map(([command]) => command)).toEqual(['gh_exec', 'gh_env_probe'])
  })

  it('routes rateLimit through gh api rate_limit', async () => {
    invokeMock.mockImplementation(async (command: string) => {
      if (command === 'gh_exec') return ghExecResult(RATE_LIMIT_PAYLOAD)
      throw new Error(`unexpected ${command}`)
    })
    const api = createGhRealApi()
    const result = await api.rateLimit()
    expect(result.ok).toBe(true)
    expect(invokeMock).toHaveBeenCalledWith('gh_exec', expect.anything())
    expect(ghExecArgs(invokeMock.mock.calls[0]?.[1])).toEqual(['api', 'rate_limit'])
  })

  it('keeps unported gh methods rejecting as unimplemented', async () => {
    const api = createGhRealApi()
    await expect(api.mergePR({ repoPath: '/repo', prNumber: 1 })).rejects.toMatchObject({
      name: 'UnimplementedBridgeError'
    })
  })

  it('resolves the repo slug and routes prChecks through GraphQL', async () => {
    invokeMock.mockImplementation(async (command: string, payload?: unknown) => {
      if (command === 'git_remote_urls') return [originRemote]
      if (command === 'gh_exec') {
        expect(ghExecArgs(payload)).toEqual(expect.arrayContaining(['owner=acme', 'repo=widgets']))
        return ghExecResult(EMPTY_CHECKS_RESPONSE)
      }
      throw new Error(`unexpected ${command}`)
    })
    const api = createGhRealApi()
    await expect(api.prChecks({ repoPath: '/repo', prNumber: 7 })).resolves.toEqual([])
  })

  it('returns no checks without touching gh when the worktree has no GitHub identity', async () => {
    invokeMock.mockImplementation(async (command: string) => {
      if (command === 'git_remote_urls') return []
      throw new Error(`unexpected ${command}`)
    })
    const api = createGhRealApi()
    await expect(api.prChecks({ repoPath: '/repo', prNumber: 7 })).resolves.toEqual([])
    expect(invokeMock.mock.calls.map(([command]) => command)).toEqual(['git_remote_urls'])
  })

  it('prefers the prRepo override without resolving the worktree identity', async () => {
    invokeMock.mockImplementation(async (command: string, payload?: unknown) => {
      if (command === 'gh_exec') {
        expect(ghExecArgs(payload)).toEqual(expect.arrayContaining(['owner=org', 'repo=fork']))
        return ghExecResult(EMPTY_CHECKS_RESPONSE)
      }
      throw new Error(`unexpected ${command}`)
    })
    const api = createGhRealApi()
    await expect(
      api.prChecks({ repoPath: '/repo', prNumber: 7, prRepo: { owner: 'org', repo: 'fork' } })
    ).resolves.toEqual([])
  })

  it('maps the refresh candidate onto the branch lookup', async () => {
    invokeMock.mockImplementation(async (command: string) => {
      if (command === 'git_remote_urls') return [originRemote]
      if (command === 'gh_exec') return ghExecResult(PR_VIEW_42)
      throw new Error(`unexpected ${command}`)
    })
    const api = createGhRealApi()
    await expect(
      api.refreshPRNow({
        candidate: {
          cacheKey: 'k',
          repoId: 'r1',
          repoPath: '/repo',
          branch: 'feature',
          repoKind: 'git',
          linkedPRNumber: 42
        }
      })
    ).resolves.toMatchObject({ kind: 'found', pr: { number: 42 } })
  })

  it('keeps a merged fallback PR when the candidate carries a fallbackPRSource', async () => {
    invokeMock.mockImplementation(async (command: string) => {
      if (command === 'git_remote_urls') return [originRemote]
      if (command === 'gh_exec') return ghExecResult(MERGED_PR_VIEW_42)
      throw new Error(`unexpected ${command}`)
    })
    const api = createGhRealApi()
    await expect(
      api.refreshPRNow({
        candidate: {
          cacheKey: 'k',
          repoId: 'r1',
          repoPath: '/repo',
          branch: '',
          repoKind: 'git',
          fallbackPRNumber: 42,
          fallbackPRSource: 'explicit'
        }
      })
    ).resolves.toMatchObject({ kind: 'found', pr: { number: 42, state: 'merged' } })
  })

  it('hides a merged fallback PR when the candidate has no fallbackPRSource', async () => {
    invokeMock.mockImplementation(async (command: string) => {
      if (command === 'git_remote_urls') return [originRemote]
      if (command === 'gh_exec') return ghExecResult(MERGED_PR_VIEW_42)
      throw new Error(`unexpected ${command}`)
    })
    const api = createGhRealApi()
    await expect(
      api.refreshPRNow({
        candidate: {
          cacheKey: 'k',
          repoId: 'r1',
          repoPath: '/repo',
          branch: '',
          repoKind: 'git',
          fallbackPRNumber: 42
        }
      })
    ).resolves.toMatchObject({ kind: 'no-pr' })
  })

  it('resolves diagnoseAuth with a null env token when the env probe rejects', async () => {
    invokeMock.mockImplementation(async (command: string) => {
      if (command === 'gh_exec') return ghExecResult(AUTH_STATUS)
      if (command === 'gh_env_probe') throw new Error('env probe failed')
      throw new Error(`unexpected ${command}`)
    })
    const api = createGhRealApi()
    await expect(api.diagnoseAuth()).resolves.toMatchObject({
      ghAvailable: true,
      activeAccount: { user: 'alice' },
      envTokenInProcess: null
    })
  })

  it('keeps gh available when the auth status runner times out', async () => {
    invokeMock.mockImplementation(async (command: string) => {
      if (command === 'gh_exec') throw new Error('gh exec timed out')
      if (command === 'gh_env_probe') return { token: null }
      throw new Error(`unexpected ${command}`)
    })
    const api = createGhRealApi()
    await expect(api.diagnoseAuth()).resolves.toMatchObject({
      ghAvailable: true,
      accounts: [],
      activeAccount: null
    })
  })

  it('keeps gh available when the auth status runner fails without a spawn signature', async () => {
    invokeMock.mockImplementation(async (command: string) => {
      if (command === 'gh_exec') throw new Error('ipc transport failed')
      if (command === 'gh_env_probe') return { token: null }
      throw new Error(`unexpected ${command}`)
    })
    const api = createGhRealApi()
    await expect(api.diagnoseAuth()).resolves.toMatchObject({
      ghAvailable: true,
      accounts: [],
      activeAccount: null
    })
  })

  it('returns no checks for a non-default host repo without touching gh', async () => {
    invokeMock.mockImplementation(async (command: string) => {
      throw new Error(`unexpected ${command}`)
    })
    const api = createGhRealApi()
    await expect(
      api.prChecks({
        repoPath: '/repo',
        prNumber: 7,
        prRepo: { owner: 'org', repo: 'repo', host: 'ghe.internal:8443' }
      })
    ).resolves.toEqual([])
    expect(invokeMock).not.toHaveBeenCalled()
  })

  it('returns null check details for a non-default host repo without touching gh', async () => {
    invokeMock.mockImplementation(async (command: string) => {
      throw new Error(`unexpected ${command}`)
    })
    const api = createGhRealApi()
    await expect(
      api.prCheckDetails({
        repoPath: '/repo',
        prRepo: { owner: 'org', repo: 'repo', host: 'ghe.internal:8443' },
        checkRunId: 5
      })
    ).resolves.toBeNull()
    expect(invokeMock).not.toHaveBeenCalled()
  })

  it('routes prCheckDetails through the REST check-run endpoint', async () => {
    const invokedArgs: string[][] = []
    invokeMock.mockImplementation(async (command: string, payload?: unknown) => {
      if (command === 'gh_exec') {
        const args = ghExecArgs(payload)
        invokedArgs.push(args)
        if (args[1]?.endsWith('/check-runs/5')) {
          return ghExecResult(
            JSON.stringify({
              name: 'build',
              status: 'completed',
              conclusion: 'success',
              html_url: 'https://example.com/run/5',
              details_url: null
            })
          )
        }
        if (args[1]?.includes('/check-runs/5/annotations')) return ghExecResult('[]')
        throw new Error(`unexpected gh ${args.join(' ')}`)
      }
      throw new Error(`unexpected ${command}`)
    })
    const api = createGhRealApi()
    await expect(
      api.prCheckDetails({
        repoPath: '/repo',
        prRepo: { owner: 'acme', repo: 'widgets' },
        checkRunId: 5
      })
    ).resolves.toMatchObject({ name: 'build', conclusion: 'success' })
    expect(invokedArgs.map((args) => args[1])).toContain('repos/acme/widgets/check-runs/5')
  })
})
