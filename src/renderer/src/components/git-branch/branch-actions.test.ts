import { describe, expect, it, vi } from 'vitest'
import type { GitRefEntry } from '../../../../shared/git-ref-types'
import type { BranchTreeRow, BranchWorkspaceOwner } from './branch-tree-model'
import {
  BRANCH_ACTIONS,
  branchActionForRef,
  primaryBranchAction,
  type BranchActionContext,
} from './branch-actions'

const refEntry = (over: Partial<GitRefEntry> & { name: string }): GitRefEntry => ({
  fullName: `refs/heads/${over.name}`,
  category: 'local',
  oid: '0'.repeat(40),
  isHead: false,
  ...over,
})

function refRow(
  ref: GitRefEntry,
  checkedOutBy: BranchWorkspaceOwner | null = null
): BranchTreeRow {
  return { kind: 'ref', id: `ref:${ref.fullName}`, depth: 1, ref, isUpstreamOfHead: false, checkedOutBy }
}

const groupRow = (): BranchTreeRow => ({
  kind: 'group',
  id: 'group:local',
  labelKey: 'auto.components.git-branch.groupLocal',
  labelFallback: 'Local',
  depth: 0,
  expandable: true,
  expanded: true,
  childCount: 2,
})

const synthetic = (value: 'all-branches' | 'head-current'): BranchTreeRow => ({
  kind: 'synthetic',
  id: `row:${value}`,
  depth: 0,
  labelKey: `k:${value}`,
  labelFallback: value,
  value,
})

const OWNER: BranchWorkspaceOwner = { worktreeId: 'r::/b', displayName: 'B', path: '/b' }

function ctx(over: Partial<BranchActionContext> = {}): BranchActionContext {
  return {
    headBranch: 'main',
    ownerOf: () => null,
    activateWorkspace: vi.fn(),
    newWorkspaceFrom: vi.fn(),
    viewInGitLog: vi.fn(),
    ...over,
  }
}

/** 矩阵断言用的局部枚举：不进生产代码（增量 1 唯一生产消费者是 primaryBranchAction）。 */
const visibleActions = (row: BranchTreeRow, c: BranchActionContext): string[] =>
  BRANCH_ACTIONS.filter((a) => a.visible(row, c)).map((a) => a.id)

describe('BRANCH_ACTIONS', () => {
  it('label 是惰性函数且非空（no-top-level-translate AST 门禁要求）', () => {
    expect(BRANCH_ACTIONS.length).toBeGreaterThan(0)
    for (const action of BRANCH_ACTIONS) {
      expect(typeof action.label).toBe('function')
      expect(action.label().length).toBeGreaterThan(0)
    }
  })

  it('每个动作都有 id / visible / run', () => {
    for (const action of BRANCH_ACTIONS) {
      expect(typeof action.id).toBe('string')
      expect(typeof action.visible).toBe('function')
      expect(typeof action.run).toBe('function')
    }
  })
})

describe('branchActionForRef', () => {
  it('有 owner 的本地分支 → switch；无 owner → new-workspace', () => {
    const ownerOf = (name: string) => (name === 'main' ? OWNER : null)
    expect(branchActionForRef(refEntry({ name: 'main' }), ownerOf)).toBe('switch')
    expect(branchActionForRef(refEntry({ name: 'dev' }), ownerOf)).toBe('new-workspace')
  })

  it('remote / tag 即使 ownerOf 能查到同名也走 new-workspace', () => {
    const ownerOf = () => OWNER
    expect(
      branchActionForRef(
        refEntry({ name: 'main', category: 'remote', remote: 'origin', fullName: 'refs/remotes/origin/main' }),
        ownerOf
      )
    ).toBe('new-workspace')
    expect(
      branchActionForRef(refEntry({ name: 'v1', category: 'tag', fullName: 'refs/tags/v1' }), ownerOf)
    ).toBe('new-workspace')
  })
})

describe('可见性矩阵（spec §7.4）', () => {
  it('组头行不出任何动作', () => {
    expect(visibleActions(groupRow(), ctx())).toEqual([])
  })

  it('有 owner 的本地分支：switch + new-workspace + view-in-log', () => {
    expect(visibleActions(refRow(refEntry({ name: 'main' }), OWNER), ctx({ ownerOf: () => OWNER }))).toEqual([
      'switch',
      'new-workspace',
      'view-in-log',
    ])
  })

  it('宿主裁剪靠「不传 viewInGitLog」，不给动作表加 opts', () => {
    expect(
      visibleActions(refRow(refEntry({ name: 'main' }), OWNER), ctx({ ownerOf: () => OWNER, viewInGitLog: undefined }))
    ).toEqual(['switch', 'new-workspace'])
  })

  it('switch 对无 owner 的行不可见（其余动作不受影响）', () => {
    const ids = visibleActions(refRow(refEntry({ name: 'dev' }), null), ctx())
    expect(ids).toEqual(['new-workspace', 'view-in-log'])
    expect(ids).not.toContain('switch')
  })

  it('remote / tag 行不出 switch', () => {
    const c = ctx({ ownerOf: () => OWNER })
    const remote = refRow(
      refEntry({ name: 'origin/main', category: 'remote', remote: 'origin', fullName: 'refs/remotes/origin/main' }),
      OWNER
    )
    expect(visibleActions(remote, c)).toEqual(['new-workspace', 'view-in-log'])
    const tag = refRow(refEntry({ name: 'v1', category: 'tag', fullName: 'refs/tags/v1' }), OWNER)
    expect(visibleActions(tag, c)).toEqual(['new-workspace', 'view-in-log'])
  })

  it('合成人物只出 view-in-log；Source Control 宿主下为空', () => {
    expect(visibleActions(synthetic('head-current'), ctx())).toEqual(['view-in-log'])
    expect(visibleActions(synthetic('head-current'), ctx({ viewInGitLog: undefined }))).toEqual([])
  })
})

describe('primaryBranchAction', () => {
  it('有 owner 的本地分支 → switch，run 带 owner', () => {
    const activateWorkspace = vi.fn()
    const row = refRow(refEntry({ name: 'main' }), OWNER)
    const c = ctx({ ownerOf: () => OWNER, activateWorkspace })
    expect(primaryBranchAction(row, c)?.id).toBe('switch')
    primaryBranchAction(row, c)?.run(row, c)
    expect(activateWorkspace).toHaveBeenCalledWith(OWNER)
  })

  it('无 owner → new-workspace，run 带 ref 本体', () => {
    const newWorkspaceFrom = vi.fn()
    const ref = refEntry({ name: 'dev' })
    const row = refRow(ref)
    const c = ctx({ newWorkspaceFrom })
    expect(primaryBranchAction(row, c)?.id).toBe('new-workspace')
    primaryBranchAction(row, c)?.run(row, c)
    expect(newWorkspaceFrom).toHaveBeenCalledWith(ref)
  })

  it('remote 行的主动作是 new-workspace，且绝不会是 switch', () => {
    const c = ctx({ ownerOf: () => OWNER })
    const remote = refRow(
      refEntry({ name: 'origin/main', category: 'remote', remote: 'origin', fullName: 'refs/remotes/origin/main' }),
      OWNER
    )
    expect(primaryBranchAction(remote, c)?.id).toBe('new-workspace')
  })

  it('合成人物在 Source Control 宿主返回 null（吞掉点击，不做假动作）', () => {
    const c = ctx({ viewInGitLog: undefined })
    expect(primaryBranchAction(synthetic('head-current'), c)).toBeNull()
    expect(primaryBranchAction(synthetic('all-branches'), c)).toBeNull()
  })

  it('合成人物在 Git Log 宿主返回 view-in-log，run 带 value', () => {
    const viewInGitLog = vi.fn()
    const c = ctx({ viewInGitLog })
    const row = synthetic('all-branches')
    expect(primaryBranchAction(row, c)?.id).toBe('view-in-log')
    primaryBranchAction(row, c)?.run(row, c)
    expect(viewInGitLog).toHaveBeenCalledWith('all-branches')
  })

  it('组头行返回 null', () => {
    expect(primaryBranchAction(groupRow(), ctx())).toBeNull()
  })

  it('switch 的 run 在 owner 消失时不抛（点击与执行之间 owner 可能已变）', () => {
    const activateWorkspace = vi.fn()
    const row = refRow(refEntry({ name: 'main' }), OWNER)
    const gone = ctx({ ownerOf: () => null, activateWorkspace })
    const action = BRANCH_ACTIONS.find((a) => a.id === 'switch')!
    expect(() => action.run(row, gone)).not.toThrow()
    expect(activateWorkspace).not.toHaveBeenCalled()
  })
})

const findAction = (id: string) => BRANCH_ACTIONS.find((a) => a.id === id)!

describe('checkout 动作（增量 2B）', () => {
  it('宿主声明 checkout：本地非 HEAD 分支可见且 enabled', () => {
    const row = refRow(refEntry({ name: 'dev' }))
    expect(findAction('checkout').visible(row, ctx({ checkout: vi.fn() }))).toBe(true)
    expect(findAction('checkout').enabled?.(row, ctx({ checkout: vi.fn() }))).toBe(true)
  })

  it('当前 HEAD 行不出 checkout（不原地检出自己）', () => {
    expect(
      findAction('checkout').visible(refRow(refEntry({ name: 'main', isHead: true })), ctx({ checkout: vi.fn() }))
    ).toBe(false)
  })

  it('tag 行不出 checkout', () => {
    const tag = refRow(refEntry({ name: 'v1', category: 'tag', fullName: 'refs/tags/v1' }))
    expect(findAction('checkout').visible(tag, ctx({ checkout: vi.fn() }))).toBe(false)
  })

  it('宿主不声明 checkout → checkout 不可见（宿主 B 门禁：绝不出现在原地检出）', () => {
    expect(findAction('checkout').visible(refRow(refEntry({ name: 'dev' })), ctx())).toBe(false)
  })

  it('被其它 workspace 检出的本地分支：可见但 disabled（F1 置灰）', () => {
    const row = refRow(refEntry({ name: 'dev' }), OWNER)
    expect(findAction('checkout').visible(row, ctx({ checkout: vi.fn() }))).toBe(true)
    expect(findAction('checkout').enabled?.(row, ctx({ checkout: vi.fn() }))).toBe(false)
  })

  it('run 把 ref 本体交给 ctx.checkout', () => {
    const run = vi.fn()
    const ref = refEntry({ name: 'dev' })
    findAction('checkout').run(refRow(ref), ctx({ checkout: run }))
    expect(run).toHaveBeenCalledWith(ref)
  })

  it('group / synthetic 行即使声明 checkout 也不可见（refOf 守卫）', () => {
    const c = ctx({ checkout: vi.fn() })
    expect(findAction('checkout').visible(groupRow(), c)).toBe(false)
    expect(findAction('checkout').visible(synthetic('head-current'), c)).toBe(false)
  })
})

describe('primaryBranchAction + checkout 宿主', () => {
  it('声明 checkout：可用分支主动作 = checkout', () => {
    expect(primaryBranchAction(refRow(refEntry({ name: 'fresh' })), ctx({ checkout: vi.fn() }))?.id).toBe('checkout')
  })

  it('声明 checkout：F1 占用分支主动作回落 switch（点击给跳转）', () => {
    const row = refRow(refEntry({ name: 'dev' }), OWNER)
    const c = ctx({ checkout: vi.fn(), ownerOf: (n) => (n === 'dev' ? OWNER : null) })
    expect(primaryBranchAction(row, c)?.id).toBe('switch')
  })

  it('声明 checkout：当前 HEAD 主动作 = null', () => {
    expect(primaryBranchAction(refRow(refEntry({ name: 'main', isHead: true })), ctx({ checkout: vi.fn() }))).toBeNull()
  })

  it('声明 checkout：tag 主动作 = new-workspace（无原地检出也不误跳）', () => {
    const tag = refRow(refEntry({ name: 'v1', category: 'tag', fullName: 'refs/tags/v1' }))
    expect(primaryBranchAction(tag, ctx({ checkout: vi.fn() }))?.id).toBe('new-workspace')
  })

  it('不声明 checkout：沿用 branchActionForRef（无 owner → new-workspace）', () => {
    expect(
      primaryBranchAction(refRow(refEntry({ name: 'fresh' })), ctx({ ownerOf: () => null }))?.id
    ).toBe('new-workspace')
  })

  it('声明 checkout：remote 行主动作 = checkout（新语义相对旧语义最大的行为差异）', () => {
    const remote = refRow(
      refEntry({ name: 'origin/dev', category: 'remote', remote: 'origin', fullName: 'refs/remotes/origin/dev' })
    )
    expect(primaryBranchAction(remote, ctx({ checkout: vi.fn() }))?.id).toBe('checkout')
  })
})

describe('增量3 非破坏动作（new-branch / show-diff / set-upstream）', () => {
  const remoteEntry = refEntry({ name: 'origin/dev', category: 'remote', remote: 'origin', fullName: 'refs/remotes/origin/dev' })
  const tagEntry = refEntry({ name: 'v1', category: 'tag', fullName: 'refs/tags/v1' })

  it('new-branch：local/remote/tag 都可见；run 交 ref 给 ctx.newBranch', () => {
    const nb = vi.fn()
    const c = ctx({ newBranch: nb })
    for (const row of [refRow(refEntry({ name: 'dev' })), refRow(remoteEntry), refRow(tagEntry)]) {
      expect(findAction('new-branch').visible(row, c)).toBe(true)
    }
    const ref = refEntry({ name: 'dev' })
    findAction('new-branch').run(refRow(ref), c)
    expect(nb).toHaveBeenCalledWith(ref)
  })

  it('new-branch：宿主不声明 newBranch → 不可见（宿主裁剪）', () => {
    expect(findAction('new-branch').visible(refRow(refEntry({ name: 'dev' })), ctx({ newBranch: undefined }))).toBe(false)
  })

  it('show-diff：非当前 local/remote 可见；HEAD 行与 tag 不可见', () => {
    const c = ctx({ showDiff: vi.fn() })
    expect(findAction('show-diff').visible(refRow(refEntry({ name: 'dev' })), c)).toBe(true)
    expect(findAction('show-diff').visible(refRow(remoteEntry), c)).toBe(true)
    expect(findAction('show-diff').visible(refRow(refEntry({ name: 'main', isHead: true })), c)).toBe(false)
    expect(findAction('show-diff').visible(refRow(tagEntry), c)).toBe(false)
  })

  it('set-upstream：仅 local 可见；submenu 由 remoteRefCandidates 生成，item.run 调 ctx.setUpstream(name, candidate)', () => {
    const su = vi.fn()
    const candidates = [remoteEntry]
    const c = ctx({ setUpstream: su, remoteRefCandidates: () => candidates })
    const row = refRow(refEntry({ name: 'dev' }))
    expect(findAction('set-upstream').visible(row, c)).toBe(true)
    const items = findAction('set-upstream').submenu?.(row, c) ?? []
    expect(items.map((i) => i.id)).toEqual(['origin/dev'])
    items[0]?.run()
    expect(su).toHaveBeenCalledWith('dev', 'origin/dev')
  })

  it('set-upstream：remote/tag 不可见；无候选上游不可见；宿主不声明不可见', () => {
    const c = ctx({ setUpstream: vi.fn(), remoteRefCandidates: () => [] })
    expect(findAction('set-upstream').visible(refRow(remoteEntry), c)).toBe(false)
    expect(findAction('set-upstream').visible(refRow(tagEntry), c)).toBe(false)
    expect(findAction('set-upstream').visible(refRow(refEntry({ name: 'dev' })), c)).toBe(false)
    expect(findAction('set-upstream').visible(refRow(refEntry({ name: 'dev' })), ctx())).toBe(false)
  })
})

describe('增量4 破坏性动作（rename / delete）+ §8.2 in-flight 置灰', () => {
  const remoteEntry = refEntry({ name: 'origin/dev', category: 'remote', remote: 'origin', fullName: 'refs/remotes/origin/dev' })
  const tagEntry = refEntry({ name: 'v1', category: 'tag', fullName: 'refs/tags/v1' })

  it('rename：仅 local 非 HEAD 可见；宿主不声明不可见；run 交 ref', () => {
    const rn = vi.fn()
    const c = ctx({ rename: rn })
    expect(findAction('rename').visible(refRow(refEntry({ name: 'dev' })), c)).toBe(true)
    expect(findAction('rename').visible(refRow(remoteEntry), c)).toBe(false)
    expect(findAction('rename').visible(refRow(tagEntry), c)).toBe(false)
    expect(findAction('rename').visible(refRow(refEntry({ name: 'main', isHead: true })), c)).toBe(false)
    expect(findAction('rename').visible(refRow(refEntry({ name: 'dev' })), ctx())).toBe(false)
    const ref = refEntry({ name: 'dev' })
    findAction('rename').run(refRow(ref), c)
    expect(rn).toHaveBeenCalledWith(ref)
  })

  it('delete：仅 local 非 HEAD 可见；宿主不声明不可见；run 交 ref', () => {
    const del = vi.fn()
    const c = ctx({ requestDelete: del })
    expect(findAction('delete').visible(refRow(refEntry({ name: 'dev' })), c)).toBe(true)
    expect(findAction('delete').visible(refRow(remoteEntry), c)).toBe(false)
    expect(findAction('delete').visible(refRow(refEntry({ name: 'main', isHead: true })), c)).toBe(false)
    expect(findAction('delete').visible(refRow(refEntry({ name: 'dev' })), ctx())).toBe(false)
    findAction('delete').run(refRow(refEntry({ name: 'dev' })), c)
    expect(del).toHaveBeenCalled()
  })

  it('rename/delete：被别的 worktree 检出（checkedOutBy 非 null）→ 不可点', () => {
    const c = ctx({ rename: vi.fn(), requestDelete: vi.fn() })
    const owner = { worktreeId: 'w2', path: '/x', displayName: 'w2' } as unknown as BranchWorkspaceOwner
    expect(findAction('rename').enabled?.(refRow(refEntry({ name: 'dev' }), owner), c)).toBe(false)
    expect(findAction('delete').enabled?.(refRow(refEntry({ name: 'dev' }), owner), c)).toBe(false)
    expect(findAction('rename').enabled?.(refRow(refEntry({ name: 'dev' })), c)).toBe(true)
  })

  it('§8.2：opInFlight 时 checkout / rename / delete 全部置灰', () => {
    const c = ctx({ checkout: vi.fn(), rename: vi.fn(), requestDelete: vi.fn(), newBranch: vi.fn(), opInFlight: true })
    const row = refRow(refEntry({ name: 'dev' }))
    expect(findAction('checkout').enabled?.(row, c)).toBe(false)
    expect(findAction('rename').enabled?.(row, c)).toBe(false)
    expect(findAction('delete').enabled?.(row, c)).toBe(false)
    expect(findAction('new-branch').enabled?.(row, c)).toBe(false)
    // 反证：无 in-flight 时四者可用（判别力）。
    const idle = ctx({ checkout: vi.fn(), rename: vi.fn(), requestDelete: vi.fn(), newBranch: vi.fn() })
    expect(findAction('checkout').enabled?.(row, idle)).toBe(true)
    expect(findAction('rename').enabled?.(row, idle)).toBe(true)
    expect(findAction('delete').enabled?.(row, idle)).toBe(true)
    expect(findAction('new-branch').enabled?.(row, idle)).toBe(true)
  })

  it('§8.2：opInFlight 时 set-upstream 子项置灰', () => {
    const row = refRow(refEntry({ name: 'dev' }))
    const busy = ctx({ setUpstream: vi.fn(), remoteRefCandidates: () => [refEntry({ name: 'origin/dev', category: 'remote', remote: 'origin', fullName: 'refs/remotes/origin/dev' })], opInFlight: true })
    expect((findAction('set-upstream').submenu?.(row, busy) ?? [])[0]?.enabled).toBe(false)
  })
})
