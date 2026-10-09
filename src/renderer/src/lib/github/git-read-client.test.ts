import { invoke } from '@tauri-apps/api/core'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { createRunGit, defaultGitReadExecutor, GitReadError } from './git-read-client'

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }))

const invokeMock = vi.mocked(invoke)

beforeEach(() => {
  invokeMock.mockReset()
})

describe('git read client', () => {
  it('returns stdout on zero exit and throws GitReadError otherwise', async () => {
    const executor = vi.fn(async (args: string[]) =>
      args[0] === 'rev-parse'
        ? { stdout: 'feature\n', stderr: '', code: 0 }
        : { stdout: '', stderr: 'fatal: bad', code: 128 }
    )
    const runGit = createRunGit(executor)
    await expect(runGit(['rev-parse', '--abbrev-ref', 'HEAD'])).resolves.toEqual({
      stdout: 'feature\n'
    })
    await expect(runGit(['show-ref', 'x'])).rejects.toMatchObject({
      name: 'GitReadError',
      code: 128,
      stderr: 'fatal: bad'
    })
  })

  it('carries the stderr text as the error message', async () => {
    const runGit = createRunGit(async () => ({ stdout: '', stderr: 'fatal: bad\n', code: 128 }))
    await expect(runGit(['show-ref', 'x'])).rejects.toThrow('fatal: bad')
  })

  it('falls back to a code-based message when stderr is empty', async () => {
    const runGit = createRunGit(async () => ({ stdout: '', stderr: '', code: 1 }))
    const pending = runGit(['show-ref', 'x'])
    await expect(pending).rejects.toBeInstanceOf(GitReadError)
    await expect(pending).rejects.toThrow('git read exited with code 1')
  })

  it('sends the worktree-scoped git_read envelope and returns the raw result', async () => {
    invokeMock.mockResolvedValueOnce({ stdout: 'main\n', stderr: '', code: 0 })
    const executor = defaultGitReadExecutor('/repo')
    await expect(executor(['rev-parse', '--abbrev-ref', 'HEAD'])).resolves.toEqual({
      stdout: 'main\n',
      stderr: '',
      code: 0
    })
    expect(invokeMock).toHaveBeenCalledWith('git_read', {
      args: { worktreePath: '/repo', args: ['rev-parse', '--abbrev-ref', 'HEAD'] }
    })
  })
})
