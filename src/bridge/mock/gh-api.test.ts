import { describe, expect, it } from 'vitest'
import type { GitHubPRRefreshCandidate } from '../../shared/github/pull-request-refresh-types'
import { createGhApi } from './gh-api'

const candidate: GitHubPRRefreshCandidate = {
  cacheKey: 'r1::/repo::main',
  repoId: 'r1',
  repoPath: '/repo',
  branch: 'main',
  repoKind: 'git'
}

describe('Phase 0 gh mock', () => {
  it('answers enqueuePRRefresh benignly so renderer refresh triggers stay best-effort', async () => {
    await expect(
      createGhApi().enqueuePRRefresh({ candidate, reason: 'visible', priority: 1 })
    ).resolves.toBe(false)
  })
})
