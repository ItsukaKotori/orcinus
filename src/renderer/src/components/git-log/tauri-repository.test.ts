// @vitest-environment happy-dom
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { TauriGitLogRepository } from './tauri-repository'

const git = {
  logQuery: vi.fn(),
  refs: vi.fn(),
  commitCompare: vi.fn(),
}

describe('TauriGitLogRepository', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    ;(window as unknown as { api: { git: unknown } }).api = { git }
  })

  it('loadSnapshot calls git.logQuery with worktreePath + query', async () => {
    git.logQuery.mockResolvedValue({ items: [], hasMore: false })
    const repo = new TauriGitLogRepository('/repo')
    await repo.loadSnapshot({ refs: 'all', author: 'Lin' }, 100)
    expect(git.logQuery).toHaveBeenCalledWith({
      worktreePath: '/repo',
      query: { skip: 100, limit: 100, refs: 'all', author: 'Lin' },
    })
  })

  it('loadRefs calls git.refs', async () => {
    git.refs.mockResolvedValue({ refs: [], truncated: false })
    const repo = new TauriGitLogRepository('/repo')
    expect(await repo.loadRefs()).toEqual({ refs: [], truncated: false })
    expect(git.refs).toHaveBeenCalledWith({ worktreePath: '/repo' })
  })

  it('loadCommitFiles maps commitCompare entries', async () => {
    git.commitCompare.mockResolvedValue({ entries: [{ path: 'a.ts', status: 'modified' }] })
    const repo = new TauriGitLogRepository('/repo')
    const files = await repo.loadCommitFiles('abc1234')
    expect(files[0]?.path).toBe('a.ts')
    expect(git.commitCompare).toHaveBeenCalledWith({ worktreePath: '/repo', commitId: 'abc1234' })
  })
})
