import { describe, expect, it } from 'vitest'
import type { GitRefEntry } from '../../../../shared/git-ref-types'
import type { Worktree } from '../../../../shared/worktree/types'
import {
  buildBranchTreeRows,
  ownerByBranch,
  type BranchTreeRow,
} from './branch-tree-model'

function ref(partial: Partial<GitRefEntry> & { name: string; fullName: string }): GitRefEntry {
  return {
    category: 'local',
    oid: '0'.repeat(40),
    isHead: false,
    ...partial,
  }
}

const REFS: GitRefEntry[] = [
  ref({ name: 'main', fullName: 'refs/heads/main', isHead: true, ahead: 14 }),
  ref({ name: 'feature/login', fullName: 'refs/heads/feature/login' }),
  ref({
    name: 'origin/main',
    fullName: 'refs/remotes/origin/main',
    category: 'remote',
    remote: 'origin',
  }),
  ref({
    name: 'origin/dev',
    fullName: 'refs/remotes/origin/dev',
    category: 'remote',
    remote: 'origin',
  }),
  ref({
    name: 'upstream/main',
    fullName: 'refs/remotes/upstream/main',
    category: 'remote',
    remote: 'upstream',
  }),
  ref({ name: 'v1.2.0', fullName: 'refs/tags/v1.2.0', category: 'tag' }),
]

/** 折叠态 + 缩进 + 组 id 压成一串，方便一眼看出结构走样。 */
const shape = (rows: BranchTreeRow[]): string[] =>
  rows.map((r) => {
    const pad = '  '.repeat(r.depth)
    if (r.kind === 'group') return `${pad}[${r.id}${r.expanded ? '' : '!'}`
    if (r.kind === 'synthetic') return `${pad}${r.value}`
    return `${pad}${r.ref.name}`
  })

/** 取可见 ref 行的短名（带 kind 窄化，避开逐处强转）。 */
function refNames(rows: BranchTreeRow[], depth?: number): string[] {
  return refRows(rows)
    .filter((r) => depth === undefined || r.depth === depth)
    .map((r) => r.ref.name)
}

/**
 * Local 组的名字。**不能**用 depth===1 当代理：tag 行也在 depth 1，
 * 用深度筛会把 tags 混进来。
 */
function localNames(rows: BranchTreeRow[]): string[] {
  return refRows(rows)
    .filter((r) => r.ref.category === 'local')
    .map((r) => r.ref.name)
}

function refRows(rows: BranchTreeRow[]): Extract<BranchTreeRow, { kind: 'ref' }>[] {
  return rows.filter((r): r is Extract<BranchTreeRow, { kind: 'ref' }> => r.kind === 'ref')
}

function groupRows(rows: BranchTreeRow[]): Extract<BranchTreeRow, { kind: 'group' }>[] {
  return rows.filter((r): r is Extract<BranchTreeRow, { kind: 'group' }> => r.kind === 'group')
}

describe('buildBranchTreeRows', () => {
  it('按 IDEA 组序：Local → Remote（按 remote 二级分组）→ Tags', () => {
    expect(shape(buildBranchTreeRows({ refs: REFS }))).toEqual([
      'head-current',
      '[group:local',
      '  feature/login',
      '[group:remote',
      '  [group:remote/origin',
      '    origin/dev',
      '    origin/main',
      '  [group:remote/upstream',
      '    upstream/main',
      '[group:tags',
      '  v1.2.0',
    ])
  })

  it('Local 组排除当前分支（由 HEAD 合成行代表），其余字典序；remote 内按去前缀名排序', () => {
    const rows = buildBranchTreeRows({ refs: REFS })
    // spec §7.2 的「当前分支置顶」由 head-current 合成行实现，
    // 所以 Local 组内**不应**再出现 main。
    expect(localNames(rows)).toEqual(['feature/login'])
    // `origin/dev` 排在 `origin/main` 前，`upstream/main` 归自己的组：
    // 比的是去掉 `<remote>/` 前缀后的名字。
    expect(refNames(rows, 2)).toEqual(['origin/dev', 'origin/main', 'upstream/main'])
  })

  it('折叠的组只出行头，不出子行；childCount 仍是过滤后的条数', () => {
    const rows = buildBranchTreeRows({ refs: REFS, collapsed: new Set(['group:remote']) })
    expect(rows.some((r) => r.kind === 'ref')).toBe(true)
    expect(refNames(rows).filter((n) => n.includes('/'))).toEqual(['feature/login'])
    const remote = groupRows(rows).find((g) => g.id === 'group:remote')!
    expect(remote.expanded).toBe(false)
    expect(remote.childCount).toBe(3)
    // 折叠一级组时，二级组头也不能出现。
    expect(groupRows(rows).map((g) => g.id)).not.toContain('group:remote/origin')
  })

  it('折叠二级组只抹掉它自己的 remote，不动别的 remote', () => {
    const rows = buildBranchTreeRows({ refs: REFS, collapsed: new Set(['group:remote/origin']) })
    expect(groupRows(rows).map((g) => g.id)).toEqual([
      'group:local',
      'group:remote',
      'group:remote/origin',
      'group:remote/upstream',
      'group:tags',
    ])
    expect(refNames(rows)).toEqual(['feature/login', 'upstream/main', 'v1.2.0'])
  })

  it('空组不出行头', () => {
    const rows = buildBranchTreeRows({ refs: REFS.filter((r) => r.category !== 'tag') })
    expect(groupRows(rows).map((g) => g.id)).not.toContain('group:tags')
  })

  it('refs 里没有任何 isHead 时不出 head-current 行（detached HEAD）', () => {
    const rows = buildBranchTreeRows({
      refs: REFS.map((r) => ({ ...r, isHead: false })),
    })
    expect(rows.some((r) => r.kind === 'synthetic' && r.value === 'head-current')).toBe(false)
    // 此时当前分支没被任何条目代表，Local 组必须把 main 收回来，否则它凭空消失。
    expect(localNames(rows)).toEqual(['feature/login', 'main'])
  })

  it('includeAllBranchesRow 追加 All Branches 且排在 HEAD 之前', () => {
    const rows = buildBranchTreeRows({ refs: REFS, includeAllBranchesRow: true })
    expect(
      rows.filter((r) => r.kind === 'synthetic').map((r) => (r.kind === 'synthetic' ? r.value : '?!'))
    ).toEqual(['all-branches', 'head-current'])
    expect(refNames(rows)).not.toContain('main')
  })

  it('不注入时只有 head-current 一行合成人物', () => {
    const rows = buildBranchTreeRows({ refs: REFS })
    expect(rows.filter((r) => r.kind === 'synthetic')).toHaveLength(1)
  })

  it('isUpstreamOfHead 只标在当前分支的 upstream 那一条', () => {
    const rows = buildBranchTreeRows({
      refs: REFS.map((r) => (r.name === 'main' ? { ...r, upstream: 'origin/main' } : r)),
      headBranch: 'main',
    })
    expect(refRows(rows).filter((r) => r.isUpstreamOfHead).map((r) => r.ref.name)).toEqual([
      'origin/main',
    ])
  })

  it('headBranch 缺省时不标任何 upstream', () => {
    const rows = buildBranchTreeRows({
      refs: REFS.map((r) => (r.name === 'main' ? { ...r, upstream: 'origin/main' } : r)),
    })
    expect(refRows(rows).some((r) => r.isUpstreamOfHead)).toBe(false)
  })

  it('checkedOutByBranch 落到 ref 行的 checkedOutBy；remote/tag 行恒 null', () => {
    const owners = new Map([
      ['feature/login', { worktreeId: 'r::/b', displayName: 'B', path: '/b' }],
    ])
    const rows = buildBranchTreeRows({ refs: REFS, checkedOutByBranch: owners })
    const hit = refRows(rows).find((r) => r.ref.name === 'feature/login')!
    expect(hit.checkedOutBy).toEqual({ worktreeId: 'r::/b', displayName: 'B', path: '/b' })
    expect(refRows(rows).find((r) => r.ref.category === 'remote')!.checkedOutBy).toBeNull()
    expect(refRows(rows).find((r) => r.ref.category === 'tag')!.checkedOutBy).toBeNull()
  })

  it('空 refs 出空列表，不出任何组头', () => {
    expect(buildBranchTreeRows({ refs: [] })).toEqual([])
  })
})

describe('ownerByBranch', () => {
  it('跳过无 branch 的条目；同分支多 worktree 取第一个', () => {
    const worktrees = [
      { id: 'r::/a', branch: 'main', displayName: 'A', path: '/a' },
      { id: 'r::/b', branch: 'feature/login', displayName: 'B', path: '/b' },
      { id: 'r::/c', branch: '', displayName: 'C', path: '/c' },
      { id: 'r::/d', branch: 'main', displayName: 'D', path: '/d' },
    ] as unknown as Worktree[]
    const map = ownerByBranch(worktrees)
    expect([...map.keys()]).toEqual(['main', 'feature/login'])
    expect(map.get('main')).toEqual({ worktreeId: 'r::/a', displayName: 'A', path: '/a' })
  })

  it('空列表 → 空 Map', () => {
    expect(ownerByBranch([]).size).toBe(0)
  })
})
