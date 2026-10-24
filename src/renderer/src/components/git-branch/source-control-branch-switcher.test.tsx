// @vitest-environment happy-dom
import { act } from 'react'
import { createRoot, type Root } from 'react-dom/client'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { GitRefEntry } from '../../../../shared/git-ref-types'
import type { Worktree } from '../../../../shared/worktree/types'
import { SourceControlBranchSwitcher } from './source-control-branch-switcher'

;(globalThis as unknown as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true

const worktrees: Worktree[] = [
  { id: 'r::/a', repoId: 'r', branch: 'main', displayName: 'A', path: '/a' },
  { id: 'r::/b', repoId: 'r', branch: 'dev', displayName: 'B', path: '/b' },
] as unknown as Worktree[]

const state = {
  worktreesByRepo: { r: worktrees },
  setActiveWorktree: vi.fn(),
  openModal: vi.fn(),
}

vi.mock('@/store', () => {
  const useAppStore = Object.assign(
    (selector: (s: typeof state) => unknown) => selector(state),
    { getState: () => state }
  )
  return { useAppStore }
})

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
  state.setActiveWorktree.mockReset()
  state.openModal.mockReset()
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

  it('点有 owner 的分支 → setActiveWorktree(该 worktreeId)，且不开新建 composer', async () => {
    await openSwitcher()
    await click(rowInPopover('branch-row-ref:refs/heads/dev'))
    expect(state.setActiveWorktree).toHaveBeenCalledTimes(1)
    expect(state.setActiveWorktree.mock.calls[0]?.[0]).toBe('r::/b')
    expect(state.openModal).not.toHaveBeenCalled()
  })

  it('点无 owner 的分支 → 打开 composer 并预填 initialBaseBranch', async () => {
    await openSwitcher()
    await click(rowInPopover('branch-row-ref:refs/heads/fresh'))
    expect(state.setActiveWorktree).not.toHaveBeenCalled()
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
    expect(state.setActiveWorktree).not.toHaveBeenCalled()
  })

  it('点 HEAD (Current Branch) 导航行不触发任何动作（本宿主无 view-in-log，主动作不存在）', async () => {
    await openSwitcher()
    await click(rowInPopover('branch-row-row:head-current'))
    expect(state.setActiveWorktree).not.toHaveBeenCalled()
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
