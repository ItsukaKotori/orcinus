// @vitest-environment happy-dom
import { act } from 'react'
import { createRoot, type Root } from 'react-dom/client'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { GitRefEntry } from '../../../../shared/git-ref-types'
import type { Worktree } from '../../../../shared/worktree/types'

// sonner / 刷状态链路隔开：Task 5 只断“成功后调了 refresh”，真跟刷是 Task 6 的职责。
vi.mock('sonner', () => ({ toast: { warning: vi.fn(), error: vi.fn(), success: vi.fn() } }))
vi.mock('../right-sidebar/git-status-refresh', () => ({
  refreshGitStatusForWorktree: vi.fn().mockResolvedValue(undefined),
}))

import { toast } from 'sonner'
import { refreshGitStatusForWorktree } from '../right-sidebar/git-status-refresh'
import { SourceControlBranchSwitcher } from './source-control-branch-switcher'

;(globalThis as unknown as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true

// 生产里 `Worktree.branch` 是**完整 ref**（`git worktree list --porcelain` 的
// `branch refs/heads/main` 被原样存下，`status.rs` 更是显式拼前缀）。
// fixture 写短名会让「忘剥前缀」这个 Critical 完全测不出来。
const worktrees: Worktree[] = [
  { id: 'r::/a', repoId: 'r', branch: 'refs/heads/main', displayName: 'A', path: '/a' },
  { id: 'r::/b', repoId: 'r', branch: 'refs/heads/dev', displayName: 'B', path: '/b', hostId: 'host-2' },
  // 同宿主（无 hostId）的另一工作区，检出 standby：验证 hostId 原样透传（undefined，不兜成某个宿主）。
  { id: 'r::/c', repoId: 'r', branch: 'refs/heads/standby', displayName: 'C', path: '/c' },
  // 另一个仓也有 `main`。owner 映射若跳 repo 汇总，它会把本仓 main 的归属顶掉。
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
  state.openModal.mockReset()
  state.gitStatusByWorktree = {}
  state.tabsByWorktree = {}
  state.unifiedTabsByWorktree = {}
  activate.mockReset().mockReturnValue({ primaryTabId: null })
  checkout.mockReset().mockResolvedValue({ status: 'switched', branch: 'x' })
  vi.mocked(toast.warning).mockReset()
  vi.mocked(toast.error).mockReset()
  vi.mocked(refreshGitStatusForWorktree).mockClear()
  refs.mockReset().mockResolvedValue({
    refs: [
      entry({ name: 'main', isHead: true }),
      entry({ name: 'dev' }),
      entry({ name: 'standby' }),
      entry({ name: 'fresh' }),
      entry({ name: 'ghost' }),
      entry({
        name: 'origin/main',
        category: 'remote',
        remote: 'origin',
        fullName: 'refs/remotes/origin/main',
      }),
      // 远程行，其解析后的本地名 dev 被 r::/b 占用（用于测 executeCheckout 的 owners 命中跳转）。
      entry({
        name: 'origin/dev',
        category: 'remote',
        remote: 'origin',
        fullName: 'refs/remotes/origin/dev',
      }),
    ],
    truncated: false,
  })
  // 只往真 window 上挂 api。`vi.stubGlobal('window', { api })` 会整个替换 window，
  // Radix 的 dismissable-layer 喊 `window.setTimeout is not a function` 就炸了。
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
/** Radix Popover 内容 portal 到 document.body，从挂载容器查永远是 null。 */
function rowInPopover(testId: string): HTMLElement {
  const el = document.querySelector(`[data-testid="${testId}"]`)
  if (!el) throw new Error(`expected popover row ${testId}`)
  return el as HTMLElement
}
const settle = async (): Promise<void> => {
  for (let i = 0; i < 8; i += 1) await flush()
}

async function openSwitcher(worktreeId = 'r::/a'): Promise<void> {
  await act(async () => {
    root.render(
      <SourceControlBranchSwitcher worktreeId={worktreeId}>
        <button type="button">main</button>
      </SourceControlBranchSwitcher>
    )
  })
  await click(host.querySelector('button'))
  await settle()
  rowInPopover('branch-tree')
}

describe('SourceControlBranchSwitcher', () => {
  it('把 children 原样渲染成触发器，不额外包可见文案', async () => {
    await act(async () => {
      root.render(
        <SourceControlBranchSwitcher worktreeId="r::/a">
          <span data-testid="trigger">main</span>
        </SourceControlBranchSwitcher>
      )
    })
    expect(host.querySelector('[data-testid="trigger"]')?.textContent).toBe('main')
  })

  it('未打开时不取 refs；打开后才取，且用 worktree 的 path', async () => {
    await act(async () => {
      root.render(
        <SourceControlBranchSwitcher worktreeId="r::/a">
          <button type="button">main</button>
        </SourceControlBranchSwitcher>
      )
    })
    await flush()
    expect(refs).not.toHaveBeenCalled()
    await openSwitcher()
    expect(refs).toHaveBeenCalledWith({ worktreePath: '/a' })
  })

  it('点无 owner 本地分支（干净无会话）→ 不弹框，直接 api.git.checkout（local target）', async () => {
    await openSwitcher()
    await click(rowInPopover('branch-row-ref:refs/heads/fresh'))
    await settle()
    expect(checkout).toHaveBeenCalledWith({
      worktreePath: '/a',
      target: { kind: 'local', name: 'fresh' },
    })
    expect(document.body.textContent).not.toContain('Checkout branch?')
    expect(activate).not.toHaveBeenCalled()
  })

  it('点远程分支 → remote target（建并跟踪）', async () => {
    await openSwitcher()
    await click(rowInPopover('branch-row-ref:refs/remotes/origin/main'))
    await settle()
    expect(checkout).toHaveBeenCalledWith({
      worktreePath: '/a',
      target: { kind: 'remote', name: 'origin/main' },
    })
  })

  it('有脏改动 → 先弹确认框，未点确认不调 checkout；点确认后才调', async () => {
    state.gitStatusByWorktree['r::/a'] = [{ path: 'a.ts' }]
    await openSwitcher()
    await click(rowInPopover('branch-row-ref:refs/heads/fresh'))
    await settle()
    expect(document.body.textContent).toContain('Checkout branch?')
    expect(checkout).not.toHaveBeenCalled()
    const confirm = [...document.querySelectorAll('button')].find(
      (b) => b.textContent?.trim() === 'Checkout'
    )
    await click(confirm ?? null)
    await settle()
    expect(checkout).toHaveBeenCalledWith({
      worktreePath: '/a',
      target: { kind: 'local', name: 'fresh' },
    })
  })

  it('终端 + agent 会话合并计数，且不漏计/不双计/不计 editor', async () => {
    // 生产形态：2 个终端在 tabsByWorktree 与 unified 各一份（镜像）；agent 会话只在 unified。
    // 正解 = 2 终端 + 1 agent = 3（editor 不计；unified 里的终端镜像不得重复加）。
    state.tabsByWorktree['r::/a'] = [{ id: 't1' }, { id: 't2' }]
    state.unifiedTabsByWorktree['r::/a'] = [
      { contentType: 'terminal' },
      { contentType: 'terminal' },
      { contentType: 'agent-session' },
      { contentType: 'editor' },
    ]
    await openSwitcher()
    await click(rowInPopover('branch-row-ref:refs/heads/fresh'))
    await settle()
    expect(document.body.textContent).toMatch(/3 running terminal\(s\)\/session\(s\)/)
    expect(checkout).not.toHaveBeenCalled() // 弹框态，未确认
  })

  it('F1：点被别的工作区检出的分支 → 走 switch 跳转，不调 checkout', async () => {
    await openSwitcher()
    await click(rowInPopover('branch-row-ref:refs/heads/dev'))
    await settle()
    expect(activate).toHaveBeenCalledTimes(1)
    expect(activate.mock.calls[0]?.[0]).toBe('r::/b')
    expect(activate.mock.calls[0]?.[1]).toEqual({ executionHostId: 'host-2' })
    expect(checkout).not.toHaveBeenCalled()
  })

  it('同宿主被检出分支：hostId 原样透传（缺省 undefined，不兜成某个宿主）', async () => {
    await openSwitcher()
    await click(rowInPopover('branch-row-ref:refs/heads/standby'))
    await settle()
    expect(activate.mock.calls[0]?.[0]).toBe('r::/c')
    expect(activate.mock.calls[0]?.[1]).toEqual({ executionHostId: undefined })
  })

  it('点当前 HEAD 分支 → 无动作（不 checkout 不 activate 不开 composer）', async () => {
    await openSwitcher()
    await click(rowInPopover('branch-row-ref:refs/heads/main'))
    await settle()
    expect(checkout).not.toHaveBeenCalled()
    expect(activate).not.toHaveBeenCalled()
    expect(state.openModal).not.toHaveBeenCalled()
  })

  it('checkout 成功后触发一次立即刷状态（refreshGitStatusForWorktree）', async () => {
    await openSwitcher()
    await click(rowInPopover('branch-row-ref:refs/heads/fresh'))
    await settle()
    expect(refreshGitStatusForWorktree).toHaveBeenCalledTimes(1)
  })

  it('checked-out-elsewhere 且 ade 未知占用方 → 用 worktreePath 弹提示，不静默', async () => {
    checkout.mockResolvedValueOnce({
      status: { refused: 'checked-out-elsewhere' },
      branch: 'ghost',
      worktreePath: '/ext/ghost',
    })
    await openSwitcher()
    await click(rowInPopover('branch-row-ref:refs/heads/ghost'))
    await settle()
    expect(toast.warning).toHaveBeenCalledWith(expect.stringContaining('/ext/ghost'))
    expect(activate).not.toHaveBeenCalled()
  })

  it('checked-out-elsewhere 且 ade 已知占用方（远程行解析到被占本地）→ 跳转，不弹外部提示', async () => {
    // origin/dev 是 checkout 可用的远程行；后端解析到本地 dev，而 dev 由 r::/b 占用。
    checkout.mockResolvedValueOnce({
      status: { refused: 'checked-out-elsewhere' },
      branch: 'dev',
      worktreePath: '/b',
    })
    await openSwitcher()
    await click(rowInPopover('branch-row-ref:refs/remotes/origin/dev'))
    await settle()
    expect(activate).toHaveBeenCalledWith('r::/b', { executionHostId: 'host-2' })
    expect(toast.warning).not.toHaveBeenCalled()
  })

  it('invalid-target → toast.error，且不刷状态', async () => {
    checkout.mockResolvedValueOnce({ status: 'invalid-target' })
    await openSwitcher()
    await click(rowInPopover('branch-row-ref:refs/heads/fresh'))
    await settle()
    expect(toast.error).toHaveBeenCalled()
    expect(refreshGitStatusForWorktree).not.toHaveBeenCalled()
  })

  it('环境级失败（git 拒绝覆盖本地改动）→ toast.error，且不刷状态', async () => {
    checkout.mockRejectedValueOnce(new Error('would be overwritten by checkout'))
    await openSwitcher()
    await click(rowInPopover('branch-row-ref:refs/heads/fresh'))
    await settle()
    expect(toast.error).toHaveBeenCalledWith('would be overwritten by checkout')
    expect(refreshGitStatusForWorktree).not.toHaveBeenCalled()
  })

  it('检出成功→刷状态带正确参数（worktreeId + interactive 档）', async () => {
    await openSwitcher()
    await click(rowInPopover('branch-row-ref:refs/heads/fresh'))
    await settle()
    expect(refreshGitStatusForWorktree).toHaveBeenCalledWith(
      expect.objectContaining({
        worktreeId: 'r::/a',
        worktreePath: '/a',
        request: { admissionTier: 'interactive' },
      })
    )
  })

  it('跳 repo：当前仓的 main 归属不被别仓同名分支顶掉（owner 映射按 repo 限定）', async () => {
    await openSwitcher()
    const el = rowInPopover('branch-row-ref:refs/heads/main')
    // fixture 里 other 排在 r 之前：若 owner 是全仓汇总，first-wins 会得到 'OTHER'。
    expect(el.getAttribute('title')).toContain('A')
    expect(el.getAttribute('title')).not.toContain('OTHER')
  })

  it('点 HEAD (Current Branch) 导航行不触发任何动作（本宿主无 view-in-log，主动作不存在）', async () => {
    await openSwitcher()
    await click(rowInPopover('branch-row-row:head-current'))
    await settle()
    expect(checkout).not.toHaveBeenCalled()
    expect(activate).not.toHaveBeenCalled()
    expect(state.openModal).not.toHaveBeenCalled()
  })

  it('本宿主不注入 All Branches 行（那是 Git Log 宿主专属）', async () => {
    await openSwitcher()
    expect(document.querySelector('[data-testid="branch-row-row:all-branches"]')).toBeNull()
  })

  it('当前分支带 ▣ 徽标：owner 映射按 store 的 branch 算', async () => {
    await openSwitcher()
    const el = rowInPopover('branch-row-ref:refs/heads/main')
    expect(el.querySelector('[data-testid="branch-workspace-badge"]')).not.toBeNull()
    expect(el.getAttribute('title')).toContain('A')
  })

  it('worktreeId 解析不到 worktree → 只渲染 children，点击不取数', async () => {
    await act(async () => {
      root.render(
        <SourceControlBranchSwitcher worktreeId="nope">
          <button type="button">main</button>
        </SourceControlBranchSwitcher>
      )
    })
    await click(host.querySelector('button'))
    await flush()
    expect(refs).not.toHaveBeenCalled()
    expect(document.querySelector('[data-testid="branch-tree"]')).toBeNull()
    expect(host.querySelector('button')?.textContent).toBe('main')
  })

  it('取数失败 → popover 里一行失败文案，不出树也不抛', async () => {
    refs.mockReset().mockRejectedValue(new Error('boom'))
    await act(async () => {
      root.render(
        <SourceControlBranchSwitcher worktreeId="r::/a">
          <button type="button">main</button>
        </SourceControlBranchSwitcher>
      )
    })
    await click(host.querySelector('button'))
    await settle()
    expect(document.querySelector('[data-testid="branch-tree"]')).toBeNull()
    expect(document.body.textContent).toContain('Failed to read branches.')
  })

  it('分支行渲染行尾 ⋯（宿主 A 声明 checkout → 菜单出现；⋯ 是行 button 的兄弟）', async () => {
    await openSwitcher()
    // 接缝把 ⋯ 作为行 <button> 的兄弟（非子），故在弹层全局查。
    expect(document.querySelector('[data-testid="branch-row-more"]')).not.toBeNull()
  })
})
