import { useCallback, useMemo, useRef, useState } from 'react'
import { useAppStore } from '@/store'
import { findWorktreeById } from '@/store/slices/worktree-helpers'
import { activateAndRevealWorktree } from '@/lib/worktree-activation'
import { branchName } from '@/lib/git-utils'
import { translate } from '@/i18n/i18n'
import { cn } from '@/lib/utils'
import { checkoutRuntimeGit } from '@/runtime/runtime-git-checkout-client'
import { createRuntimeGitBranch, setRuntimeGitBranchUpstream, renameRuntimeGitBranch, deleteRuntimeGitBranch, getRuntimeGitBranchCompare } from '@/runtime/runtime-git-client'
import type { RuntimeGitContext } from '@/runtime/runtime-git-client-context'
import { refreshGitStatusForWorktree } from '../right-sidebar/git-status-refresh'
import { toast } from 'sonner'
import type { GitRefEntry } from '../../../../shared/git-ref-types'
import { BranchRefTree } from './branch-ref-tree'
import { ownerByBranch } from './branch-tree-model'
import {
  BRANCH_ACTIONS,
  primaryBranchAction,
  type BranchActionContext,
  type BranchActionId,
} from './branch-actions'
import { BranchRowMenu } from './branch-menu'
import { CheckoutConfirmDialog, needsCheckoutConfirmation } from './checkout-confirm-dialog'
import { NewBranchDialog } from './new-branch-dialog'
import { RenameDialog } from './rename-dialog'
import { DeleteConfirmDialog } from './delete-confirm-dialog'
import { useBranchRefs } from './use-branch-refs'
import {
  persistBranchSectionCollapsed,
  readPersistedBranchSectionCollapsed,
} from './source-control-branch-section-view-state'

// 宿主 A 声明的可用动作集合：checkout 是主动作，switch 降级为次要项，恒保留 new-workspace。
// new-branch / rename / delete（增量3/4）+ set-upstream / show-diff（show-diff 于增量3b 接 `getRuntimeGitBranchCompare`）。
// **不含 view-in-log**（Git Log 专属）；宿主 B（Git Log）不注入 ctx.showDiff → show-diff 在 B 自然不可见。
const HOST_A_ALLOWED: ReadonlySet<BranchActionId> = new Set([
  'checkout',
  'switch',
  'new-workspace',
  'new-branch',
  'update',
  'push',
  'rename',
  'delete',
  'set-upstream',
  'show-diff',
])

// 稳定的空数组引用：脏改动缺失时避免每次渲染新建数组打乱 zustand 选择器一致性。
const EMPTY_ENTRIES: never[] = []

export type SourceControlBranchTreeSectionProps = {
  worktreeId: string
  /**
   * 面板内优先注入 foundation 的 `refreshActiveGitStatusAfterMutation`（按 repo owner 宿主正确路由
   * settings/connectionId）。缺省时回落到模块级 `refreshGitStatusForWorktree`（Git Log 等独立宿主用）。
   */
  refreshActiveGitStatusAfterMutation?: () => Promise<void>
  /** 面板传入 `model.activeRepoSettings`；缺省 null = 本地（getActiveRuntimeTarget(null)→local）。 */
  settings?: RuntimeGitContext['settings']
}

/**
 * Source Control 面板 header 之下的常驻分支树区块（增量 2C，spec §13.2 决策 1 / §13.6）。
 *
 * 取代 2B 的头部弹层：分支树常驻可见、默认展开、折叠态持久化到 localStorage，原地 Checkout 的
 * 动作装配（checkout/F1 置灰+跳转/脏+会话确认框/立即刷状态）从 `SourceControlBranchSwitcher` 平移进来。
 * 与弹层的关键差异：**区块不关** → 成功检出后必须 `invalidate()` 让树重新取数（弹层时代靠重开，此处无此步）。
 *
 * 复用 2B 的无业务语义树接缝（`BranchRefTree` + `renderRowMenu` + `BranchRowMenu`），宿主裁剪靠
 * 不声明 `viewInGitLog` + `HOST_A_ALLOWED`。**解析不到 worktree 时整块返回 null**（面板已守卫，此为兜底）。
 */
export function SourceControlBranchTreeSection({
  worktreeId,
  refreshActiveGitStatusAfterMutation,
  settings = null,
}: SourceControlBranchTreeSectionProps): React.JSX.Element | null {
  const [collapsed, setCollapsed] = useState(() => readPersistedBranchSectionCollapsed())
  const [pendingCheckout, setPendingCheckout] = useState<GitRefEntry | null>(null)
  const [newBranchTarget, setNewBranchTarget] = useState<GitRefEntry | null>(null)
  // 增量4：Rename 弹框目标 + Delete 强制二次确认（NotMergeable 后才带 unmergedCount 置入）。
  const [renameTarget, setRenameTarget] = useState<GitRefEntry | null>(null)
  const [deleteForce, setDeleteForce] = useState<{ ref: GitRefEntry; unmergedCount: number } | null>(null)
  // §8.2：ref 是权威互斥门（防并发写），state 只驱菜单置灰。两者在每个写 execute 同步。
  const [opInFlight, setOpInFlight] = useState(false)
  // 防并发写：checkout / new-branch / set-upstream 都是会改 ref 的写操作，连点会在
  // 同一 worktree 上叠两条 git 写、大概率撞 index.lock。共用一个 in-flight 门，进入置位、finally 复位。
  const writeInFlightRef = useRef(false)
  // Show Diff 只读但会起 git 子进程（diff/merge-base）；连点会叠加，单独一个只读门，不占写门。
  const showDiffBusyRef = useRef(false)

  const worktree = useAppStore((s) => findWorktreeById(s.worktreesByRepo, worktreeId))
  const openModal = useAppStore((s) => s.openModal)
  // 增量5：Update/Push 直接复用 store 现成远端动作（自带 beginRemoteOperation/endRemoteOperation try/finally
  // + 错误 toast + upstream 刷新），树侧不另造并发门。owner worktree 目标从 owner 投影取。
  const pullBranch = useAppStore((s) => s.pullBranch)
  const pushBranch = useAppStore((s) => s.pushBranch)
  // §8.2：远端进行中→全部写动作置灰。面板部分 store 桩下可能 undefined→按 falsy 处理（不置灰），安全。
  const remoteOpActive = useAppStore((s) => s.isRemoteOperationActive)
  // Show Diff 目标（只读）：把 compare 得到的该分支 entries 交给 openBranchAllDiffs 的 tab 快照。
  const openBranchAllDiffs = useAppStore((s) => s.openBranchAllDiffs)

  const repoId = worktree?.repoId ?? ''
  const repoWorktrees = useAppStore((s) => s.worktreesByRepo[repoId])

  // 确认框数据源（spec §13.7）：全部走可选链 —— 生产 store 恒有这些 slice，但区块会被
  // 面板测试以部分 store 桩渲染（SourceControl.*.test），不加 ? 会在 `undefined[worktreeId]` 上抛。
  const dirtyByWt = useAppStore((s) => s.gitStatusByWorktree?.[worktreeId])
  const terminalCount = useAppStore((s) => s.tabsByWorktree?.[worktreeId]?.length ?? 0)
  const agentCount = useAppStore(
    (s) => s.unifiedTabsByWorktree?.[worktreeId]?.filter((t) => t.contentType === 'agent-session').length ?? 0
  )
  const dirty = dirtyByWt ?? EMPTY_ENTRIES
  // sessionCount = 终端数 + agent 会话数（产品决策 2）。两类取不同源、**不双计**：终端取自
  // tabsByWorktree（纯 TerminalTab[]）；agent 会话在其中无记录，只存在于 unifiedTabsByWorktree。
  const sessionCount = terminalCount + agentCount

  // 折叠时不请求（传 null）；展开时按 path 取数。
  const { refs, truncated, loading, error, invalidate } = useBranchRefs(
    !collapsed && worktree ? worktree.path : null
  )

  const owners = useMemo(() => ownerByBranch(repoWorktrees ?? []), [repoWorktrees])

  const refreshAfterCheckout = useCallback(async () => {
    try {
      if (refreshActiveGitStatusAfterMutation) {
        await refreshActiveGitStatusAfterMutation()
      } else if (worktree) {
        // 回退路径仅覆盖本地宿主（不接 runtime/connectionId）：面板调用面总会注入
        // refreshActiveGitStatusAfterMutation；无注入时（如独立/测试宿主）按本地刷新。
        await refreshGitStatusForWorktree({
          worktreeId,
          worktreePath: worktree.path,
          deps: {
            setGitStatus: useAppStore.getState().setGitStatus,
            updateWorktreeGitIdentity: useAppStore.getState().updateWorktreeGitIdentity,
            setUpstreamStatus: useAppStore.getState().setUpstreamStatus,
            fetchUpstreamStatus: useAppStore.getState().fetchUpstreamStatus,
          },
          request: { admissionTier: 'interactive' },
        })
      }
    } catch (e) {
      // 跟刷失败不能伪装成"检出失败"：分支已真切过去。只 console.warn，交给轮询兜底
      // （对齐 source-control 既有 post-mutation 语义）。
      console.warn('[git-branch] post-checkout status refresh failed', e)
    }
  }, [refreshActiveGitStatusAfterMutation, worktree, worktreeId])

  const executeCheckout = useCallback(
    async (ref: GitRefEntry) => {
      if (!worktree) return
      // §8.2 发起前权威复查（getState 而非渲染期闭包）：远端 op 进行中不得起本地写，防“先开框、再起远端、后确认”绕过。
      if (useAppStore.getState().isRemoteOperationActive) return
      if (writeInFlightRef.current) return
      writeInFlightRef.current = true
      setOpInFlight(true)
      const target =
        ref.category === 'remote'
          ? { kind: 'remote' as const, name: ref.name }
          : { kind: 'local' as const, name: ref.name }
      try {
        const outcome = await checkoutRuntimeGit(
          { settings, worktreeId, worktreePath: worktree.path },
          target
        )
        // F1 竞态/外部占用：结构化软错误（不经 BridgeError）。优先按短名跳 ade 已知的占用工作区；
        // owners 命中不到（ade 未知的外部 worktree）才回落到用 worktreePath 提示 —— 「worktreePath 被用上」的落点。
        if (
          typeof outcome.status === 'object' &&
          'refused' in outcome.status &&
          outcome.status.refused === 'checked-out-elsewhere'
        ) {
          const occupant = outcome.branch ? owners.get(outcome.branch) ?? null : null
          if (occupant) {
            activateAndRevealWorktree(occupant.worktreeId, { executionHostId: occupant.hostId })
          } else {
            toast.warning(
              translate(
                'auto.components.git-branch.checkoutOccupiedExternally',
                'This branch is checked out in another worktree: {{path}}',
                { path: outcome.worktreePath ?? '' }
              )
            )
          }
          return
        }
        if (outcome.status === 'invalid-target') {
          toast.error(
            translate('auto.components.git-branch.checkoutInvalid', 'Invalid checkout target.')
          )
          return
        }
        // 'switched' | 'already-on-branch'：检出已落地。刷状态跟刷 Worktree.branch，
        // 再 invalidate 让常驻树重新取数（区块不关，这是与 2B 弹层的关键差异）。
        await refreshAfterCheckout()
        invalidate()
      } catch (e) {
        // 环境级失败（git 拒绝覆盖本地改动等）原样上抛；后端 message 是英文，这里只做兜底提示。
        toast.error(e instanceof Error ? e.message : String(e))
      } finally {
        writeInFlightRef.current = false
        setOpInFlight(false)
      }
    },
    [worktree, worktreeId, owners, settings, refreshAfterCheckout, invalidate]
  )

  // 给本地分支新建分支：弹名字框后提交。create 不改当前分支 → 成功仅 invalidate（§8.3）。
  const executeNewBranch = useCallback(
    async (name: string, startRef: GitRefEntry) => {
      if (!worktree) return
      // §8.2 发起前权威复查（getState 而非渲染期闭包）：远端 op 进行中不得起本地写，防“先开框、再起远端、后确认”绕过。
      if (useAppStore.getState().isRemoteOperationActive) return
      if (writeInFlightRef.current) return
      writeInFlightRef.current = true
      setOpInFlight(true)
      try {
        const outcome = await createRuntimeGitBranch(
          { settings, worktreeId, worktreePath: worktree.path },
          { name, startPoint: startRef.oid }
        )
        if (outcome.status === 'created') {
          setNewBranchTarget(null)
          invalidate()
        } else {
          // 按 refusal 变体分文案（不是一律当 NameTaken）；未知变体走通用兜底（与增量4 共 enum 时不误报）。
          const refusal =
            typeof outcome.status === 'object' && 'refused' in outcome.status
              ? outcome.status.refused
              : null
          toast.error(
            refusal === 'name-taken'
              ? translate(
                  'auto.components.git-branch.newBranchNameTaken',
                  'Branch "{{name}}" already exists.',
                  { name }
                )
              : translate(
                  'auto.components.git-branch.newBranchCreateFailed',
                  'Could not create branch.'
                )
          )
          // NameTaken 等保持框开着让用户改名（非法名会 reject 进 catch）。
        }
      } catch (e) {
        toast.error(e instanceof Error ? e.message : String(e))
      } finally {
        writeInFlightRef.current = false
        setOpInFlight(false)
      }
    },
    [worktree, worktreeId, settings, invalidate]
  )

  // 设 upstream。目标是当前分支时影响 ahead/behind → 额外刷状态；否则仅 invalidate（§8.3）。
  const executeSetUpstream = useCallback(
    async (branch: string, upstream: string) => {
      if (!worktree) return
      // §8.2 发起前权威复查（getState 而非渲染期闭包）：远端 op 进行中不得起本地写，防“先开框、再起远端、后确认”绕过。
      if (useAppStore.getState().isRemoteOperationActive) return
      if (writeInFlightRef.current) return
      writeInFlightRef.current = true
      setOpInFlight(true)
      try {
        const outcome = await setRuntimeGitBranchUpstream(
          { settings, worktreeId, worktreePath: worktree.path },
          { branch, upstream }
        )
        if (outcome.status === 'set') {
          if ((branchName(worktree.branch ?? '') || null) === branch) {
            await refreshAfterCheckout()
          }
          invalidate()
        } else {
          const refusal =
            typeof outcome.status === 'object' && 'refused' in outcome.status
              ? outcome.status.refused
              : null
          toast.error(
            refusal === 'upstream-missing'
              ? translate(
                  'auto.components.git-branch.setUpstreamMissing',
                  'Upstream {{upstream}} no longer exists.',
                  { upstream }
                )
              : translate(
                  'auto.components.git-branch.setUpstreamFailed',
                  'Could not set upstream.'
                )
          )
        }
      } catch (e) {
        toast.error(e instanceof Error ? e.message : String(e))
      } finally {
        writeInFlightRef.current = false
        setOpInFlight(false)
      }
    },
    [worktree, worktreeId, settings, refreshAfterCheckout, invalidate]
  )

  // 改名。目标恒为非当前分支（isHead 不可见）→ 成功仅 invalidate（§8.3）。
  const executeRename = useCallback(
    async (oldName: string, newName: string) => {
      if (!worktree) return
      // §8.2 发起前权威复查（getState 而非渲染期闭包）：远端 op 进行中不得起本地写，防“先开框、再起远端、后确认”绕过。
      if (useAppStore.getState().isRemoteOperationActive) return
      if (writeInFlightRef.current) return
      writeInFlightRef.current = true
      setOpInFlight(true)
      try {
        const outcome = await renameRuntimeGitBranch(
          { settings, worktreeId, worktreePath: worktree.path },
          { oldName, newName }
        )
        if (outcome.status === 'renamed') {
          setRenameTarget(null)
          invalidate()
          return
        }
        const refusal =
          typeof outcome.status === 'object' && 'refused' in outcome.status
            ? outcome.status.refused
            : null
        if (refusal === 'name-taken') {
          toast.error(
            translate(
              'auto.components.git-branch.renameNameTaken',
              'Branch "{{name}}" already exists.',
              { name: newName }
            )
          )
        } else if (refusal && typeof refusal === 'object' && 'checked-out' in refusal) {
          toast.error(
            translate(
              'auto.components.git-branch.renameCheckedOut',
              'This branch is checked out in another worktree.'
            )
          )
        } else {
          toast.error(
            translate('auto.components.git-branch.renameFailed', 'Could not rename branch.')
          )
        }
      } catch (e) {
        toast.error(e instanceof Error ? e.message : String(e))
      } finally {
        writeInFlightRef.current = false
        setOpInFlight(false)
      }
    },
    [worktree, worktreeId, settings, invalidate]
  )

  // 删除（两步）。force=false 先 `-d`；后端回 NotMergeable 则弹强制确认框；确认后
  // force=true 走 `-D`。占用回 CheckedOut → 跳转已知占用工作区，否则按 path 提示。
  // 目标恒为非当前分支 → 成功仅 invalidate（§8.3）。
  const executeDelete = useCallback(
    async (ref: GitRefEntry, force: boolean) => {
      if (!worktree) return
      // §8.2 发起前权威复查（getState 而非渲染期闭包）：远端 op 进行中不得起本地写，防“先开框、再起远端、后确认”绕过。
      if (useAppStore.getState().isRemoteOperationActive) return
      if (writeInFlightRef.current) return
      writeInFlightRef.current = true
      setOpInFlight(true)
      try {
        const outcome = await deleteRuntimeGitBranch(
          { settings, worktreeId, worktreePath: worktree.path },
          { branch: ref.name, force }
        )
        if (outcome.status === 'deleted') {
          setDeleteForce(null)
          invalidate()
          return
        }
        const refusal =
          typeof outcome.status === 'object' && 'refused' in outcome.status
            ? outcome.status.refused
            : null
        if (refusal && typeof refusal === 'object' && 'not-mergeable' in refusal) {
          setDeleteForce({ ref, unmergedCount: refusal['not-mergeable'].unmergedCount })
        } else if (refusal && typeof refusal === 'object' && 'checked-out' in refusal) {
          const occupant = owners.get(ref.name)
          if (occupant) {
            activateAndRevealWorktree(occupant.worktreeId, { executionHostId: occupant.hostId })
          } else {
            toast.warning(
              translate(
                'auto.components.git-branch.deleteOccupiedExternally',
                'This branch is checked out in another worktree: {{path}}',
                { path: refusal['checked-out'].worktreePath }
              )
            )
          }
          setDeleteForce(null)
        } else {
          toast.error(
            translate('auto.components.git-branch.deleteFailed', 'Could not delete branch.')
          )
        }
      } catch (e) {
        toast.error(e instanceof Error ? e.message : String(e))
      } finally {
        writeInFlightRef.current = false
        setOpInFlight(false)
      }
    },
    [worktree, worktreeId, settings, owners, invalidate]
  )

  // Update（§7.4）：目标=该分支 owner 的 worktree。复用 store `pullBranch`（=git pull，含 fetch）：
  // 它自带 beginRemoteOperation/错误 toast/upstream 刷新。按仓库 owner 路由 settings。
  // 发起前用 getState 权威复查远端门（同其余写动作）。
  const executeUpdate = useCallback(
    async (ref: GitRefEntry) => {
      if (!worktree || writeInFlightRef.current) return
      if (useAppStore.getState().isRemoteOperationActive) return
      const owner = owners.get(ref.name)
      if (!owner) return
      try {
        await pullBranch?.(owner.worktreeId, owner.path, undefined, undefined, { runtimeTargetSettings: settings })
      } catch {
        return
      }
      if (owner.worktreeId === worktreeId) await refreshAfterCheckout()
      invalidate()
    },
    [worktree, worktreeId, owners, settings, pullBranch, refreshAfterCheckout, invalidate]
  )

  // Push…（§7.4）：目标=owner worktree，按仓库 owner 路由 settings + 该 worktree 的 pushTarget。
  // 无 upstream 时 publish。注：既有 `git_push` 不建跟踪（不带 -u），故 publish 成功后额外链一步
  // set-upstream（origin/<branch>），让 Update 随后能出现；best-effort，失败不阻断已落地的推送。
  const executePush = useCallback(
    async (ref: GitRefEntry) => {
      if (!worktree || writeInFlightRef.current) return
      if (useAppStore.getState().isRemoteOperationActive) return
      const owner = owners.get(ref.name)
      if (!owner) return
      const ownerWt = repoWorktrees?.find((w) => w.id === owner.worktreeId)
      const remoteName = ownerWt?.pushTarget?.remoteName ?? 'origin'
      const publish = ref.upstream == null
      try {
        await pushBranch?.(owner.worktreeId, owner.path, publish, undefined, ownerWt?.pushTarget, { runtimeTargetSettings: settings })
      } catch {
        return
      }
      if (publish) {
        try {
          await setRuntimeGitBranchUpstream(
            { settings, worktreeId: owner.worktreeId, worktreePath: owner.path },
            { branch: ref.name, upstream: `${remoteName}/${ref.name}` }
          )
        } catch {
          // 设跟踪失败不影响已推送的提交；不弹错（推送本身已成功）。
        }
      }
      invalidate()
    },
    [worktree, owners, repoWorktrees, settings, pushBranch, invalidate]
  )

  // 与当前工作区对比选中分支：取该分支的 compare（{summary, entries}），开一个自带该列表的
  // combined-branch tab。**只读**，不入 writeInFlightRef/opInFlight。compare 非 ready 不出 tab。
  const executeShowDiff = useCallback(
    async (ref: GitRefEntry) => {
      if (!worktree) return
      if (showDiffBusyRef.current) return
      showDiffBusyRef.current = true
      try {
        const result = await getRuntimeGitBranchCompare(
          { settings, worktreeId, worktreePath: worktree.path },
          ref.name
        )
        // 与兄弟 openAllDiffs 同口径：ready 但三 oid 不全（HEAD 退化）也是空/错列表 → 不开 tab。
        const s = result.summary
        if (s.status !== 'ready' || !s.baseOid || !s.headOid || !s.mergeBase) {
          toast.error(
            translate(
              'auto.components.git-branch.showDiffUnavailable',
              'Cannot compare this branch with the working tree.'
            )
          )
          return
        }
        openBranchAllDiffs?.(worktreeId, worktree.path, s, undefined, result.entries)
      } catch (e) {
        toast.error(e instanceof Error ? e.message : String(e))
      } finally {
        showDiffBusyRef.current = false
      }
    },
    [worktree, worktreeId, settings, openBranchAllDiffs]
  )

  // Set Upstream 子菜单候选：已加载 refs 里的 remote 条目，同名优先（`origin/<branch>` 排前）。
  const buildRemoteCandidates = useCallback(
    (branch: string): GitRefEntry[] => {
      const rank = (ref: GitRefEntry): number => {
        const slash = ref.name.indexOf('/')
        return slash >= 0 && ref.name.slice(slash + 1) === branch ? 1 : 0
      }
      return refs
        .filter((ref) => ref.category === 'remote')
        .sort((a, b) => rank(b) - rank(a))
    },
    [refs]
  )

  const ctx = useMemo<BranchActionContext>(() => {
    return {
      headBranch: branchName(worktree?.branch ?? '') || null,
      ownerOf: (shortName) => owners.get(shortName) ?? null,
      activateWorkspace: (owner) => {
        activateAndRevealWorktree(owner.worktreeId, { executionHostId: owner.hostId })
      },
      newWorkspaceFrom: (ref: GitRefEntry) => {
        openModal('new-workspace-composer', {
          initialRepoId: repoId,
          initialBaseBranch: ref.name,
          telemetrySource: 'source_control_branch_menu',
        })
      },
      checkout: (ref: GitRefEntry) => {
        // 产品决策 1：脏或有终端/会话才弹确认框；干净且无会话直接检出。
        if (needsCheckoutConfirmation({ dirtyCount: dirty.length, sessionCount })) {
          setPendingCheckout(ref)
        } else {
          void executeCheckout(ref)
        }
      },
      // 增量3：New Branch 弹名字框（start-point = 选中 ref）；Set Upstream 直接执行（子菜单已选定候选）。
      newBranch: (ref: GitRefEntry) => setNewBranchTarget(ref),
      setUpstream: (branch: string, upstream: string) => void executeSetUpstream(branch, upstream),
      remoteRefCandidates: buildRemoteCandidates,
      // 增量4：Rename 弹名字框；Delete 先走 force=false（未合并则框内升 -D）。opInFlight 驱菜单置灰。
      rename: (ref: GitRefEntry) => setRenameTarget(ref),
      requestDelete: (ref: GitRefEntry) => void executeDelete(ref, false),
      showDiff: (ref: GitRefEntry) => void executeShowDiff(ref),
      // 增量5：Update/Push 走 store 现成远端动作；remoteOpActive 驱全部写动作置灰。
      update: (ref: GitRefEntry) => void executeUpdate(ref),
      push: (ref: GitRefEntry) => void executePush(ref),
      remoteOpActive,
      opInFlight,
    }
  }, [
    owners,
    openModal,
    repoId,
    worktree?.branch,
    dirty.length,
    sessionCount,
    executeCheckout,
    executeSetUpstream,
    executeRename,
    executeDelete,
    executeShowDiff,
    executeUpdate,
    executePush,
    remoteOpActive,
    opInFlight,
    buildRemoteCandidates,
  ])

  const toggleCollapse = useCallback(() => {
    setCollapsed((prev) => {
      const next = !prev
      persistBranchSectionCollapsed(next)
      return next
    })
  }, [])

  if (!worktree) return null

  return (
    <section
      data-testid="source-control-branch-tree-section"
      className="flex shrink-0 flex-col border-b border-border/60"
    >
      <button
        type="button"
        data-testid="branch-section-toggle"
        aria-expanded={!collapsed}
        onClick={toggleCollapse}
        className={cn(
          'flex items-center gap-1.5 px-2 py-1 text-left text-xs font-medium text-muted-foreground',
          'outline-none hover:text-foreground focus-visible:ring-1 focus-visible:ring-ring'
        )}
      >
        <span aria-hidden="true" className="w-3 shrink-0 text-center">
          {collapsed ? '›' : '⌄'}
        </span>
        {translate('auto.components.git-branch.branchesSection', 'Branches')}
      </button>

      {!collapsed && (
        <div className="px-1.5 pb-1.5">
          {loading && refs.length === 0 ? (
            <p className="px-1 py-1 text-xs text-muted-foreground">
              {translate('auto.components.git-branch.loading', 'Loading branches…')}
            </p>
          ) : error && refs.length === 0 ? (
            <p className="px-1 py-1 text-xs text-muted-foreground">
              {translate('auto.components.git-branch.loadFailed', 'Failed to read branches.')}
            </p>
          ) : (
            <BranchRefTree
              refs={refs}
              truncated={truncated}
              headBranch={ctx.headBranch}
              checkedOutByBranch={owners}
              onActivate={(row) => {
                primaryBranchAction(row, ctx)?.run(row, ctx)
              }}
              onRenameRequest={(row) => {
                // §7.3 F2：走与菜单同一套 rename 可见/可用谓词，不可则静默（不越菜单置灰）。
                const rename = BRANCH_ACTIONS.find((a) => a.id === 'rename')
                if (!rename || !rename.visible(row, ctx)) return
                if (rename.enabled && !rename.enabled(row, ctx)) return
                rename.run(row, ctx)
              }}
              renderRowMenu={(row, content) =>
                row.kind === 'ref' ? (
                  <BranchRowMenu row={row} ctx={ctx} allowedActionIds={HOST_A_ALLOWED}>
                    {content}
                  </BranchRowMenu>
                ) : (
                  content
                )
              }
            />
          )}
        </div>
      )}

      {pendingCheckout && (
        <CheckoutConfirmDialog
          open
          input={{
            target: pendingCheckout,
            dirtyCount: dirty.length,
            dirtyPaths: dirty.map((entry) => entry.path),
            sessionCount,
          }}
          onCancel={() => setPendingCheckout(null)}
          onConfirm={() => {
            const ref = pendingCheckout
            setPendingCheckout(null)
            void executeCheckout(ref)
          }}
        />
      )}

      {newBranchTarget && (
        <NewBranchDialog
          open
          startPointLabel={newBranchTarget.name}
          onCancel={() => setNewBranchTarget(null)}
          onCreate={(name) => void executeNewBranch(name, newBranchTarget)}
        />
      )}

      {renameTarget && (
        <RenameDialog
          open
          currentName={renameTarget.name}
          onCancel={() => setRenameTarget(null)}
          onRename={(name) => void executeRename(renameTarget.name, name)}
        />
      )}

      {deleteForce && (
        <DeleteConfirmDialog
          open
          branchName={deleteForce.ref.name}
          unmergedCount={deleteForce.unmergedCount}
          onCancel={() => setDeleteForce(null)}
          onForceDelete={() => void executeDelete(deleteForce.ref, true)}
        />
      )}
    </section>
  )
}
