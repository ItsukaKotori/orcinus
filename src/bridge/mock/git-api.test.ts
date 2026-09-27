import { describe, expect, it, vi } from 'vitest'
import { UnimplementedBridgeError } from '../unimplemented-fallback'
import { createGitApi } from './git-api'

describe('Phase 0 git mock (inspection subset)', () => {
  it('answers the SCM status poll with an empty clean status', async () => {
    await expect(createGitApi().status({ worktreePath: '/repo' })).resolves.toEqual({
      entries: [],
      conflictOperation: 'unknown'
    })
  })

  it('accepts the cancellation and upstream-ref watch registrations as no-ops', async () => {
    const git = createGitApi()
    await expect(git.cancelStatus({ requestToken: 'token-1' })).resolves.toBeUndefined()
    await expect(
      git.setStatusUpstreamRefWatch({
        worktreeId: 'repo::/repo',
        worktreePath: '/repo',
        executionHostId: 'local'
      })
    ).resolves.toBeUndefined()
  })

  it('keeps the unported git surface loudly rejecting', async () => {
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => {})
    await expect(
      createGitApi().checkIgnored({ worktreePath: '/repo', paths: ['dist/'] })
    ).rejects.toBeInstanceOf(UnimplementedBridgeError)
    expect(warn).toHaveBeenCalledWith(expect.stringContaining('git.checkIgnored'))
    warn.mockRestore()
  })
})
