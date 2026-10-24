import type { GitRefEntry } from '../../../../shared/git-ref-types'
import type { Worktree } from '../../../../shared/worktree/types'

/** 检出某分支的 ade workspace。前端从 store 算，advisory（spec §6.4）。 */
export type BranchWorkspaceOwner = {
  worktreeId: string
  displayName: string
  path: string
}

export type BranchTreeRow =
  | {
      kind: 'group'
      id: string
      /** translate key；label 惰性求值（no-top-level-translate 门禁）。 */
      labelKey: string
      labelFallback: string
      depth: number
      expandable: true
      expanded: boolean
      /** 该组在过滤后的条目数，用于组尾计数。 */
      childCount: number
    }
  | {
      kind: 'ref'
      id: string
      depth: number
      ref: GitRefEntry
      /** 当前分支的 upstream（仅 remote 条目可能为真）。 */
      isUpstreamOfHead: boolean
      checkedOutBy: BranchWorkspaceOwner | null
    }
  | {
      kind: 'synthetic'
      id: string
      depth: number
      labelKey: string
      labelFallback: string
      value: 'all-branches' | 'head-current'
    }

export const GROUP_LOCAL = 'group:local'
export const GROUP_REMOTE = 'group:remote'
export const GROUP_TAGS = 'group:tags'
export const ROW_HEAD = 'row:head-current'
export const ROW_ALL = 'row:all-branches'

export const GROUP_LABELS: Record<string, { key: string; fallback: string }> = {
  [GROUP_LOCAL]: { key: 'auto.components.git-branch.groupLocal', fallback: 'Local' },
  [GROUP_REMOTE]: { key: 'auto.components.git-branch.groupRemote', fallback: 'Remote' },
  [GROUP_TAGS]: { key: 'auto.components.git-branch.groupTags', fallback: 'Tags' },
}

export const SYNTHETIC_LABELS: Record<
  'all-branches' | 'head-current',
  { labelKey: string; labelFallback: string }
> = {
  'all-branches': {
    labelKey: 'auto.components.git-branch.allBranches',
    labelFallback: 'All Branches',
  },
  'head-current': {
    labelKey: 'auto.components.git-branch.headCurrent',
    labelFallback: 'HEAD (Current Branch)',
  },
}

export type BuildBranchTreeInput = {
  refs: readonly GitRefEntry[]
  /** 折叠中的组 id；未出现的组视为展开。 */
  collapsed?: ReadonlySet<string>
  /** 当前分支短名（`main`），null / 缺省 = detached 或未知。 */
  headBranch?: string | null
  /** 分支短名 → 检出它的 workspace。advisory，见 spec §6.4。 */
  checkedOutByBranch?: ReadonlyMap<string, BranchWorkspaceOwner>
  /** 宿主 B（Git Log）注入 `All Branches` 行；宿主 A 不注入。 */
  includeAllBranchesRow?: boolean
}

type Bucket = {
  id: string
  label: { key: string; fallback: string }
  refs: GitRefEntry[]
}

/**
 * Remote 条目比的是**去掉 `<remote>/` 前缀后**的名字，否则
 * `origin/a` 会排到 `origin/b` 之后（前缀主导排序是反直觉的）。
 */
function compareName(a: GitRefEntry, b: GitRefEntry): number {
  const an = a.category === 'remote' ? a.name.replace(/^[^/]+\//, '') : a.name
  const bn = b.category === 'remote' ? b.name.replace(/^[^/]+\//, '') : b.name
  return an.localeCompare(bn)
}

/** 组序固定：Local → Remote → Tags；空组不出行头。 */
function bucketize(refs: readonly GitRefEntry[]): Bucket[] {
  const local = refs.filter((r) => r.category === 'local')
  const remote = refs.filter((r) => r.category === 'remote')
  const tags = refs.filter((r) => r.category === 'tag')
  const buckets: Bucket[] = []
  if (local.length) buckets.push({ id: GROUP_LOCAL, label: GROUP_LABELS[GROUP_LOCAL]!, refs: local })
  if (remote.length) {
    buckets.push({ id: GROUP_REMOTE, label: GROUP_LABELS[GROUP_REMOTE]!, refs: remote })
  }
  if (tags.length) buckets.push({ id: GROUP_TAGS, label: GROUP_LABELS[GROUP_TAGS]!, refs: tags })
  return buckets
}

/** Remote 组按 remote 名二级分组；remote 名升序。 */
function byRemote(refs: GitRefEntry[]): Array<[string, GitRefEntry[]]> {
  const out = new Map<string, GitRefEntry[]>()
  for (const r of refs) {
    const key = r.remote ?? r.name.split('/')[0] ?? ''
    let list = out.get(key)
    if (!list) {
      list = []
      out.set(key, list)
    }
    list.push(r)
  }
  return [...out.entries()].sort(([a], [b]) => a.localeCompare(b))
}

/** 当前分支的 upstream 短名（`origin/main`），用于 `★` 标记。 */
function headUpstreamName(
  refs: readonly GitRefEntry[],
  headBranch: string | null | undefined
): string | null {
  if (!headBranch) return null
  return refs.find((r) => r.category === 'local' && r.name === headBranch)?.upstream ?? null
}

/**
 * 分支短名 → 检出它的 workspace。
 *
 * 无 branch 的条目（bare / detached / 读不到 refname 的 worktree）跳过；
 * 同一分支被多个 worktree 检出时取第一个 —— git 本身禁止这种状态，
 * 出现即说明元数据已漂移，此处不做仲裁。
 */
export function ownerByBranch(
  worktrees: readonly Worktree[]
): Map<string, BranchWorkspaceOwner> {
  const out = new Map<string, BranchWorkspaceOwner>()
  for (const wt of worktrees) {
    if (!wt.branch || out.has(wt.branch)) continue
    out.set(wt.branch, {
      worktreeId: wt.id,
      displayName: wt.displayName,
      path: wt.path,
    })
  }
  return out
}

/**
 * refs → 可见行列表（折叠态已应用，可直接渲染与 ↑↓ 导航）。
 *
 * 刻意返回扁平行而非嵌套树：键盘导航与测试定位都要遍历可见行，
 * 扁平结构让「折叠即不可见」成为单一事实源。
 */
export function buildBranchTreeRows(input: BuildBranchTreeInput): BranchTreeRow[] {
  const {
    refs,
    collapsed = new Set<string>(),
    headBranch = null,
    checkedOutByBranch = new Map<string, BranchWorkspaceOwner>(),
    includeAllBranchesRow = false,
  } = input

  const head = refs.find((r) => r.isHead) ?? null
  const upstream = headUpstreamName(refs, headBranch)
  const rows: BranchTreeRow[] = []

  if (includeAllBranchesRow) {
    rows.push({ kind: 'synthetic', id: ROW_ALL, depth: 0, ...SYNTHETIC_LABELS['all-branches'], value: 'all-branches' })
  }
  if (head) {
    rows.push({ kind: 'synthetic', id: ROW_HEAD, depth: 0, ...SYNTHETIC_LABELS['head-current'], value: 'head-current' })
  }

  // HEAD 行已代表当前分支，Local 组里不再重复出它；detached（head 为 null）时
  // 没有任何条目代表当前分支，所以全部留下。
  const rest = refs.filter((r) => r !== head)

  for (const bucket of bucketize(rest)) {
    const expanded = !collapsed.has(bucket.id)
    rows.push({
      kind: 'group',
      id: bucket.id,
      labelKey: bucket.label.key,
      labelFallback: bucket.label.fallback,
      depth: 0,
      expandable: true,
      expanded,
      childCount: bucket.refs.length,
    })
    if (!expanded) continue

    if (bucket.id !== GROUP_REMOTE) {
      for (const r of [...bucket.refs].sort(compareName)) {
        rows.push(refRow(r, 1, upstream, checkedOutByBranch))
      }
      continue
    }

    // Remote：一级组头 + 每个 remote 一个二级组头。折叠一级即抹掉全部二级内容。
    for (const [remote, remoteRefs] of byRemote(bucket.refs)) {
      const subId = `${GROUP_REMOTE}/${remote}`
      const subExpanded = !collapsed.has(subId)
      rows.push({
        kind: 'group',
        id: subId,
        // 二级组头就是 remote 名本身，不是可翻译文案 —— labelKey 只作 id 用。
        labelKey: subId,
        labelFallback: remote,
        depth: 1,
        expandable: true,
        expanded: subExpanded,
        childCount: remoteRefs.length,
      })
      if (!subExpanded) continue
      for (const r of [...remoteRefs].sort(compareName)) {
        rows.push(refRow(r, 2, upstream, checkedOutByBranch))
      }
    }
  }

  return rows
}

function refRow(
  r: GitRefEntry,
  depth: number,
  headUpstream: string | null,
  owners: ReadonlyMap<string, BranchWorkspaceOwner>
): BranchTreeRow {
  return {
    kind: 'ref',
    id: `ref:${r.fullName}`,
    depth,
    ref: r,
    isUpstreamOfHead: headUpstream !== null && r.name === headUpstream,
    // 只有本地分支可能被 workspace 检出（远程分支/标签的 owner 语义不成立）。
    checkedOutBy: r.category === 'local' ? owners.get(r.name) ?? null : null,
  }
}
