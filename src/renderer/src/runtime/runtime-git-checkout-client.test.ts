import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type {
  GitCheckoutOutcome,
  GitCheckoutTarget
} from '../../../shared/git-checkout-types'
import { checkoutRuntimeGit } from './runtime-git-checkout-client'

const checkout = vi.fn()

beforeEach(() => {
  checkout.mockReset()
  vi.stubGlobal('window', { api: { git: { checkout } } })
})
afterEach(() => {
  vi.unstubAllGlobals()
})

const context = {
  settings: null,
  worktreeId: 'r::/a',
  worktreePath: '/a',
  connectionId: undefined,
} as never

const refused: GitCheckoutOutcome = {
  status: { refused: 'checked-out-elsewhere' },
  branch: 'feature',
  worktreePath: '/other',
}

describe('checkoutRuntimeGit', () => {
  it('把 worktreePath 与 target 原样下发', async () => {
    checkout.mockResolvedValue({ status: 'switched', branch: 'feature' } satisfies GitCheckoutOutcome)
    const outcome = await checkoutRuntimeGit(context, { kind: 'local', name: 'feature' })
    expect(checkout).toHaveBeenCalledWith({
      worktreePath: '/a',
      target: { kind: 'local', name: 'feature' },
    })
    expect(outcome.status).toBe('switched')
  })

  it('业务级拒绝**不**被改写成异常（前端要按 status 分支出不同文案与跳转）', async () => {
    checkout.mockResolvedValue(refused)
    const outcome = await checkoutRuntimeGit(context, { kind: 'local', name: 'feature' })
    expect(outcome).toEqual(refused)
  })

  it('环境级失败原样上抛，不吞也不转成软错误', async () => {
    checkout.mockRejectedValue(new Error('would be overwritten by checkout'))
    await expect(
      checkoutRuntimeGit(context, { kind: 'local', name: 'feature' })
    ).rejects.toThrow(/overwritten by checkout/)
  })

  it('空 worktreePath 时本地就拒，不把空路径下发给鉴权层', async () => {
    await expect(
      checkoutRuntimeGit(
        { settings: null, worktreeId: null, worktreePath: '' } as never,
        { kind: 'local', name: 'x' } satisfies GitCheckoutTarget
      )
    ).rejects.toThrow(/worktree path/)
    expect(checkout).not.toHaveBeenCalled()
  })

  // 非 local 目标必须**显式拒绝**，而不是悄悄拿本机路径跑写操作：
  // 用户选中远程 runtime 时，worktreePath 是远端机器的路径，
  // 只要同拼写在本机已授权就是一次错主机的写。
  it('远程 runtime 目标被显拒绝，不命中 window.api.git.checkout', async () => {
    checkout.mockResolvedValue({ status: 'switched', branch: 'main' })
    await expect(
      checkoutRuntimeGit(
        {
          settings: { activeRuntimeEnvironmentId: 'env-1' },
          worktreeId: 'r::/a',
          worktreePath: '/a',
        } as never,
        { kind: 'local', name: 'main' }
      )
    ).rejects.toThrow(/remote runtime targets are not supported/)
    expect(checkout).not.toHaveBeenCalled()
  })

  it('本地 runtime 目标正常下发（与上一条成对，否则“总拒绝”的实现也能过）', async () => {
    checkout.mockResolvedValue({ status: 'switched', branch: 'main' })
    await expect(
      checkoutRuntimeGit(
        {
          settings: { activeRuntimeEnvironmentId: null },
          worktreeId: 'r::/a',
          worktreePath: '/a',
        } as never,
        { kind: 'local', name: 'main' }
      )
    ).resolves.toMatchObject({ status: 'switched' })
    expect(checkout).toHaveBeenCalledTimes(1)
  })
})
