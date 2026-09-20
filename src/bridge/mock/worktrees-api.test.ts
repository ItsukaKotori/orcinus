import { describe, expect, it } from 'vitest'
import { createWorktreesApi } from './worktrees-api'

describe('Phase 0 worktrees mock', () => {
  it('does not leak module state through the worktree lists it returns', async () => {
    const worktrees = createWorktreesApi()

    const list = await worktrees.list({ repoId: 'mock-repo-1' })
    list[0].displayName = 'polluted'
    list[0].isPinned = true
    const reread = await worktrees.list({ repoId: 'mock-repo-1' })
    expect(reread[0]).toMatchObject({ displayName: 'calm-otter', isPinned: false })

    const detected = await worktrees.listDetected({ repoId: 'repo-1' })
    detected.worktrees[0].branch = 'polluted'
    const redetected = await worktrees.listDetected({ repoId: 'repo-1' })
    expect(redetected.worktrees[0]).toMatchObject({ branch: 'feature/plugin-center' })
  })
})
