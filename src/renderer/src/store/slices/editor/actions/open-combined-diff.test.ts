import { describe, expect, it, vi } from 'vitest'

// 只测 openBranchAllDiffs 的 entries→tab 快照这一接缝；重副作用与快照换算全部桩掉。
vi.mock('../git/git-status-reconciliation', () => ({
  toBranchCompareSnapshot: (s: { baseRef: string }) => ({ ...s, compareVersion: 'v1' }),
  toCommitCompareSnapshot: (s: unknown) => s
}))
vi.mock('../git/diff-runtime-owner', () => ({ resolveDiffRuntimeEnvironmentId: () => null }))
vi.mock('../tabs/workspace-editor-item', () => ({ openWorkspaceEditorItem: vi.fn() }))

import { createOpenCombinedDiff } from './open-combined-diff'
import type { GitBranchChangeEntry, GitBranchCompareSummary } from '../../../../../../shared/git-diff-compare-types'

const CACHE: GitBranchChangeEntry = { path: 'base-cached.ts', status: 'modified' }
const X1: GitBranchChangeEntry = { path: 'feature-a.ts', status: 'added' }
const X2: GitBranchChangeEntry = { path: 'feature-b.ts', status: 'modified' }

const summary = (baseRef: string): GitBranchCompareSummary => ({
  baseRef,
  baseOid: 'b',
  compareRef: 'HEAD',
  headOid: 'h',
  mergeBase: 'm',
  changedFiles: 1,
  status: 'ready'
})

// 用 set(updater) 抓 zustand 局部 patch，检查写入的 OpenFile.branchEntriesSnapshot。
function runOpen(openFiles: unknown[], entriesArg?: GitBranchChangeEntry[]): Record<string, unknown> {
  let patch: Record<string, unknown> = {}
  const base = { openFiles, gitBranchChangesByWorktree: { 'wt-1': [CACHE] } }
  const set = (updater: (s: unknown) => Record<string, unknown>) => {
    patch = updater(base)
  }
  const get = () => base
  const actions = createOpenCombinedDiff(set as never, get as never)
  actions.openBranchAllDiffs('wt-1', '/repo', summary('feature'), undefined, entriesArg)
  return patch
}

describe('openBranchAllDiffs 显式 entries（增量3b）', () => {
  it('传入 entries → tab 快照恰为该 entries（不串当前 worktree 缓存）', () => {
    const patch = runOpen([], [X1, X2])
    const file = (patch.openFiles as Record<string, unknown>[])[0]
    expect(file.branchEntriesSnapshot).toEqual([X1, X2])
    expect(file.relativePath).toBe('Branch Changes (feature)')
    expect(file.diffSource).toBe('combined-branch')
  })

  it('不传 entries → 快照取当前 worktree 缓存（向后兼容现有 Branch Changes）', () => {
    const patch = runOpen([])
    const file = (patch.openFiles as Record<string, unknown>[])[0]
    expect(file.branchEntriesSnapshot).toEqual([CACHE])
  })

  it('tab id 按 baseRef 去重：同 ref 已存在则复用、不新增', () => {
    const existing = [{ id: 'wt-1::all-diffs::branch::feature::v1', branchEntriesSnapshot: [], branchCompare: {} }]
    const patch = runOpen(existing, [X1])
    expect(patch.openFiles).toHaveLength(1)
    expect((patch.openFiles as Record<string, unknown>[])[0].branchEntriesSnapshot).toEqual([X1])
  })
})
