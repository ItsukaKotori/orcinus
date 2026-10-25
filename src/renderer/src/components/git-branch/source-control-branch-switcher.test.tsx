// @vitest-environment happy-dom
import { act } from 'react'
import { createRoot, type Root } from 'react-dom/client'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { GitRefEntry } from '../../../../shared/git-ref-types'
import type { Worktree } from '../../../../shared/worktree/types'
import { SourceControlBranchSwitcher } from './source-control-branch-switcher'

;(globalThis as unknown as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true

// 生产里 `Worktree.branch` 是**完整 ref**（`git worktree list --porcelain` 的
// `branch refs/heads/main` 被原样存下，`status.rs` 更是显式拼前缀）。
// fixture 写短名会让「忘剥前缀」这个 Critical 完全测不出来。
const worktrees: Worktree[] = [
  { id: 'r::/a', repoId: 'r', branch: 'refs/heads/main', displayName: 'A', path: '/a' },
  { id: 'r::/b', repoId: 'r', branch: 'refs/heads/dev', displayName: 'B', path: '/b', hostId: 'host-2' },
  // 另一个仓也有 `main`。owner 映射若跳 repo 汇总，它会把本仓 main 的归属顶掉。
  { id: 'other::/x', repoId: 'other', branch: 'refs/heads/main', displayName: 'OTHER', path: '/x' },
] as unknown as Worktree[]

const state = {
  // `other` **必须排在 `r` 之前**：旧的全仓压平实现靠 `Object.values` 插入序 +
  // first-wins，若 `r` 先入则跨仓 bug 下依然得到 A，测试对 C-1 就没有判别力。
  worktreesByRepo: {
    other: worktrees.filter((w) => w.repoId === 'other'),
    r: worktrees.filter((w) => w.repoId === 'r'),
  },
  openModal: vi.fn(),
}

const activate = vi.fn(
  (
    _id: string,
    _opts?: { executionHostId?: string }
  ): { primaryTabId: string | null } | false => ({ primaryTabId: null })
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
  activate.mockReset().mockReturnValue({ primaryTabId: null })
  refs.mockReset().mockResolvedValue({
    refs: [
      entry({ name: 'main', isHead: true }),
      entry({ name: 'dev' }),
      entry({ name: 'fresh' }),
      entry({
        name: 'origin/main',
        category: 'remote',
        remote: 'origin',
        fullName: 'refs/remotes/origin/main',
      }),
    ],
    truncated: false,
  })
  // 只往真 window 上挂 api。`vi.stubGlobal('window', { api })` 会整个替换 window，
  // Radix 的 dismissable-layer 喊 `window.setTimeout is not a function` 就炸了。
  ;(window as unknown as { api: unknown }).api = { git: { refs } }
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

async function openSwitcher(worktreeId = 'r::/a'): Promise<void> {
  await act(async () => {
    root.render(
      <SourceControlBranchSwitcher worktreeId={worktreeId}>
        <button type="button">main</button>
      </SourceControlBranchSwitcher>
    )
  })
  await click(host.querySelector('button'))
  // 弹层内容要先经过 loading（refs 为空）再到已解析，一个微任务不够。
  for (let i = 0; i < 5; i += 1) await flush()
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

  it('点有 owner 的分支 → activateAndRevealWorktree(id, {executionHostId})，且不开 composer', async () => {
    await openSwitcher()
    await click(rowInPopover('branch-row-ref:refs/heads/dev'))
    expect(activate).toHaveBeenCalledTimes(1)
    expect(activate.mock.calls[0]?.[0]).toBe('r::/b')
    // 宿主必须跟着走：裸 setActiveWorktree 会把 activeWorkspaceExecutionHostId 写成 null，
    // 跨宿主 workspace 的 tab 会被打戳成 local。
    expect(activate.mock.calls[0]?.[1]).toEqual({ executionHostId: 'host-2' })
    expect(state.openModal).not.toHaveBeenCalled()
  })

  it('同宿主 workspace 把 hostId 原样透传（缺省就是 undefined，不兜成某个具体宿主）', async () => {
    await openSwitcher()
    await click(rowInPopover('branch-row-ref:refs/heads/main'))
    expect(activate.mock.calls[0]?.[0]).toBe('r::/a')
    expect(activate.mock.calls[0]?.[1]).toEqual({ executionHostId: undefined })
  })

  it('跳 repo：当前仓的 main 归属自己仓的 workspace，不被别仓同名分支顶掉', async () => {
    await openSwitcher()
    const el = rowInPopover('branch-row-ref:refs/heads/main')
    // fixture 里 other 排在 r 之前：若 owner 是全仓汇总，first-wins 会得到 'OTHER'。
    expect(el.getAttribute('title')).toContain('A')
    expect(el.getAttribute('title')).not.toContain('OTHER')
    await click(el)
    expect(activate.mock.calls[0]?.[0]).toBe('r::/a')
  })

  it('激活返回 false（workspace 已被删）时不抛错', async () => {
    activate.mockReturnValue(false)
    await openSwitcher()
    await expect(click(rowInPopover('branch-row-ref:refs/heads/dev'))).resolves.not.toThrow()
    expect(activate).toHaveBeenCalledTimes(1)
  })

  it('点无 owner 的分支 → 打开 composer 并预填 initialBaseBranch', async () => {
    await openSwitcher()
    await click(rowInPopover('branch-row-ref:refs/heads/fresh'))
    expect(activate).not.toHaveBeenCalled()
    expect(state.openModal).toHaveBeenCalledWith('new-workspace-composer', {
      initialRepoId: 'r',
      initialBaseBranch: 'fresh',
      telemetrySource: 'source_control_branch_menu',
    })
  })

  it('点 remote 分支 → 一律新建 workspace，base 用去不掉的完整短名', async () => {
    await openSwitcher()
    await click(rowInPopover('branch-row-ref:refs/remotes/origin/main'))
    expect(state.openModal).toHaveBeenCalledWith(
      'new-workspace-composer',
      expect.objectContaining({ initialBaseBranch: 'origin/main' })
    )
    expect(activate).not.toHaveBeenCalled()
  })

  it('点 HEAD (Current Branch) 导航行不触发任何动作（本宿主无 view-in-log，主动作不存在）', async () => {
    await openSwitcher()
    await click(rowInPopover('branch-row-row:head-current'))
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
    for (let i = 0; i < 5; i += 1) await flush()
    expect(document.querySelector('[data-testid="branch-tree"]')).toBeNull()
    expect(document.body.textContent).toContain('Failed to read branches.')
  })
})
