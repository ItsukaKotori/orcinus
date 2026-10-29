// @vitest-environment happy-dom
import { act } from 'react'
import { createRoot, type Root } from 'react-dom/client'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { GitRefEntry } from '../../../../shared/git-ref-types'
import type { GitBranchChangeEntry, GitBranchCompareSummary } from '../../../../shared/git-diff-compare-types'
import type { Worktree } from '../../../../shared/worktree/types'

// Show Diff 宿主接线集成：mock 菜单原语内联 + runtime getRuntimeGitBranchCompare + store openBranchAllDiffs spy。
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
  DropdownMenuSubTrigger: ({ children }: { children: React.ReactNode }) => <span>{children}</span>,
  DropdownMenuSubContent: ({ children }: { children: React.ReactNode }) => <>{children}</>,
}))
vi.mock('sonner', () => ({ toast: { warning: vi.fn(), error: vi.fn(), success: vi.fn() } }))
vi.mock('@/runtime/runtime-git-client', () => ({
  createRuntimeGitBranch: vi.fn(),
  setRuntimeGitBranchUpstream: vi.fn(),
  renameRuntimeGitBranch: vi.fn(),
  deleteRuntimeGitBranch: vi.fn(),
  getRuntimeGitBranchCompare: vi.fn(),
}))
vi.mock('@/lib/worktree-activation', () => ({ activateAndRevealWorktree: vi.fn(() => ({ primaryTabId: null })) }))

import { getRuntimeGitBranchCompare } from '@/runtime/runtime-git-client'
import { toast } from 'sonner'
import { SourceControlBranchTreeSection } from './source-control-branch-tree-section'

;(globalThis as unknown as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true

const worktrees = [{ id: 'r::/a', repoId: 'r', branch: 'refs/heads/main', displayName: 'A', path: '/a' }] as unknown as Worktree[]
const LOCAL: GitRefEntry = { name: 'feature', fullName: 'refs/heads/feature', category: 'local', oid: 'aaa', isHead: false }
const HEAD: GitRefEntry = { name: 'main', fullName: 'refs/heads/main', category: 'local', oid: 'ccc', isHead: true }

const openBranchAllDiffs = vi.fn()
const state = {
  worktreesByRepo: { r: worktrees },
  openModal: vi.fn(),
  openBranchAllDiffs,
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
  refs.mockReset().mockResolvedValue({ refs: [HEAD, LOCAL], truncated: false })
  ;(window as unknown as { api: unknown }).api = { git: { refs } }
  openBranchAllDiffs.mockReset()
  vi.mocked(getRuntimeGitBranchCompare).mockReset()
})
afterEach(() => {
  act(() => root.unmount())
  host.remove()
  delete (window as unknown as { api?: unknown }).api
})

const flush = async (): Promise<void> => {
  await act(async () => { await Promise.resolve() })
}
const clickEl = async (el: Element | null): Promise<void> => {
  await act(async () => { el?.dispatchEvent(new MouseEvent('click', { bubbles: true })) })
}
const byText = (text: string): HTMLElement | undefined =>
  [...document.querySelectorAll<HTMLElement>('[data-testid="ctx-item"]')].find(
    (el) => el.textContent?.trim() === text
  )

async function mount(): Promise<void> {
  await act(async () => {
    root.render(<SourceControlBranchTreeSection worktreeId="r::/a" settings={null} />)
  })
  for (let i = 0; i < 6; i += 1) await flush()
}

const summary = (status: GitBranchCompareSummary['status']): GitBranchCompareSummary => ({
  baseRef: 'feature', baseOid: 'b', compareRef: 'HEAD', headOid: 'h', mergeBase: 'm', changedFiles: 2, status
})

describe('SourceControlBranchTreeSection Show Diff 接线', () => {
  it('点 Show Diff（local feature）→ compare(ref.name) → openBranchAllDiffs 带该分支 entries', async () => {
    const entries: GitBranchChangeEntry[] = [{ path: 'a.ts', status: 'modified' }, { path: 'b.ts', status: 'added' }]
    vi.mocked(getRuntimeGitBranchCompare).mockResolvedValue({ summary: summary('ready'), entries })
    await mount()
    await clickEl(byText('Show Diff with Working Tree') ?? null)
    await flush()
    expect(getRuntimeGitBranchCompare).toHaveBeenCalledWith(
      expect.objectContaining({ worktreeId: 'r::/a', worktreePath: '/a' }),
      'feature'
    )
    expect(openBranchAllDiffs).toHaveBeenCalledWith('r::/a', '/a', summary('ready'), undefined, entries)
  })

  it('compare 非 ready（no-merge-base）→ toast.error、不开 tab', async () => {
    vi.mocked(getRuntimeGitBranchCompare).mockResolvedValue({ summary: summary('no-merge-base'), entries: [] })
    await mount()
    await clickEl(byText('Show Diff with Working Tree') ?? null)
    await flush()
    expect(toast.error).toHaveBeenCalled()
    expect(openBranchAllDiffs).not.toHaveBeenCalled()
  })

  it('compare reject → toast.error、不开 tab', async () => {
    vi.mocked(getRuntimeGitBranchCompare).mockRejectedValue(new Error('boom'))
    await mount()
    await clickEl(byText('Show Diff with Working Tree') ?? null)
    await flush()
    expect(toast.error).toHaveBeenCalledWith('boom')
    expect(openBranchAllDiffs).not.toHaveBeenCalled()
  })

  it('当前分支（isHead）无 Show Diff 项（谓词裁剪，即便进 HOST_A_ALLOWED）', async () => {
    vi.mocked(getRuntimeGitBranchCompare).mockResolvedValue({ summary: summary('ready'), entries: [] })
    await mount()
    // 只有 feature 行出一个 Show Diff；main(isHead) 不产该项 → 全文只应命中 1 个。
    const hits = [...document.querySelectorAll<HTMLElement>('[data-testid="ctx-item"]')].filter(
      (el) => el.textContent?.trim() === 'Show Diff with Working Tree'
    )
    expect(hits).toHaveLength(1)
  })
})
