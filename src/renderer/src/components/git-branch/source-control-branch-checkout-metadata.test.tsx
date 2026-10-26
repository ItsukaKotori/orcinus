// @vitest-environment happy-dom
import { act } from 'react'
import { createRoot, type Root } from 'react-dom/client'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { GitRefEntry } from '../../../../shared/git-ref-types'
import type { Worktree } from '../../../../shared/worktree/types'
import { SourceControlBranchSwitcher } from './source-control-branch-switcher'

;(globalThis as unknown as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true

// 与 source-control-branch-switcher.test.tsx 的关键差异：**不** mock git-status-refresh。
// 本文件要让真 refreshGitStatusForWorktree 跑通到 store，验证「checkout → 跟刷
// Worktree.branch 成完整 ref」这条 spec §13.4 前提有人守（否则不显式刷新最长 60s 才追上）。
// 只截最底层取数：getRuntimeGitStatus 返回检出后的新状态。
vi.mock('@/runtime/runtime-git-client', () => ({
  getRuntimeGitStatus: vi.fn().mockResolvedValue({
    entries: [],
    conflictOperation: 'unknown',
    head: 'fresh-oid',
    branch: 'refs/heads/fresh',
  }),
  getRuntimeGitUpstreamStatus: vi.fn().mockResolvedValue({ hasUpstream: false, ahead: 0, behind: 0 }),
}))
vi.mock('sonner', () => ({ toast: { warning: vi.fn(), error: vi.fn(), success: vi.fn() } }))

const worktrees = [
  { id: 'r::/a', repoId: 'r', branch: 'refs/heads/main', displayName: 'A', path: '/a' },
] as unknown as Worktree[]

const state = {
  worktreesByRepo: { r: worktrees },
  openModal: vi.fn(),
  gitStatusByWorktree: {} as Record<string, { path: string }[]>,
  tabsByWorktree: {} as Record<string, unknown[]>,
  unifiedTabsByWorktree: {} as Record<string, { contentType: string }[]>,
  setGitStatus: vi.fn(),
  updateWorktreeGitIdentity: vi.fn(),
  setUpstreamStatus: vi.fn(),
  fetchUpstreamStatus: vi.fn(),
}

vi.mock('@/store', () => {
  const useAppStore = Object.assign(
    (selector: (s: typeof state) => unknown) => selector(state),
    { getState: () => state }
  )
  return { useAppStore }
})

vi.mock('@/lib/worktree-activation', () => ({
  activateAndRevealWorktree: vi.fn(() => ({ primaryTabId: null })),
}))

const refs = vi.fn()
const checkout = vi.fn().mockResolvedValue({ status: 'switched', branch: 'fresh' })

let host: HTMLDivElement
let root: Root

beforeEach(() => {
  host = document.createElement('div')
  document.body.append(host)
  root = createRoot(host)
  state.gitStatusByWorktree = {}
  state.tabsByWorktree = {}
  state.unifiedTabsByWorktree = {}
  state.setGitStatus.mockClear()
  state.updateWorktreeGitIdentity.mockClear()
  checkout.mockClear().mockResolvedValue({ status: 'switched', branch: 'fresh' })
  refs.mockReset().mockResolvedValue({
    refs: [
      { name: 'main', fullName: 'refs/heads/main', category: 'local', oid: '0'.repeat(40), isHead: true },
      { name: 'fresh', fullName: 'refs/heads/fresh', category: 'local', oid: '1'.repeat(40), isHead: false },
    ] satisfies GitRefEntry[],
    truncated: false,
  })
  ;(window as unknown as { api: unknown }).api = { git: { refs, checkout } }
})
afterEach(() => {
  act(() => root.unmount())
  host.remove()
  delete (window as unknown as { api?: unknown }).api
})

const flush = async (): Promise<void> => {
  await act(async () => {
    await Promise.resolve()
  })
}
const click = async (el: Element | null): Promise<void> => {
  await act(async () => {
    el?.dispatchEvent(new MouseEvent('click', { bubbles: true }))
  })
}
const rowInPopover = (testId: string): HTMLElement => {
  const el = document.querySelector(`[data-testid="${testId}"]`)
  if (!el) throw new Error(`expected popover row ${testId}`)
  return el as HTMLElement
}

async function openSwitcher(): Promise<void> {
  await act(async () => {
    root.render(
      <SourceControlBranchSwitcher worktreeId="r::/a">
        <button type="button">main</button>
      </SourceControlBranchSwitcher>
    )
  })
  await click(host.querySelector('button'))
  for (let i = 0; i < 8; i += 1) await flush()
  rowInPopover('branch-tree')
}

describe('checkout 元数据跟随（spec §13.4 守卫）', () => {
  it('检出成功后真跑刷状态，把 Worktree.branch 跟刷成新分支的完整 ref', async () => {
    await openSwitcher()
    // fresh 无 owner、无脏、无会话 → 直接检出（不弹框），executeCheckout 成功后调真 refresh。
    await click(rowInPopover('branch-row-ref:refs/heads/fresh'))
    for (let i = 0; i < 12; i += 1) await flush()

    // 真 refreshGitStatusForWorktree 内部经 deps 调 store action：断跟刷成**完整 ref**。
    expect(state.updateWorktreeGitIdentity).toHaveBeenCalledWith('r::/a', {
      head: 'fresh-oid',
      branch: 'refs/heads/fresh',
    })
  })
})
