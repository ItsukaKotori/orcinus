import { useCallback, useMemo, useRef, useState } from 'react'
import { useAppStore } from '@/store'
import { findWorktreeById } from '@/store/slices/worktree-helpers'
import { activateAndRevealWorktree } from '@/lib/worktree-activation'
import { branchName } from '@/lib/git-utils'
import { translate } from '@/i18n/i18n'
import { cn } from '@/lib/utils'
import { checkoutRuntimeGit } from '@/runtime/runtime-git-checkout-client'
import { createRuntimeGitBranch, setRuntimeGitBranchUpstream } from '@/runtime/runtime-git-client'
import type { RuntimeGitContext } from '@/runtime/runtime-git-client-context'
import { refreshGitStatusForWorktree } from '../right-sidebar/git-status-refresh'
import { toast } from 'sonner'
import type { GitRefEntry } from '../../../../shared/git-ref-types'
import { BranchRefTree } from './branch-ref-tree'
import { ownerByBranch } from './branch-tree-model'
import {
  primaryBranchAction,
  type BranchActionContext,
  type BranchActionId,
} from './branch-actions'
import { BranchRowMenu } from './branch-menu'
import { CheckoutConfirmDialog, needsCheckoutConfirmation } from './checkout-confirm-dialog'
import { NewBranchDialog } from './new-branch-dialog'
import { useBranchRefs } from './use-branch-refs'
import {
  persistBranchSectionCollapsed,
  readPersistedBranchSectionCollapsed,
} from './source-control-branch-section-view-state'

// 宿主 A 声明的可用动作集合：checkout 是主动作，switch 降级为次要项，恒保留 new-workspace。
// new-branch / set-upstream（增量3）。**不含 view-in-log**（Git Log 专属）；**不含 show-diff**
// （其宿主接线延到增量3b，需 per-ref git_branch_compare）。
const HOST_A_ALLOWED: ReadonlySet<BranchActionId> = new Set([
  'checkout',
  'switch',
  'new-workspace',
  'new-branch',
  'set-upstream',
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
  // 防并发写：checkout / new-branch / set-upstream 都是会改 ref 的写操作，连点会在
  // 同一 worktree 上叠两条 git 写、大概率撞 index.lock。共用一个 in-flight 门，进入置位、finally 复位。
  const writeInFlightRef = useRef(false)

  const worktree = useAppStore((s) => findWorktreeById(s.worktreesByRepo, worktreeId))
  const openModal = useAppStore((s) => s.openModal)

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
      if (writeInFlightRef.current) return
      writeInFlightRef.current = true
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
      }
    },
    [worktree, worktreeId, owners, settings, refreshAfterCheckout, invalidate]
  )

  // 给本地分支新建分支：弹名字框后提交。create 不改当前分支 → 成功仅 invalidate（§8.3）。
  const executeNewBranch = useCallback(
    async (name: string, startRef: GitRefEntry) => {
      if (!worktree) return
      if (writeInFlightRef.current) return
      writeInFlightRef.current = true
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
      }
    },
    [worktree, worktreeId, settings, invalidate]
  )

  // 设 upstream。目标是当前分支时影响 ahead/behind → 额外刷状态；否则仅 invalidate（§8.3）。
  const executeSetUpstream = useCallback(
    async (branch: string, upstream: string) => {
      if (!worktree) return
      if (writeInFlightRef.current) return
      writeInFlightRef.current = true
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
      }
    },
    [worktree, worktreeId, settings, refreshAfterCheckout, invalidate]
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
    </section>
  )
}
