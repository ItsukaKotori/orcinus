import { describe, expect, it } from 'vitest'
import { getTaskPageRepoCacheInput } from './task-page-source-context'
import type { Repo } from '../../../shared/repo-types'

describe('getTaskPageRepoCacheInput', () => {
  it('copies repo identity fields used by the GitHub work-item cache', () => {
    const repo = {
      id: 'repo-1',
      path: '/tmp/orca',
      executionHostId: 'local'
    } as Repo
    const input = getTaskPageRepoCacheInput(repo)
    expect(input.id).toBe('repo-1')
    expect(input.path).toBe('/tmp/orca')
    expect(input.executionHostId).toBe('local')
  })
})
