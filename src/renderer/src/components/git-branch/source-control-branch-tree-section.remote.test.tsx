// @vitest-environment happy-dom
import { act } from 'react'
import { createRoot, type Root } from 'react-dom/client'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { GitRefEntry } from '../../../../shared/git-ref-types'
import type { Worktree } from '../../../../shared/worktree/types'

// 增量5：树 Update/Push 宿主接线集成。菜单原语内联；store 远端动作 spy。
vi.mock('@/components/ui/context-menu', () => ({
  ContextMenu: ({ children }: { children: React.ReactNode }) => <>{children}</>,
  ContextMenuTrigger: ({ children }: { children: React.ReactNode }) => <>{children}</>,
  ContextMenuContent: ({ children }: { children: React.ReactNode }) => <>{children}</>,
  ContextMenuItem: ({ children, onSelect }: { children: React.ReactNode; onSelect?: () => void }) => (
    <button type="button" data-testid="ctx-item" onClick={onSelect}>{children}</button>
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
  getRuntimeGitBranchCompare: vi.fn().mockResolvedValue({ summary: { status: 'ready' }, entries: [] }),
}))
vi.mock('@/lib/worktree-activation', () => ({ activateAndRevealWorktree: vi.fn(() => ({ primaryTabId: null })) }))

import { SourceControlBranchTreeSection } from './source-control-branch-tree-section'

;(globalThis as unknown as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true

// 两个同仓 worktree：current 检出 main（有 upstream），另一检出 solo（无 upstream→push 走 publish）。
const worktrees = [
  { id: 'r::/a', repoId: 'r', branch: 'refs/heads/main', displayName: 'A', path: '/a' },
  { id: 'r::/b', repoId: 'r', branch: 'refs/heads/solo', displayName: 'B', path: '/b' },
] as unknown as Worktree[]

const MAIN: GitRefEntry = { name: 'main', fullName: 'refs/heads/main', category: 'local', oid: 'm', isHead: true, upstream: 'origin/main' }
const SOLO: GitRefEntry = { name: 'solo', fullName: 'refs/heads/solo', category: 'local', oid: 's', isHead: false }

const pullBranch = vi.fn(async () => {})
const pushBranch = vi.fn(async () => {})
const state = {
  worktreesByRepo: { r: worktrees },
  openModal: vi.fn(),
  pullBranch,
  pushBranch,
  isRemoteOperationActive: false,
  gitStatusByWorktree: {},
  tabsByWorktree: {},
  unifiedTabsByWorktree: {},
  openBranchAllDiffs: vi.fn(),
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
  refs.mockReset().mockResolvedValue({ refs: [MAIN, SOLO], truncated: false })
  ;(window as unknown as { api: unknown }).api = { git: { refs } }
  pullBranch.mockClear()
  pushBranch.mockClear()
  state.isRemoteOperationActive = false
})
afterEach(() => {
  act(() => root.unmount())
  host.remove()
  delete (window as unknown as { api?: unknown }).api
})

const flush = async (): Promise<void> => { await act(async () => { await Promise.resolve() }) }
const clickEl = async (el: Element | null): Promise<void> => {
  await act(async () => { el?.dispatchEvent(new MouseEvent('click', { bubbles: true })) })
}
const byText = (text: string): HTMLElement | undefined =>
  [...document.querySelectorAll<HTMLElement>('[data-testid="ctx-item"]')].find((el) => el.textContent?.trim() === text)

async function mount(): Promise<void> {
  await act(async () => { root.render(<SourceControlBranchTreeSection worktreeId="r::/a" settings={null} />) })
  for (let i = 0; i < 6; i += 1) await flush()
}

describe('增量5 树 Update/Push 接线', () => {
  it('Update（当前 main）→ 用 owner worktree 上下文调 pullBranch → invalidate', async () => {
    await mount()
    const before = refs.mock.calls.length
    await clickEl(byText('Update') ?? null)
    await flush()
    expect(pullBranch).toHaveBeenCalledWith('r::/a', '/a', undefined, undefined, { runtimeTargetSettings: null })
    expect(refs.mock.calls.length).toBeGreaterThan(before) // invalidate 重取
  })

  it('Push（solo 无 upstream）→ publish=true；Push（main 有 upstream）→ publish=false', async () => {
    await mount()
    const pushes = [...document.querySelectorAll<HTMLElement>('[data-testid="ctx-item"]')].filter((el) => el.textContent?.trim() === 'Push…')
    expect(pushes.length).toBe(2) // main + solo 都被检出 → 各一个 Push
    await clickEl(pushes[0] ?? null) // 首个 = main（refs 顺序）
    await flush()
    expect(pushBranch).toHaveBeenCalledWith('r::/a', '/a', false, undefined, undefined, { runtimeTargetSettings: null })
    await clickEl(pushes[1] ?? null) // solo（无 upstream → publish=true）
    await flush()
    expect(pushBranch).toHaveBeenCalledWith('r::/b', '/b', true, undefined, undefined, { runtimeTargetSettings: null })
  })

  it('远端进行中 → executeUpdate 前置守卫，不调 pullBranch（即便菜单可点）', async () => {
    state.isRemoteOperationActive = true
    await mount()
    await clickEl(byText('Update') ?? null)
    await flush()
    expect(pullBranch).not.toHaveBeenCalled()
  })
})
