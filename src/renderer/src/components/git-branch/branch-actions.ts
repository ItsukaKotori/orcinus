import { translate } from '@/i18n/i18n'
import type { GitRefEntry } from '../../../../shared/git-ref-types'
import type { BranchTreeRow, BranchWorkspaceOwner } from './branch-tree-model'

export type BranchActionId = 'checkout' | 'switch' | 'new-workspace' | 'view-in-log'

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
  /**
   * 在当前工作区原地检出该 ref。仅宿主 A（Source Control）注入。声明它的宿主把
   * Checkout 作为主动作；不提供的宿主（Git Log）沿用 switch / new-workspace。
   */
  checkout?: (ref: GitRefEntry) => void
}

export type BranchAction = {
  id: BranchActionId
  /** 惰性求值：`no-top-level-translate.test.ts` 是 AST 门禁。 */
  label: () => string
  visible: (row: BranchTreeRow, ctx: BranchActionContext) => boolean
  /** visible 但不可点时用；缺省 = 恒可用。用于 F1：被其它 worktree 检出 → 置灰。 */
  enabled?: (row: BranchTreeRow, ctx: BranchActionContext) => boolean
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
    // 宿主 A 的主动作：原地检出。放最前，供菜单顺序体现「主动作优先」。
    id: 'checkout',
    label: () => translate('auto.components.git-branch.checkout', 'Checkout'),
    visible: (row, ctx) => {
      const ref = refOf(row)
      // 仅声明 checkout 的宿主（宿主 A）出现；tag 不原地检出；当前 HEAD 不检出自己。
      return ctx.checkout !== undefined && ref !== null && ref.category !== 'tag' && !ref.isHead
    },
    // F1 置灰契约：
    //  - 只按**本地分支行**的 checkedOutBy 判占用（branch-tree-model 对非 local 行恒置 null）。
    //  - remote 行（如 origin/dev）恒可见可用：其占用与否由后端 resolve 成同名本地短名**后**复核，
    //    命中则回 checked-out-elsewhere 拒绝，宿主 A（Task 5）据此跳转；故前端不为 remote 置灰。
    //  - checkedOutBy 源自含当前 worktree 的 owner 映射；正常数据下当前分支行 isHead=true 已令
    //    checkout 不可见，走不到这里；仅元数据陈旧时才会出现“自己占用→置灰+回落跳转”的连带面。
    enabled: (row) => row.kind === 'ref' && row.checkedOutBy === null,
    run: (row, ctx) => {
      const ref = refOf(row)
      if (ref) ctx.checkout?.(ref)
    },
  },
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
    if (ctx.checkout) {
      const checkout = BRANCH_ACTIONS.find((a) => a.id === 'checkout')!
      if (checkout.visible(row, ctx)) {
        if (!checkout.enabled || checkout.enabled(row, ctx)) return checkout
        // F1：checkout 可见但被占用 → 优先 switch 跳转；owner 数据缺失时再回落 owner 判定，绝不空动作
        const sw = BRANCH_ACTIONS.find((a) => a.id === 'switch')!
        if (sw.visible(row, ctx)) return sw
        return BRANCH_ACTIONS.find((a) => a.id === branchActionForRef(row.ref, ctx.ownerOf)) ?? null
      }
      // checkout 不适用此行：当前 HEAD 无原地动作；tag 等仍走原有 owner 判定（→ new-workspace）
      if (row.ref.isHead) return null
      return BRANCH_ACTIONS.find((a) => a.id === branchActionForRef(row.ref, ctx.ownerOf)) ?? null
    }
    return BRANCH_ACTIONS.find((a) => a.id === branchActionForRef(row.ref, ctx.ownerOf)) ?? null
  }
  if (row.kind === 'synthetic' && ctx.viewInGitLog) {
    return BRANCH_ACTIONS.find((a) => a.id === 'view-in-log') ?? null
  }
  return null
}
