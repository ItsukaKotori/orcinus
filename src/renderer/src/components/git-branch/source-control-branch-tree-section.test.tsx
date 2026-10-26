// @vitest-environment happy-dom
import { act } from 'react'
import { createRoot, type Root } from 'react-dom/client'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { GitRefEntry } from '../../../../shared/git-ref-types'
import type { Worktree } from '../../../../shared/worktree/types'
import { BRANCH_SECTION_VIEW_STATE_STORAGE_KEY } from './source-control-branch-section-view-state'

// 与 source-control-branch-switcher.test.tsx 的关键差异：常驻区块无 Popover，默认展开即在 DOM；
// 成功路径优先用注入的 refreshActiveGitStatusAfterMutation（断它，而非模块 refresh）；
// 且 invalidate 现在真起作用（断 refs 二次取数）。
vi.mock('sonner', () => ({ toast: { warning: vi.fn(), error: vi.fn(), success: vi.fn() } }))
vi.mock('../right-sidebar/git-status-refresh', () => ({
  refreshGitStatusForWorktree: vi.fn().mockResolvedValue(undefined),
}))

import { toast } from 'sonner'
import { SourceControlBranchTreeSection } from './source-control-branch-tree-section'

;(globalThis as unknown as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true

// 生产里 `Worktree.branch` 是完整 ref；fixture 写短名会让「忘剥前缀」这个 Critical 测不出来。
const worktrees: Worktree[] = [
  { id: 'r::/a', repoId: 'r', branch: 'refs/heads/main', displayName: 'A', path: '/a' },
  { id: 'r::/b', repoId: 'r', branch: 'refs/heads/dev', displayName: 'B', path: '/b', hostId: 'host-2' },
  { id: 'r::/c', repoId: 'r', branch: 'refs/heads/standby', displayName: 'C', path: '/c' },
  { id: 'other::/x', repoId: 'other', branch: 'refs/heads/main', displayName: 'OTHER', path: '/x' },
] as unknown as Worktree[]

const state = {
  worktreesByRepo: {
    other: worktrees.filter((w) => w.repoId === 'other'),
    r: worktrees.filter((w) => w.repoId === 'r'),
  },
  openModal: vi.fn(),
  gitStatusByWorktree: {} as Record<string, { path: string }[]>,
  tabsByWorktree: {} as Record<string, unknown[]>,
  unifiedTabsByWorktree: {} as Record<string, { contentType: string }[]>,
  setGitStatus: vi.fn(),
  updateWorktreeGitIdentity: vi.fn(),
  setUpstreamStatus: vi.fn(),
  fetchUpstreamStatus: vi.fn(),
}

const activate = vi.fn(
  (_id: string, _opts?: { executionHostId?: string }): { primaryTabId: string | null } | false => ({
    primaryTabId: null,
  })
)

vi.mock('@/store', () => {
  const useAppStore = Object.assign(
    (selector: (s: typeof state) => unknown) => selector(state),
    { getState: () => state }
  )
  return { useAppStore }
})

vi.mock('@/lib/worktree-activation', () => ({
  activateAndRevealWorktree: (
    id: string,
    opts?: { executionHostId?: string }
  ): { primaryTabId: string | null } | false => activate(id, opts),
}))

const refs = vi.fn()
const checkout = vi.fn().mockResolvedValue({ status: 'switched', branch: 'x' })

function entry(over: Partial<GitRefEntry> & { name: string }): GitRefEntry {
  return {
    fullName: `refs/heads/${over.name}`,
    category: 'local',
    oid: '0'.repeat(40),
    isHead: false,
    ...over,
  }
}

let host: HTMLDivElement
let root: Root

beforeEach(() => {
  host = document.createElement('div')
  document.body.append(host)
  root = createRoot(host)
  window.localStorage.clear()
  state.openModal.mockReset()
  state.gitStatusByWorktree = {}
  state.tabsByWorktree = {}
  state.unifiedTabsByWorktree = {}
  activate.mockReset().mockReturnValue({ primaryTabId: null })
  checkout.mockReset().mockResolvedValue({ status: 'switched', branch: 'x' })
  vi.mocked(toast.warning).mockReset()
  vi.mocked(toast.error).mockReset()
  refs.mockReset().mockResolvedValue({
    refs: [
      entry({ name: 'main', isHead: true }),
      entry({ name: 'dev' }),
      entry({ name: 'standby' }),
      entry({ name: 'fresh' }),
      entry({ name: 'ghost' }),
      entry({ name: 'origin/main', category: 'remote', remote: 'origin', fullName: 'refs/remotes/origin/main' }),
      entry({ name: 'origin/dev', category: 'remote', remote: 'origin', fullName: 'refs/remotes/origin/dev' }),
    ],
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
const settle = async (n = 8): Promise<void> => {
  for (let i = 0; i < n; i += 1) await flush()
}
const rowInTree = (testId: string): HTMLElement => {
  const el = document.querySelector(`[data-testid="${testId}"]`)
  if (!el) throw new Error(`expected tree row ${testId}`)
  return el as HTMLElement
}

async function mountSection(
  worktreeId = 'r::/a',
  extra: Partial<{ refreshActiveGitStatusAfterMutation: () => Promise<void>; settings: unknown }> = {}
): Promise<void> {
  await act(async () => {
    root.render(
      <SourceControlBranchTreeSection
        worktreeId={worktreeId}
        refreshActiveGitStatusAfterMutation={
          extra.refreshActiveGitStatusAfterMutation ?? (async () => {})
        }
        settings={extra.settings ?? null}
      />
    )
  })
  await settle()
  rowInTree('branch-tree')
}

describe('SourceControlBranchTreeSection', () => {
  it('默认展开：挂载即取数并渲染树', async () => {
    await mountSection()
    expect(refs).toHaveBeenCalledWith({ worktreePath: '/a' })
    expect(document.querySelector('[data-testid="branch-tree"]')).not.toBeNull()
  })

  it('折叠态持久化为 true 时挂载即折叠：不渲染树', async () => {
    window.localStorage.setItem(BRANCH_SECTION_VIEW_STATE_STORAGE_KEY, JSON.stringify({ collapsed: true }))
    await act(async () => {
      root.render(
        <SourceControlBranchTreeSection worktreeId="r::/a" refreshActiveGitStatusAfterMutation={async () => {}} settings={null} />
      )
    })
    await settle()
    expect(document.querySelector('[data-testid="branch-tree"]')).toBeNull()
    expect(refs).not.toHaveBeenCalled()
  })

  it('点折叠头：写 localStorage collapsed=true', async () => {
    await mountSection()
    await click(document.querySelector('[data-testid="branch-section-toggle"]'))
    await settle()
    expect(window.localStorage.getItem(BRANCH_SECTION_VIEW_STATE_STORAGE_KEY)).toContain('true')
  })

  it('点无 owner 本地分支（干净无会话）→ 不弹框，直接 checkout（local target）', async () => {
    await mountSection()
    await click(rowInTree('branch-row-ref:refs/heads/fresh'))
    await settle()
    expect(checkout).toHaveBeenCalledWith({ worktreePath: '/a', target: { kind: 'local', name: 'fresh' } })
    expect(document.body.textContent).not.toContain('Checkout branch?')
    expect(activate).not.toHaveBeenCalled()
  })

  it('检出成功 → 调注入的 refreshActiveGitStatusAfterMutation（走 foundation 路径）', async () => {
    const refreshSpy = vi.fn(async () => {})
    await mountSection('r::/a', { refreshActiveGitStatusAfterMutation: refreshSpy })
    await click(rowInTree('branch-row-ref:refs/heads/fresh'))
    await settle()
    expect(refreshSpy).toHaveBeenCalledTimes(1)
  })

  it('检出成功 → 常驻树 invalidate 重新取数（refs 二次请求，无需重开）', async () => {
    await mountSection()
    const before = refs.mock.calls.length
    await click(rowInTree('branch-row-ref:refs/heads/fresh'))
    await settle()
    expect(refs.mock.calls.length).toBeGreaterThan(before)
  })

  it('点远程分支 → remote target', async () => {
    await mountSection()
    await click(rowInTree('branch-row-ref:refs/remotes/origin/main'))
    await settle()
    expect(checkout).toHaveBeenCalledWith({ worktreePath: '/a', target: { kind: 'remote', name: 'origin/main' } })
  })

  it('有脏改动 → 先弹确认框，未确认不 checkout；确认后 checkout', async () => {
    state.gitStatusByWorktree['r::/a'] = [{ path: 'a.ts' }]
    await mountSection()
    await click(rowInTree('branch-row-ref:refs/heads/fresh'))
    await settle()
    expect(document.body.textContent).toContain('Checkout branch?')
    expect(checkout).not.toHaveBeenCalled()
    const confirm = [...document.querySelectorAll('button')].find((b) => b.textContent?.trim() === 'Checkout')
    await click(confirm ?? null)
    await settle()
    expect(checkout).toHaveBeenCalledWith({ worktreePath: '/a', target: { kind: 'local', name: 'fresh' } })
  })

  it('终端 + agent 会话合并计数（2终端+1agent+1editor→3），不计 editor/不双计', async () => {
    state.tabsByWorktree['r::/a'] = [{ id: 't1' }, { id: 't2' }]
    state.unifiedTabsByWorktree['r::/a'] = [
      { contentType: 'terminal' },
      { contentType: 'terminal' },
      { contentType: 'agent-session' },
      { contentType: 'editor' },
    ]
    await mountSection()
    await click(rowInTree('branch-row-ref:refs/heads/fresh'))
    await settle()
    expect(document.body.textContent).toMatch(/3 running terminal\(s\)\/session\(s\)/)
    expect(checkout).not.toHaveBeenCalled()
  })

  it('F1：点被别的工作区检出的分支 → switch 跳转，不 checkout', async () => {
    await mountSection()
    await click(rowInTree('branch-row-ref:refs/heads/dev'))
    await settle()
    expect(activate).toHaveBeenCalledWith('r::/b', { executionHostId: 'host-2' })
    expect(checkout).not.toHaveBeenCalled()
  })

  it('点当前 HEAD 分支 → 无动作', async () => {
    await mountSection()
    await click(rowInTree('branch-row-ref:refs/heads/main'))
    await settle()
    expect(checkout).not.toHaveBeenCalled()
    expect(activate).not.toHaveBeenCalled()
    expect(state.openModal).not.toHaveBeenCalled()
  })

  it('checked-out-elsewhere 且 ade 已知占用方（远程解析到被占本地）→ 跳转，不弹外部提示', async () => {
    checkout.mockResolvedValueOnce({ status: { refused: 'checked-out-elsewhere' }, branch: 'dev', worktreePath: '/b' })
    await mountSection()
    await click(rowInTree('branch-row-ref:refs/remotes/origin/dev'))
    await settle()
    expect(activate).toHaveBeenCalledWith('r::/b', { executionHostId: 'host-2' })
    expect(toast.warning).not.toHaveBeenCalled()
  })

  it('checked-out-elsewhere 且 ade 未知占用方 → 用 worktreePath 弹提示，不静默', async () => {
    checkout.mockResolvedValueOnce({ status: { refused: 'checked-out-elsewhere' }, branch: 'ghost', worktreePath: '/ext/ghost' })
    await mountSection()
    await click(rowInTree('branch-row-ref:refs/heads/ghost'))
    await settle()
    expect(toast.warning).toHaveBeenCalledWith(expect.stringContaining('/ext/ghost'))
    expect(activate).not.toHaveBeenCalled()
  })

  it('invalid-target → toast.error，且不刷状态', async () => {
    const refreshSpy = vi.fn(async () => {})
    checkout.mockResolvedValueOnce({ status: 'invalid-target' })
    await mountSection('r::/a', { refreshActiveGitStatusAfterMutation: refreshSpy })
    await click(rowInTree('branch-row-ref:refs/heads/fresh'))
    await settle()
    expect(toast.error).toHaveBeenCalled()
    expect(refreshSpy).not.toHaveBeenCalled()
  })

  it('环境级失败 → toast.error，且不刷状态', async () => {
    const refreshSpy = vi.fn(async () => {})
    checkout.mockRejectedValueOnce(new Error('would be overwritten by checkout'))
    await mountSection('r::/a', { refreshActiveGitStatusAfterMutation: refreshSpy })
    await click(rowInTree('branch-row-ref:refs/heads/fresh'))
    await settle()
    expect(toast.error).toHaveBeenCalledWith('would be overwritten by checkout')
    expect(refreshSpy).not.toHaveBeenCalled()
  })

  it('本宿主不注入 All Branches 行', async () => {
    await mountSection()
    expect(document.querySelector('[data-testid="branch-row-row:all-branches"]')).toBeNull()
  })

  it('取数失败 → 区块渲染一行失败文案，不出树也不抛', async () => {
    refs.mockReset().mockRejectedValue(new Error('boom'))
    await act(async () => {
      root.render(<SourceControlBranchTreeSection worktreeId="r::/a" settings={null} />)
    })
    await settle()
    expect(document.querySelector('[data-testid="branch-tree"]')).toBeNull()
    expect(document.body.textContent).toContain('Failed to read branches.')
  })

  it('同宿主被检出分支：hostId 原样透传 undefined（不兜成某个宿主）', async () => {
    await mountSection()
    await click(rowInTree('branch-row-ref:refs/heads/standby'))
    await settle()
    expect(activate).toHaveBeenCalledWith('r::/c', { executionHostId: undefined })
    expect(checkout).not.toHaveBeenCalled()
  })

  it('worktreeId 解析不到 → 区块渲染 null，不取数', async () => {
    await act(async () => {
      root.render(<SourceControlBranchTreeSection worktreeId="nope" settings={null} />)
    })
    await settle()
    expect(refs).not.toHaveBeenCalled()
    expect(document.querySelector('[data-testid="source-control-branch-tree-section"]')).toBeNull()
  })
})
