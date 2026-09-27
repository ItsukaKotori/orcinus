import { describe, expect, it, vi } from 'vitest'
import { UnimplementedBridgeError } from '../unimplemented-fallback'
import { createHostedReviewApi } from './hosted-review-api'

describe('Phase 0 hostedReview mock', () => {
  it('answers the per-worktree branch lookup with null instead of rejecting', async () => {
    await expect(
      createHostedReviewApi().forBranch({ repoPath: '/repo', branch: 'main' })
    ).resolves.toBeNull()
  })

  it('keeps unimplemented methods loudly rejecting', async () => {
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => {})
    await expect(
      createHostedReviewApi().create({ repoPath: '/repo', provider: 'github', base: 'main', title: 't' })
    ).rejects.toBeInstanceOf(UnimplementedBridgeError)
    expect(warn).toHaveBeenCalledWith(expect.stringContaining('hostedReview.create'))
    warn.mockRestore()
  })
})
