import { invoke } from '@tauri-apps/api/core'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { createHostedReviewRealApi } from './hosted-review'

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }))

const invokeMock = vi.mocked(invoke)

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

  it('keeps create rejecting as unimplemented', async () => {
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => {})
    const api = createHostedReviewRealApi()
    await expect(
      api.create({ provider: 'github', repoPath: '/repo', base: 'main', title: 'Add widget' })
    ).rejects.toMatchObject({ name: 'UnimplementedBridgeError' })
    warn.mockRestore()
  })
})
