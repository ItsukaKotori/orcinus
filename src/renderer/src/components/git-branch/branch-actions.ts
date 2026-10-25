import { translate } from '@/i18n/i18n'
import type { GitRefEntry } from '../../../../shared/git-ref-types'
import type { BranchTreeRow, BranchWorkspaceOwner } from './branch-tree-model'

export type BranchActionId = 'switch' | 'new-workspace' | 'view-in-log'

export type BranchActionContext = {
  /** 当前分支短名，null = detached。 */
  headBranch: string | null
  /** advisory：分支短名 → 检出它的 workspace（spec §6.4）。 */
  ownerOf: (branchName: string) => BranchWorkspaceOwner | null
  activateWorkspace: (owner: BranchWorkspaceOwner) => void
  newWorkspaceFrom: (ref: GitRefEntry) => void
  /**
   * 仅 Git Log 宿主提供。**宿主裁剪靠「不传这个回调」而不是额外的 opts** ——
   * 少一个平行参数，就少一处两宿主会各自漂移的地方。
   */
  viewInGitLog?: (value: 'all-branches' | 'head-current' | GitRefEntry) => void
}

export type BranchAction = {
  id: BranchActionId
  /** 惰性求值：`no-top-level-translate.test.ts` 是 AST 门禁。 */
  label: () => string
  visible: (row: BranchTreeRow, ctx: BranchActionContext) => boolean
  run: (row: BranchTreeRow, ctx: BranchActionContext) => void
}

/**
 * ref 行 → 该走 switch 还是 new-workspace 的**唯一**判定入口。
 * 动作表与宿主（决定 Enter/单击做什么）共用它，避免两处 owner 判断漂移。
 */
export function branchActionForRef(
  ref: GitRefEntry,
  ownerOf: (branchName: string) => BranchWorkspaceOwner | null
): BranchActionId {
  // 只有本地分支可能被 workspace 检出并直接跳转；remote/tag 一律走新建 workspace。
  return ref.category === 'local' && ownerOf(ref.name) ? 'switch' : 'new-workspace'
}

function refOf(row: BranchTreeRow): GitRefEntry | null {
  return row.kind === 'ref' ? row.ref : null
}

export const BRANCH_ACTIONS: readonly BranchAction[] = [
  {
    id: 'switch',
    label: () => translate('auto.components.git-branch.switchTo', 'Switch to this branch'),
    visible: (row, ctx) => {
      const ref = refOf(row)
      // 复用 branchActionForRef 而不是重抄一遍谓词：它是 owner 判定的唯一入口。
      // 重拄会让增量 2 的菜单出现「列了 Switch、点下去走 New Workspace」的缝隙
      // （例如宿主把 ownerOf 写成返回 undefined 而非 null）。
      return ref !== null && branchActionForRef(ref, ctx.ownerOf) === 'switch'
    },
    run: (row, ctx) => {
      const ref = refOf(row)
      const owner = ref ? ctx.ownerOf(ref.name) : null
      // 点击与执行之间 owner 可能已消失（workspace 被关）。静默不做事，
      // 比在一个纯函数里为 advisory 数据抛异常好 —— 后者会掀掉整个 popover。
      if (owner) ctx.activateWorkspace(owner)
    },
  },
  {
    id: 'new-workspace',
    label: () =>
      translate('auto.components.git-branch.newWorkspaceFrom', 'New Workspace from Branch…'),
    visible: (row) => refOf(row) !== null,
    run: (row, ctx) => {
      const ref = refOf(row)
      if (ref) ctx.newWorkspaceFrom(ref)
    },
  },
  {
    id: 'view-in-log',
    label: () => translate('auto.components.git-branch.viewInGitLog', 'View in Git Log'),
    visible: (row, ctx) =>
      ctx.viewInGitLog !== undefined && (row.kind === 'ref' || row.kind === 'synthetic'),
    run: (row, ctx) => {
      if (!ctx.viewInGitLog) return
      if (row.kind === 'ref') ctx.viewInGitLog(row.ref)
      else if (row.kind === 'synthetic') ctx.viewInGitLog(row.value)
    },
  },
]

/**
 * Enter / 单击的动作。刻意**只**覆盖 ref 行与（Git Log 宿主的）合成人物：
 * Source Control 里点 `HEAD (Current Branch)` 没有可执行的动作，返回 null 让宿主
 * 吞掉点击，而不是执行一个看起来像成功的空动作。
 */
export function primaryBranchAction(
  row: BranchTreeRow,
  ctx: BranchActionContext
): BranchAction | null {
  if (row.kind === 'ref') {
    return BRANCH_ACTIONS.find((a) => a.id === branchActionForRef(row.ref, ctx.ownerOf)) ?? null
  }
  if (row.kind === 'synthetic' && ctx.viewInGitLog) {
    return BRANCH_ACTIONS.find((a) => a.id === 'view-in-log') ?? null
  }
  return null
}
