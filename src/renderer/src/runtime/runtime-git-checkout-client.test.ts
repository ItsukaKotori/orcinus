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

  it('不开 callRuntimeRpc 分支：远程 runtime 面在本仓库没有 real 实现（spec §3.6）', async () => {
    // 若有人补了 RPC 分支，这个只带 api.git.checkout 的最小 window 桩会让它炸出来。
    checkout.mockResolvedValue({ status: 'already-on-branch', branch: 'main' })
    await expect(
      checkoutRuntimeGit(context, { kind: 'remote', name: 'origin/main' })
    ).resolves.toMatchObject({ status: 'already-on-branch' })
    expect(checkout).toHaveBeenCalledTimes(1)
  })
})
