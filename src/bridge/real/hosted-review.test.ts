import { invoke } from '@tauri-apps/api/core'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { createHostedReviewRealApi } from './hosted-review'

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }))

const invokeMock = vi.mocked(invoke)

type ExecResult = { stdout: string; stderr: string; code: number }

const AUTH_STATUS = [
  'github.com',
  '  ✓ Logged in to github.com account alice (keyring)',
  '  - Active account: true',
  "  - Token scopes: 'project', 'read:org', 'repo'",
  ''
].join('\n')

const NOT_AUTHENTICATED: ExecResult = {
  stdout: '',
  stderr: 'You are not logged into any GitHub hosts. To log in, run: gh auth login\n',
  code: 1
}

const PR_URL_7 = 'https://github.com/acme/widgets/pull/7'

function ghExecArgs(payload: unknown): string[] {
  return (payload as { args: { args: string[] } }).args.args
}

/** Mock the commands the assembled real domain reaches for the happy path. */
function mockBackend(options: { auth?: ExecResult } = {}): void {
  invokeMock.mockImplementation(async (command: string, payload?: unknown) => {
    if (command === 'git_remote_urls') {
      return [{ name: 'origin', url: 'https://github.com/acme/widgets.git' }]
    }
    if (command === 'git_read') {
      const args = (payload as { args: { args: string[] } }).args.args
      if (args[0] === 'rev-parse' && args[1] === '--abbrev-ref') {
        return { stdout: 'feature\n', stderr: '', code: 0 }
      }
      return { stdout: '', stderr: '', code: 0 }
    }
    if (command === 'git_status') return { entries: [] }
    if (command === 'git_upstream_status') return { hasUpstream: true, ahead: 0, behind: 0 }
    if (command === 'gh_exec') {
      const args = ghExecArgs(payload)
      if (args[0] === 'auth') return options.auth ?? { stdout: AUTH_STATUS, stderr: '', code: 0 }
      if (args[0] === 'api') return { stdout: '[]', stderr: '', code: 0 }
      if (args[0] === 'pr' && args[1] === 'create') {
        return { stdout: JSON.stringify({ number: 7, url: PR_URL_7 }), stderr: '', code: 0 }
      }
      throw new Error(`unexpected gh ${args.join(' ')}`)
    }
    throw new Error(`unexpected ${command}`)
  })
}

const PR_VIEW_42 = JSON.stringify({
  number: 42,
  title: 'Add widget',
  state: 'OPEN',
  url: 'https://github.com/acme/widgets/pull/42',
  statusCheckRollup: [],
  updatedAt: '2026-01-01T00:00:00Z',
  isDraft: false,
  mergeable: 'MERGEABLE',
  baseRefName: 'main',
  headRefName: 'feature',
  baseRefOid: 'base-sha',
  headRefOid: 'head-sha'
})

beforeEach(() => {
  invokeMock.mockReset()
})

describe('hostedReview real api', () => {
  it('resolves forBranch through the real gh PR lookup', async () => {
    invokeMock.mockImplementation(async (command: string) => {
      if (command === 'git_remote_urls') {
        return [{ name: 'origin', url: 'https://github.com/acme/widgets.git' }]
      }
      if (command === 'gh_exec') return { stdout: PR_VIEW_42, stderr: '', code: 0 }
      throw new Error(`unexpected ${command}`)
    })
    const api = createHostedReviewRealApi()
    await expect(
      api.forBranch({
        repoPath: '/repo',
        branch: 'feature',
        linkedGitHubPR: 42,
        currentHeadOid: 'head-sha'
      })
    ).resolves.toMatchObject({
      provider: 'github',
      number: 42,
      title: 'Add widget',
      state: 'open',
      headSha: 'head-sha'
    })
  })

  it('resolves creation eligibility through the real git and gh probes', async () => {
    mockBackend()
    const api = createHostedReviewRealApi()
    await expect(
      api.getCreationEligibility({
        repoPath: '/repo',
        branch: 'feature',
        base: 'main',
        hasUncommittedChanges: false,
        hasUpstream: true,
        ahead: 0,
        behind: 0
      })
    ).resolves.toMatchObject({
      provider: 'github',
      review: null,
      reviewLookupOutcome: 'not_found',
      canCreate: true,
      blockedReason: null,
      nextAction: null,
      defaultBaseRef: 'main',
      head: 'feature'
    })
  })

  it('blocks creation eligibility when gh has no active account', async () => {
    mockBackend({ auth: NOT_AUTHENTICATED })
    const api = createHostedReviewRealApi()
    await expect(
      api.getCreationEligibility({
        repoPath: '/repo',
        branch: 'feature',
        hasUncommittedChanges: false,
        hasUpstream: true,
        ahead: 0,
        behind: 0
      })
    ).resolves.toMatchObject({
      provider: 'github',
      canCreate: false,
      blockedReason: 'auth_required',
      nextAction: 'authenticate'
    })
  })

  it('creates through the real gh pr create argv and stdin', async () => {
    mockBackend()
    const api = createHostedReviewRealApi()
    await expect(
      api.create({
        provider: 'github',
        repoPath: '/repo',
        base: 'main',
        title: 'Add widget',
        head: 'feature'
      })
    ).resolves.toEqual({ ok: true, number: 7, url: PR_URL_7 })

    expect(invokeMock).toHaveBeenCalledWith('git_status', {
      args: { worktreePath: '/repo' }
    })
    expect(invokeMock).toHaveBeenCalledWith('git_upstream_status', {
      args: { worktreePath: '/repo' }
    })
    const createCall = invokeMock.mock.calls.find(
      ([command, payload]) =>
        command === 'gh_exec' && ghExecArgs(payload)[0] === 'pr' && ghExecArgs(payload)[1] === 'create'
    )
    expect(ghExecArgs(createCall?.[1])).toEqual([
      'pr',
      'create',
      '--repo',
      'acme/widgets',
      '--base',
      'main',
      '--title',
      'Add widget',
      '--body-file',
      '-',
      '--head',
      'feature'
    ])
    expect((createCall?.[1] as { args: { stdin?: string } }).args.stdin).toBe('')
  })

  it('keeps createStacked rejecting as unimplemented', async () => {
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => {})
    const api = createHostedReviewRealApi()
    await expect(
      api.createStacked({ provider: 'github', repoPath: '/repo', base: 'main', title: 'Add widget' })
    ).rejects.toMatchObject({ name: 'UnimplementedBridgeError' })
    warn.mockRestore()
  })
})
