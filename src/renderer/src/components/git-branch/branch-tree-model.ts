import { branchName } from '@/lib/git-utils'
import type { GitRefEntry } from '../../../../shared/git-ref-types'
import type { ExecutionHostId } from '../../../../shared/execution-host'
import type { Worktree } from '../../../../shared/worktree/types'

/** 检出某分支的 ade workspace。前端从 store 算，advisory（spec §6.4）。 */
export type BranchWorkspaceOwner = {
  worktreeId: string
  displayName: string
  path: string
  /**
   * 宿主 id。激活一个跨宿主 workspace 时必须把它传回去 ——
   * `setActiveWorktree` 把第二参缺省当作 `null`（不从 worktree identity 推导），
   * 漏传会把 tab 打戳成 local 宿主。见 `store/slices/worktrees/session/set-active-worktree.ts`。
   */
  hostId?: ExecutionHostId
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
  /**
   * 当前分支短名（`main`），null / 缺省 = detached 或未知。
   *
   * 它同时决定两件事：`head-current` 合成行是否出现，以及 `★` 算谁的 upstream。
   * 用 `headBranch`（而非 refs 里的 isHead 项）做前者，是为了让搜索过滤抹掉
   * `main` 行时，`HEAD (Current Branch)` 这条**导航行**仍然在顶 ——
   * 否则用户在过滤态下无法跳回「当前分支范围」。
   */
  headBranch?: string | null
  /** 分支短名 → 检出它的 workspace。advisory，见 spec §6.4。 */
  checkedOutByBranch?: ReadonlyMap<string, BranchWorkspaceOwner>
  /** 宿主 B（Git Log）注入 `All Branches` 行；宿主 A 不注入。 */
  includeAllBranchesRow?: boolean
  /**
   * 当前分支的 upstream 短名，用于 `★`。
   *
   * 宿主应从**未过滤**的完整 ref 列表算出它再传进来：否则搜索抹掉 upstream
   * 那一条时 ★ 会跟着消失，而同一行的 `HEAD (Current Branch)` 导航行却是
   * 刻意设计成过滤态存活的 —— 两者口径不自洽。
   * 缺省（undefined）时回退到在 `refs` 里现算，供单测与简单宿主使用。
   */
  headUpstream?: string | null
}

type Bucket = {
  id: string
  label: { key: string; fallback: string }
  refs: GitRefEntry[]
}

/**
 * 名称**码点序**，刻意不用 `localeCompare()`。
 *
 * Why: spec §7.2 要的是「字典序」。无参 `localeCompare()` 走宿主 locale + ICU 的
 * 本地化权重，忽略/降权标点与大小写 —— 实测 `['ab','a-b','Ab','aB']` 在
 * localeCompare 下是 `[a-b, ab, aB, Ab]`，码点序下是 `[Ab, a-b, aB, ab]`。
 * 同一仓库在不同用户机器上排出不同顺，对这棵要靠 ↑↓ 稳定导航的树是可观察的歧义。
 *
 * 也不剥 `<remote>/` 前缀：remote 条目先按 `r.remote` 分组，组内所有名字共享
 * 同一前缀，剥掉公共前缀不改变任何相对次序（Task review 证实为 no-op）。
 */
function compareName(a: GitRefEntry, b: GitRefEntry): number {
  return a.name < b.name ? -1 : a.name > b.name ? 1 : 0
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

/** Remote 组按 remote 名码点序二级分组（与 compareName 同一理由）。 */
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
  return [...out.entries()].sort(([a], [b]) => (a < b ? -1 : a > b ? 1 : 0))
}

/**
 * 分支短名 → 检出它的 workspace。
 *
 * **`Worktree.branch` 生产形态是完整 ref**（`ade-git::porcelain` 原样存
 * `git worktree list` 的 `branch refs/heads/main`，`status.rs` 更是显式
 * `format!("refs/heads/{value}")`），而 `GitRefEntry.name` 是短名。
 * 所以这里必须先用 `branchName()` 剥前缀 —— 否则键是 `refs/heads/main`、
 * 查的是 `main`，永远 miss，▣ 徽标与 Switch 动作在生产环境全部失效。
 *
 * **调用方必须把 `worktrees` 限定到 ref 所属的那个 repo。**
 * `Worktree.branch` 剥完前缀只是短名，两个 repo 都有 `main` 是多仓用户的常态；
 * 把全仓 worktree 压进一个按短名索引的 Map 会让当前仓的 `main`
 * 显示（并跳转）到另一个仓的 workspace。
 *
 * 无 branch 的条目（bare / detached / 读不到 refname）跳过；
 * 同一 repo 内同分支多 worktree 取第一个 —— git 本身禁止这种状态，
 * 出现即元数据已漂移，此处不做仲裁。
 */
export function ownerByBranch(
  worktrees: readonly Worktree[]
): Map<string, BranchWorkspaceOwner> {
  const out = new Map<string, BranchWorkspaceOwner>()
  for (const wt of worktrees) {
    const short = wt.branch ? branchName(wt.branch) : ''
    if (!short || out.has(short)) continue
    out.set(short, {
      worktreeId: wt.id,
      displayName: wt.displayName,
      path: wt.path,
      hostId: wt.hostId,
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
    headUpstream,
  } = input

  // 空串不算「知道当前分支」：必须走 `||` 而不是 `??`，否则宿主传了个初值 `''`
  // 会把 isHead 回落压掉，导航行与 ★ 双双消失。
  const headName = headBranch?.trim() || refs.find((r) => r.isHead)?.name || null
  const upstream =
    headUpstream !== undefined
      ? headUpstream
      : (refs.find((r) => r.category === 'local' && r.name === headName)?.upstream ?? null)
  const rows: BranchTreeRow[] = []

  if (includeAllBranchesRow) {
    rows.push({ kind: 'synthetic', id: ROW_ALL, depth: 0, ...SYNTHETIC_LABELS['all-branches'], value: 'all-branches' })
  }
  if (headName) {
    rows.push({ kind: 'synthetic', id: ROW_HEAD, depth: 0, ...SYNTHETIC_LABELS['head-current'], value: 'head-current' })
  }

  // spec §7.1/§7.2：当前分支**既**在 Local 组内置顶并带 🏷，**也**有
  // `HEAD (Current Branch)` 合成行。两行语义不同且都需要：
  //  合成行 = 「当前分支范围」（Git Log 宿主：refs 不设限）；
  //  Local 里的 main 行 = 该 ref 本身（Git Log 宿主：refs=main；也可被右键）。
  // 把它们合二为一会让当前分支无法作为普通 ref 被选中。

  for (const bucket of bucketize(refs)) {
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
      const sorted = [...bucket.refs].sort(compareName)
      // spec §7.2「当前分支置顶」：码点序会把 `Main`/`cache` 之类排到前面，
      // 所以置顶必须显式做，不能指望 isHead 碰巧排在第一位。
      if (bucket.id === GROUP_LOCAL) {
        sorted.sort((a, b) => Number(b.isHead) - Number(a.isHead))
      }
      for (const r of sorted) {
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
