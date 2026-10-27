// @vitest-environment happy-dom
import { act } from 'react'
import { createRoot, type Root } from 'react-dom/client'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { GitRefEntry } from '../../../../shared/git-ref-types'
import type { Worktree } from '../../../../shared/worktree/types'

// 隔离集成测试：mock 菜单原语为内联可点（仓库惯例，见 branch-menu.test）+ dialog 内联 +
// runtime barrel 的 create/upstream spy。测 section 的 new-branch / set-upstream 胶水。
vi.mock('@/components/ui/context-menu', () => ({
  ContextMenu: ({ children }: { children: React.ReactNode }) => <>{children}</>,
  ContextMenuTrigger: ({ children }: { children: React.ReactNode }) => <>{children}</>,
  ContextMenuContent: ({ children }: { children: React.ReactNode }) => <>{children}</>,
  ContextMenuItem: ({ children, onSelect }: { children: React.ReactNode; onSelect?: () => void }) => (
    <button type="button" data-testid="ctx-item" onClick={onSelect}>
      {children}
    </button>
  ),
  ContextMenuSub: ({ children }: { children: React.ReactNode }) => <>{children}</>,
  ContextMenuSubTrigger: ({ children }: { children: React.ReactNode }) => <span>{children}</span>,
  ContextMenuSubContent: ({ children }: { children: React.ReactNode }) => <>{children}</>,
}))
vi.mock('@/components/ui/dropdown-menu', () => ({
  DropdownMenu: ({ children }: { children: React.ReactNode }) => <>{children}</>,
  DropdownMenuTrigger: ({ children }: { children: React.ReactNode }) => <>{children}</>,
  DropdownMenuContent: ({ children }: { children: React.ReactNode }) => <>{children}</>,
  DropdownMenuItem: ({ children }: { children: React.ReactNode }) => <>{children}</>,
  DropdownMenuSub: ({ children }: { children: React.ReactNode }) => <>{children}</>,
  DropdownMenuSubTrigger: ({ children }: { children: React.ReactNode }) => <>{children}</>,
  DropdownMenuSubContent: ({ children }: { children: React.ReactNode }) => <>{children}</>,
}))
vi.mock('@/components/ui/dialog', () => ({
  Dialog: ({ children, open }: { children: React.ReactNode; open?: boolean }) =>
    open ? <div role="dialog">{children}</div> : null,
  DialogContent: ({ children }: { children: React.ReactNode }) => <div>{children}</div>,
  DialogHeader: ({ children }: { children: React.ReactNode }) => <div>{children}</div>,
  DialogTitle: ({ children }: { children: React.ReactNode }) => <div>{children}</div>,
  DialogDescription: ({ children }: { children: React.ReactNode }) => <div>{children}</div>,
  DialogFooter: ({ children }: { children: React.ReactNode }) => <div>{children}</div>,
}))
vi.mock('sonner', () => ({ toast: { warning: vi.fn(), error: vi.fn(), success: vi.fn() } }))
vi.mock('@/runtime/runtime-git-client', () => ({
  createRuntimeGitBranch: vi.fn().mockResolvedValue({ status: 'created', branch: 'x' }),
  setRuntimeGitBranchUpstream: vi.fn().mockResolvedValue({ status: 'set' }),
}))
vi.mock('@/lib/worktree-activation', () => ({ activateAndRevealWorktree: vi.fn(() => ({ primaryTabId: null })) }))

import { createRuntimeGitBranch, setRuntimeGitBranchUpstream } from '@/runtime/runtime-git-client'
import { SourceControlBranchTreeSection } from './source-control-branch-tree-section'

;(globalThis as unknown as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true

const worktrees = [
  { id: 'r::/a', repoId: 'r', branch: 'refs/heads/main', displayName: 'A', path: '/a' },
] as unknown as Worktree[]

const LOCAL: GitRefEntry = { name: 'feature', fullName: 'refs/heads/feature', category: 'local', oid: 'aaa', isHead: false }
const REMOTE: GitRefEntry = { name: 'origin/feature', fullName: 'refs/remotes/origin/feature', category: 'remote', remote: 'origin', oid: 'bbb', isHead: false }

const state = {
  worktreesByRepo: { r: worktrees },
  openModal: vi.fn(),
  gitStatusByWorktree: {},
  tabsByWorktree: {},
  unifiedTabsByWorktree: {},
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

const refs = vi.fn()
let host: HTMLDivElement
let root: Root

beforeEach(() => {
  host = document.createElement('div')
  document.body.append(host)
  root = createRoot(host)
  window.localStorage.clear()
  state.gitStatusByWorktree = {}
  refs.mockReset().mockResolvedValue({ refs: [LOCAL, REMOTE], truncated: false })
  ;(window as unknown as { api: unknown }).api = { git: { refs } }
  vi.mocked(createRuntimeGitBranch).mockClear().mockResolvedValue({ status: 'created', branch: 'x' })
  vi.mocked(setRuntimeGitBranchUpstream).mockClear().mockResolvedValue({ status: 'set' })
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
const clickEl = async (el: Element | null): Promise<void> => {
  await act(async () => {
    el?.dispatchEvent(new MouseEvent('click', { bubbles: true }))
  })
}
const byText = (testid: string, text: string): HTMLElement | undefined =>
  [...document.querySelectorAll<HTMLElement>(`[data-testid="${testid}"]`)].find(
    (el) => el.textContent?.trim() === text
  )

async function mount(): Promise<void> {
  await act(async () => {
    root.render(<SourceControlBranchTreeSection worktreeId="r::/a" refreshActiveGitStatusAfterMutation={async () => {}} settings={null} />)
  })
  for (let i = 0; i < 6; i += 1) await flush()
}

describe('SourceControlBranchTreeSection 写操作集成（菜单项 → runtime）', () => {
  it('New Branch from… → 名字框 → create 收到 start-point oid 并 invalidate', async () => {
    await mount()
    await clickEl(byText('ctx-item', 'New Branch from…') ?? null)
    await flush()
    // 名字框出现
    expect(document.querySelector('[data-testid="new-branch-name-input"]')).not.toBeNull()
    const input = document.querySelector('[data-testid="new-branch-name-input"]') as HTMLInputElement
    const setter = Object.getOwnPropertyDescriptor(window.HTMLInputElement.prototype, 'value')?.set
    await act(async () => {
      setter?.call(input, 'brand-new')
      input.dispatchEvent(new Event('input', { bubbles: true }))
    })
    await flush()
    const before = refs.mock.calls.length
    const createBtn = [...document.querySelectorAll('[role="dialog"] button')].find(
      (b) => b.textContent?.trim() === 'Create branch'
    )
    await clickEl(createBtn ?? null)
    await flush()
    expect(createRuntimeGitBranch).toHaveBeenCalledWith(
      expect.objectContaining({ worktreeId: 'r::/a' }),
      { name: 'brand-new', startPoint: 'aaa' }
    )
    expect(refs.mock.calls.length).toBeGreaterThan(before) // invalidate 重新取数
  })

  it('Set Upstream To… 子菜单 → setUpstream 收到 branch + 同名优先上游', async () => {
    await mount()
    // 子菜单候选项 origin/feature（同名优先）作为 ctx-item 内联渲染
    await clickEl(byText('ctx-item', 'origin/feature') ?? null)
    await flush()
    expect(setRuntimeGitBranchUpstream).toHaveBeenCalledWith(
      expect.objectContaining({ worktreeId: 'r::/a' }),
      { branch: 'feature', upstream: 'origin/feature' }
    )
  })
})
