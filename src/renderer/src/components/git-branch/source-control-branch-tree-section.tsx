import { useCallback, useMemo, useRef, useState } from 'react'
import { useAppStore } from '@/store'
import { findWorktreeById } from '@/store/slices/worktree-helpers'
import { activateAndRevealWorktree } from '@/lib/worktree-activation'
import { branchName } from '@/lib/git-utils'
import { translate } from '@/i18n/i18n'
import { cn } from '@/lib/utils'
import { checkoutRuntimeGit } from '@/runtime/runtime-git-checkout-client'
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
import { useBranchRefs } from './use-branch-refs'
import {
  persistBranchSectionCollapsed,
  readPersistedBranchSectionCollapsed,
} from './source-control-branch-section-view-state'

// 宿主 A 声明的可用动作集合：checkout 是主动作，switch 降级为次要项，恒保留 new-workspace。
// **不含 view-in-log**（那是 Git Log 宿主专属，spec §13.6）。
const HOST_A_ALLOWED: ReadonlySet<BranchActionId> = new Set(['checkout', 'switch', 'new-workspace'])

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
  // 防并发 checkout：干净无会话路径是 fire-and-forget，连点两行会在同一 worktree 上叠两条
  // `git checkout`，第二条大概率撞 index.lock。进入置位、finally 复位。
  const checkoutInFlightRef = useRef(false)

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
      if (checkoutInFlightRef.current) return
      checkoutInFlightRef.current = true
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
        checkoutInFlightRef.current = false
      }
    },
    [worktree, worktreeId, owners, settings, refreshAfterCheckout, invalidate]
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
    }
  }, [owners, openModal, repoId, worktree?.branch, dirty.length, sessionCount, executeCheckout])

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
        <div className="max-h-72 overflow-auto px-1.5 pb-1.5">
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
    </section>
  )
}
