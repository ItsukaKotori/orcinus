import { beforeEach, describe, expect, it, vi } from 'vitest'
import type { GitBranchCreateOutcome } from '../../../shared/git-branch-ops-types'
import { createRuntimeGitBranch } from './runtime-git-branch-client'

const branchCreate = vi.fn()

beforeEach(() => {
  branchCreate.mockReset()
  ;(globalThis as unknown as { window: unknown }).window = { api: { git: { branchCreate } } }
})

const context = {
  settings: { activeRuntimeEnvironmentId: null },
  worktreeId: 'r::/a',
  worktreePath: '/a',
}

describe('createRuntimeGitBranch', () => {
  it('把 worktreePath 与 args 原样下发（args 形状对齐 GitBranchCreateArgs）', async () => {
    branchCreate.mockResolvedValue({ status: 'created', branch: 'new-branch' } satisfies GitBranchCreateOutcome)
    const outcome = await createRuntimeGitBranch(context, { name: 'new-branch', startPoint: 'deadbeef' })
    expect(branchCreate).toHaveBeenCalledWith({
      worktreePath: '/a',
      args: { name: 'new-branch', startPoint: 'deadbeef' },
    })
    expect(outcome.status).toBe('created')
  })

  it('业务拒绝 NameTaken **不**被改写成异常（前端要按 status 分支）', async () => {
    const refused = { status: { refused: 'name-taken' }, branch: 'feature' } satisfies GitBranchCreateOutcome
    branchCreate.mockResolvedValue(refused)
    const outcome = await createRuntimeGitBranch(context, { name: 'feature', startPoint: 'x' })
    expect(outcome).toEqual(refused)
  })

  it('环境级失败原样上抛，不吞', async () => {
    branchCreate.mockRejectedValue(new Error('unknown start point'))
    await expect(createRuntimeGitBranch(context, { name: 'n', startPoint: 'nope' })).rejects.toThrow(
      /unknown start point/
    )
  })

  it('空 worktreePath 时本地就拒，不把空路径下发给鉴权层', async () => {
    await expect(
      createRuntimeGitBranch(
        { settings: null, worktreeId: null, worktreePath: '' } as never,
        { name: 'x', startPoint: 'y' }
      )
    ).rejects.toThrow(/missing worktree path/)
    expect(branchCreate).not.toHaveBeenCalled()
  })

  it('远程 runtime 目标被显式拒绝，不命中 window.api.git.branchCreate', async () => {
    branchCreate.mockResolvedValue({ status: 'created' })
    await expect(
      createRuntimeGitBranch(
        { settings: { activeRuntimeEnvironmentId: 'env-1' }, worktreeId: 'r::/a', worktreePath: '/a' },
        { name: 'x', startPoint: 'y' }
      )
    ).rejects.toThrow(/remote runtime targets are not supported/)
    expect(branchCreate).not.toHaveBeenCalled()
  })
})
