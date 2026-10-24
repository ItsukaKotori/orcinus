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
      '  main',
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

  it('当前分支既在 Local 置顶、又有 head-current 合成行（spec §7.1/§7.2）', () => {
    const rows = buildBranchTreeRows({ refs: REFS })
    // 两行语义不同：合成行 = 当前分支范围；Local 里的 main = 该 ref 本身。
    // 合并它们会让当前分支无法作为普通 ref 被选中（Git Log 设查看 ref / 右键）。
    expect(rows.some((r) => r.kind === 'synthetic' && r.value === 'head-current')).toBe(true)
    // 置顶不是巧合：main 码点序本应排在 feature/login 之后。
    expect(localNames(rows)).toEqual(['main', 'feature/login'])
    // 可达性：当前分支仍是一条 depth-1 的 ref 行（而非只剩合成行）。
    expect(refRows(rows).find((r) => r.ref.name === 'main')!.depth).toBe(1)
    // §7.1 的 🏷 图标必须可达：Local 里确实有一行 isHead。
    expect(refRows(rows).filter((r) => r.ref.isHead).map((r) => r.ref.name)).toEqual(['main'])
  })

  it('isHead 即使码点序不排第一也被置顶（置顶是显式的，不靠巧合）', () => {
    const rows = buildBranchTreeRows({
      refs: [
        ref({ name: 'Cache', fullName: 'refs/heads/Cache' }),
        ref({ name: 'main', fullName: 'refs/heads/main', isHead: true }),
      ],
    })
    expect(localNames(rows)).toEqual(['main', 'Cache'])
  })

  it('用码点序而非 localeCompare（否则同一仓库在不同宿主 locale 下排出不同序）', () => {
    // 实测分岔：localeCompare → [a-b, ab, aB, Ab]；码点序 → [Ab, a-b, aB, ab]。
    const rows = buildBranchTreeRows({
      refs: ['ab', 'a-b', 'Ab', 'aB'].map((n) => ref({ name: n, fullName: `refs/heads/${n}` })),
    })
    expect(localNames(rows)).toEqual(['Ab', 'a-b', 'aB', 'ab'])
  })

  it('remote 组按 remote 名码点序排列（origin 在 upstream 前）', () => {
    const rows = buildBranchTreeRows({ refs: REFS })
    expect(groupRows(rows).map((g) => g.id).filter((id) => id.startsWith('group:remote/'))).toEqual([
      'group:remote/origin',
      'group:remote/upstream',
    ])
  })

  it('Remote 子组内按名称码点序（不再剥公共前缀）', () => {
    const rows = buildBranchTreeRows({ refs: REFS })
    // depth 2 = Remote 子组的行。不剥 `<remote>/` 前缀：同组内前缀相同，剥了也不变序。
    expect(refNames(rows, 2)).toEqual(['origin/dev', 'origin/main', 'upstream/main'])
  })

  it('折叠的组只出行头，不出子行；childCount 仍是过滤后的条数', () => {
    const rows = buildBranchTreeRows({ refs: REFS, collapsed: new Set(['group:remote']) })
    expect(rows.some((r) => r.kind === 'ref')).toBe(true)
    // remote 组已折叠 → 带 `/` 的名字只剩 Local 里的 feature/login。
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
    expect(refNames(rows)).toEqual([
      'main',
      'feature/login',
      'upstream/main',
      'v1.2.0',
    ])
  })

  it('空组不出行头', () => {
    const rows = buildBranchTreeRows({ refs: REFS.filter((r) => r.category !== 'tag') })
    expect(groupRows(rows).map((g) => g.id)).not.toContain('group:tags')
  })

  it('refs 里没有任何 isHead 时不出 head-current 行（detached HEAD）', () => {
    const rows = buildBranchTreeRows({
      refs: REFS.map((r) => ({ ...r, isHead: false })),
    })
    // 无 isHead 条目时不出合成行，且 Local 仍含全部分支（当前分支不会只剩一行代表）。
    expect(rows.some((r) => r.kind === 'synthetic' && r.value === 'head-current')).toBe(false)
    expect(localNames(rows)).toEqual(['feature/login', 'main'])
  })

  it('includeAllBranchesRow 追加 All Branches 且排在 HEAD 之前', () => {
    const rows = buildBranchTreeRows({ refs: REFS, includeAllBranchesRow: true })
    expect(
      rows.filter((r) => r.kind === 'synthetic').map((r) => (r.kind === 'synthetic' ? r.value : '?!'))
    ).toEqual(['all-branches', 'head-current'])
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

  it('headBranch 缺省时回退到 git 报的 isHead 来算 upstream', () => {
    // 不要求宿主必须传 headBranch：refs 里已有 isHead 时它就是权威来源。
    const rows = buildBranchTreeRows({
      refs: REFS.map((r) => (r.name === 'main' ? { ...r, upstream: 'origin/main' } : r)),
    })
    expect(refRows(rows).filter((r) => r.isUpstreamOfHead).map((r) => r.ref.name)).toEqual([
      'origin/main',
    ])
  })

  it('既无 headBranch 也无 isHead 条目时不标任何 upstream', () => {
    const rows = buildBranchTreeRows({
      refs: REFS.map((r) =>
        r.name === 'main' ? { ...r, upstream: 'origin/main', isHead: false } : r
      ),
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

  it('搜索抹掉当前分支行时，head-current 导航行仍在（靠 headBranch 而非 refs）', () => {
    // 宿主传的是已过滤的 refs；`main` 不匹配查询时被抹掉，
    // 但 `HEAD (Current Branch)` 是导航行不是搜索结果，必须留在顶。
    const filtered = REFS.filter((r) => r.name.includes('login'))
    expect(filtered.some((r) => r.name === 'main')).toBe(false)
    const rows = buildBranchTreeRows({ refs: filtered, headBranch: 'main' })
    expect(
      rows.filter((r) => r.kind === 'synthetic').map((r) => (r.kind === 'synthetic' ? r.value : '?!'))
    ).toEqual(['head-current'])
    expect(localNames(rows)).toEqual(['feature/login'])
  })

  it('既无 headBranch 也无 isHead 条目时不出 head-current', () => {
    const rows = buildBranchTreeRows({
      refs: REFS.map((r) => ({ ...r, isHead: false })),
    })
    expect(rows.some((r) => r.kind === 'synthetic')).toBe(false)
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
