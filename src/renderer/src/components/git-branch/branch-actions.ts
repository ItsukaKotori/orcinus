import { translate } from '@/i18n/i18n'
import type { GitRefEntry } from '../../../../shared/git-ref-types'
import type { BranchTreeRow, BranchWorkspaceOwner } from './branch-tree-model'

export type BranchActionId =
  | 'checkout'
  | 'switch'
  | 'new-workspace'
  | 'new-branch'
  | 'update'
  | 'push'
  | 'rename'
  | 'delete'
  | 'set-upstream'
  | 'show-diff'
  | 'view-in-log'

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
  /** 以该 ref 的 oid 为 start-point 新建分支（弹名字输入框）。宿主 A 注入。 */
  newBranch?: (ref: GitRefEntry) => void
  /** 把工作区中该分支与某上游对齐显示（只读）。宿主 A 注入。 */
  showDiff?: (ref: GitRefEntry) => void
  /** 给本地分支设 upstream（从子菜单选中项）。宿主 A 注入。 */
  setUpstream?: (branch: string, upstream: string) => void
  /**
   * 子菜单候选上游：返回可作 `branch` upstream 的 `refs/remotes/*` 条目（同名优先已排序）。
   * 与 `setUpstream` 一同声明时 set-upstream 才可见；返回空数组则该动作不可见。
   */
  remoteRefCandidates?: (branch: string) => GitRefEntry[]
  /** Rename（仅未检出 local）；宿主不声明 → 菜单不出现。run 只开框，写入在宿主。 */
  rename?: (ref: GitRefEntry) => void
  /** Update（§7.4：仅被某 workspace 检出**且**有 upstream 的 local 分支）；宿主复用 store 远端动作。 */
  update?: (ref: GitRefEntry) => void
  /** Push…（§7.4：仅被某 workspace 检出的 local 分支；无 upstream 则 publish）。 */
  push?: (ref: GitRefEntry) => void
  /** Delete（仅未检出 local）；run 只触发删除流程（含 -d→-D 二次确认）。 */
  requestDelete?: (ref: GitRefEntry) => void
  /** §8.2：有写操作 in-flight 时，checkout / set-upstream / rename / delete 菜单项置灰（防并发）。 */
  opInFlight?: boolean
  /** §8.2：有远端操作进行中（store `isRemoteOperationActive`）时，全部写动作置灰；与 opInFlight 正交。 */
  remoteOpActive?: boolean
}

/** 子菜单项（Set Upstream To… 用）。 */
export type BranchSubmenuItem = {
  id: string
  label: () => string
  enabled: boolean
  run: () => void
}

export type BranchAction = {
  id: BranchActionId
  /** 惰性求值：`no-top-level-translate.test.ts` 是 AST 门禁。 */
  label: () => string
  visible: (row: BranchTreeRow, ctx: BranchActionContext) => boolean
  /** visible 但不可点时用；缺省 = 恒可用。用于 F1：被其它 worktree 检出 → 置灰。 */
  enabled?: (row: BranchTreeRow, ctx: BranchActionContext) => boolean
  /** 有子项的动作（Set Upstream To…）：菜单渲染展开区而非叶子。有此字段时顶层 run 不被调用。 */
  submenu?: (row: BranchTreeRow, ctx: BranchActionContext) => BranchSubmenuItem[]
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
    enabled: (row, ctx) => row.kind === 'ref' && row.checkedOutBy === null && !ctx.opInFlight && !ctx.remoteOpActive,
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
    id: 'new-branch',
    label: () => translate('auto.components.git-branch.newBranchFrom', 'New Branch from…'),
    // local/remote/tag 都可作为 start-point 新建分支（§7.4）；仅声明 newBranch 的宿主出现。
    visible: (row, ctx) => ctx.newBranch !== undefined && refOf(row) !== null,
    // §8.2：与其余写动作一致，in-flight / 远端进行中时置灰（防叠开多个写框）。
    enabled: (_row, ctx) => !ctx.opInFlight && !ctx.remoteOpActive,
    run: (row, ctx) => {
      const ref = refOf(row)
      if (ref) ctx.newBranch?.(ref)
    },
  },
  {
    id: 'show-diff',
    label: () => translate('auto.components.git-branch.showDiff', 'Show Diff with Working Tree'),
    // 非当前分支的 local/remote 才有对比对象（tag 不列入，§7.4）。
    visible: (row, ctx) => {
      const ref = refOf(row)
      return ctx.showDiff !== undefined && ref !== null && ref.category !== 'tag' && !ref.isHead
    },
    run: (row, ctx) => {
      const ref = refOf(row)
      if (ref) ctx.showDiff?.(ref)
    },
  },
  {
    id: 'set-upstream',
    label: () => translate('auto.components.git-branch.setUpstreamTo', 'Set Upstream To…'),
    // 仅本地分支；需宿主声明 setUpstream + remoteRefCandidates，且至少有一个候选上游。
    visible: (row, ctx) => {
      const ref = refOf(row)
      return (
        ref !== null &&
        ref.category === 'local' &&
        ctx.setUpstream !== undefined &&
        ctx.remoteRefCandidates !== undefined &&
        ctx.remoteRefCandidates(ref.name).length > 0
      )
    },
    submenu: (row, ctx) => {
      const ref = refOf(row)
      if (!ref || !ctx.setUpstream || !ctx.remoteRefCandidates) return []
      return ctx.remoteRefCandidates(ref.name).map((candidate) => ({
        id: candidate.name,
        label: () => candidate.name,
        enabled: !ctx.opInFlight && !ctx.remoteOpActive,
        run: () => ctx.setUpstream?.(ref.name, candidate.name),
      }))
    },
    // 子项自带 run；顶层 run 不被调用（submenu 存在时菜单渲染展开区）。
    run: () => {},
  },
  {
    id: 'update',
    label: () => translate('auto.components.git-branch.update', 'Update'),
    // §7.4：Update 仅对**被某 workspace 检出且有 upstream** 的 local 分支（当前分支也算“已检出”）。remote/tag/未检出不现。
    visible: (row, ctx) => {
      const ref = refOf(row)
      return (
        ctx.update !== undefined &&
        ref !== null &&
        ref.category === 'local' &&
        !!ref.upstream &&
        row.kind === 'ref' &&
        row.checkedOutBy !== null
      )
    },
    enabled: (_row, ctx) => !ctx.opInFlight && !ctx.remoteOpActive,
    run: (row, ctx) => {
      const ref = refOf(row)
      if (ref) ctx.update?.(ref)
    },
  },
  {
    id: 'push',
    label: () => translate('auto.components.git-branch.push', 'Push…'),
    // §7.4：Push 仅对**被某 workspace 检出**的 local 分支（无 upstream 时宿主以 publish 首次推送）。
    visible: (row, ctx) => {
      const ref = refOf(row)
      return (
        ctx.push !== undefined &&
        ref !== null &&
        ref.category === 'local' &&
        row.kind === 'ref' &&
        row.checkedOutBy !== null
      )
    },
    enabled: (_row, ctx) => !ctx.opInFlight && !ctx.remoteOpActive,
    run: (row, ctx) => {
      const ref = refOf(row)
      if (ref) ctx.push?.(ref)
    },
  },
  {
    id: 'rename',
    label: () => translate('auto.components.git-branch.rename', 'Rename…'),
    // §7.4：Rename 仅 local、非当前分支；宿主声明 rename 才出现。
    visible: (row, ctx) => {
      const ref = refOf(row)
      return ctx.rename !== undefined && ref !== null && ref.category === 'local' && !ref.isHead
    },
    // 仅未检出分支可改名（§7.4 / 决策 5）；in-flight 置灰。
    enabled: (row, ctx) => row.kind === 'ref' && row.checkedOutBy === null && !ctx.opInFlight && !ctx.remoteOpActive,
    run: (row, ctx) => {
      const ref = refOf(row)
      if (ref) ctx.rename?.(ref)
    },
  },
  {
    id: 'delete',
    label: () => translate('auto.components.git-branch.delete', 'Delete'),
    visible: (row, ctx) => {
      const ref = refOf(row)
      return ctx.requestDelete !== undefined && ref !== null && ref.category === 'local' && !ref.isHead
    },
    enabled: (row, ctx) => row.kind === 'ref' && row.checkedOutBy === null && !ctx.opInFlight && !ctx.remoteOpActive,
    run: (row, ctx) => {
      const ref = refOf(row)
      if (ref) ctx.requestDelete?.(ref)
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
