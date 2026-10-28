import { beforeEach, describe, expect, it, vi } from 'vitest'
import type {
  GitBranchCreateOutcome,
  GitBranchSetUpstreamOutcome,
  GitBranchRenameOutcome,
  GitBranchDeleteOutcome
} from '../../../shared/git-branch-ops-types'
import {
  createRuntimeGitBranch,
  setRuntimeGitBranchUpstream,
  renameRuntimeGitBranch,
  deleteRuntimeGitBranch
} from './runtime-git-branch-client'

const branchCreate = vi.fn()
const branchSetUpstream = vi.fn()
const branchRename = vi.fn()
const branchDelete = vi.fn()

beforeEach(() => {
  branchCreate.mockReset()
  branchSetUpstream.mockReset()
  branchRename.mockReset()
  branchDelete.mockReset()
  ;(globalThis as unknown as { window: unknown }).window = { api: { git: { branchCreate, branchSetUpstream, branchRename, branchDelete } } }
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

describe('setRuntimeGitBranchUpstream', () => {
  it('把 worktreePath 与 args 原样下发', async () => {
    branchSetUpstream.mockResolvedValue({ status: 'set' } satisfies GitBranchSetUpstreamOutcome)
    await setRuntimeGitBranchUpstream(context, { branch: 'feature', upstream: 'origin/feature' })
    expect(branchSetUpstream).toHaveBeenCalledWith({
      worktreePath: '/a',
      args: { branch: 'feature', upstream: 'origin/feature' },
    })
  })

  it('业务拒绝 UpstreamMissing **不**被改写成异常', async () => {
    const refused = {
      status: { refused: 'upstream-missing' },
      branch: 'feature',
      upstream: 'origin/ghost',
    } satisfies GitBranchSetUpstreamOutcome
    branchSetUpstream.mockResolvedValue(refused)
    const outcome = await setRuntimeGitBranchUpstream(context, { branch: 'feature', upstream: 'origin/ghost' })
    expect(outcome).toEqual(refused)
  })

  it('环境级失败原样上抛', async () => {
    branchSetUpstream.mockRejectedValue(new Error('unknown local branch'))
    await expect(
      setRuntimeGitBranchUpstream(context, { branch: 'nope', upstream: 'origin/feature' })
    ).rejects.toThrow(/unknown local branch/)
  })

  it('远程 runtime 目标被显式拒绝，不命中 window.api', async () => {
    await expect(
      setRuntimeGitBranchUpstream(
        { settings: { activeRuntimeEnvironmentId: 'env-1' }, worktreeId: 'r::/a', worktreePath: '/a' },
        { branch: 'feature', upstream: 'origin/feature' }
      )
    ).rejects.toThrow(/remote runtime targets are not supported/)
    expect(branchSetUpstream).not.toHaveBeenCalled()
  })
})

describe('renameRuntimeGitBranch / deleteRuntimeGitBranch', () => {
  it('rename 原样下发 + NameTaken 不转异常', async () => {
    branchRename.mockResolvedValue({ status: 'renamed' } satisfies GitBranchRenameOutcome)
    await renameRuntimeGitBranch(context, { oldName: 'feat', newName: 'feat2' })
    expect(branchRename).toHaveBeenCalledWith({ worktreePath: '/a', args: { oldName: 'feat', newName: 'feat2' } })
    branchRename.mockResolvedValue({ status: { refused: 'name-taken' }, newName: 'other' })
    const outcome = await renameRuntimeGitBranch(context, { oldName: 'feat', newName: 'other' })
    expect(outcome.status).toEqual({ refused: 'name-taken' })
  })

  it('delete 原样下发 + NotMergeable 不转异常（供二次确认）', async () => {
    branchDelete.mockResolvedValue({ status: 'deleted' } satisfies GitBranchDeleteOutcome)
    await deleteRuntimeGitBranch(context, { branch: 'feat', force: false })
    expect(branchDelete).toHaveBeenCalledWith({ worktreePath: '/a', args: { branch: 'feat', force: false } })
    branchDelete.mockResolvedValue({ status: { refused: { 'not-mergeable': { unmergedCount: 3 } } } })
    const outcome = await deleteRuntimeGitBranch(context, { branch: 'feat', force: false })
    expect(outcome.status).toEqual({ refused: { 'not-mergeable': { unmergedCount: 3 } } })
  })

  it('rename 空路径本地拒 + 远程目标拒', async () => {
    await expect(
      renameRuntimeGitBranch({ settings: null, worktreeId: null, worktreePath: '' } as never, { oldName: 'a', newName: 'b' })
    ).rejects.toThrow(/missing worktree path/)
    await expect(
      deleteRuntimeGitBranch(
        { settings: { activeRuntimeEnvironmentId: 'env-1' }, worktreeId: 'r::/a', worktreePath: '/a' },
        { branch: 'x', force: true }
      )
    ).rejects.toThrow(/remote runtime targets are not supported/)
    expect(branchDelete).not.toHaveBeenCalled()
  })
})
