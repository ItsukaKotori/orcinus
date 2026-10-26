import { useCallback, useMemo, useRef, useState } from 'react'
import { useAppStore } from '@/store'
import { findWorktreeById } from '@/store/slices/worktree-helpers'
import { activateAndRevealWorktree } from '@/lib/worktree-activation'
import { branchName } from '@/lib/git-utils'
import { translate } from '@/i18n/i18n'
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover'
import { checkoutRuntimeGit } from '@/runtime/runtime-git-checkout-client'
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
import {
  CheckoutConfirmDialog,
  needsCheckoutConfirmation,
} from './checkout-confirm-dialog'
import { useBranchRefs } from './use-branch-refs'

// 宿主 A 声明的可用动作集合：checkout 是主动作，switch 降级为次要项，
// 恒保留 new-workspace。**不含 view-in-log**（那是 Git Log 宿主专属，spec §13.6）。
const HOST_A_ALLOWED: ReadonlySet<BranchActionId> = new Set(['checkout', 'switch', 'new-workspace'])

// 稳定的空数组引用：脏改动缺失时避免每次渲染新建数组打乱 zustand 选择器一致性。
const EMPTY_ENTRIES: never[] = []

export type SourceControlBranchSwitcherProps = {
  worktreeId: string
  /** 触发器内容（分支名元素）。由宿主渲染，保持 `HeadIdentity` 的字号与截断样式。 */
  children: React.ReactNode
}

/**
 * Source Control 头部的分支树弹层（spec §5 宿主 A）。
 *
 * 只吃一个 `worktreeId`，其余全从 store 自取：把 5 个回调沿 3 层组件钻下去会改动
 * `header-toolbar` / `panel-content` / `panel-ready` 的既有 props 面，并连带打红
 * `SourceControl.*.test.tsx` 那批大桩。代价是本组件不再是纯呈现件，由它自己的测试覆盖。
 *
 * 本增量（2B）把主动作从「跳 workspace」改为「原地 Checkout」（spec §13.6）：脏或
 * 有终端/会话先弹确认框；成功立即刷状态让 `Worktree.branch` 跟上（不等 60s 轮询）；
 * 被别的工作区占用（F1）走跳转。**不注入** `viewInGitLog` —— 该动作只属于 Git Log 宿主。
 */
export function SourceControlBranchSwitcher({
  worktreeId,
  children,
}: SourceControlBranchSwitcherProps): React.JSX.Element {
  const [open, setOpen] = useState(false)
  const [pendingCheckout, setPendingCheckout] = useState<GitRefEntry | null>(null)
  // 防并发 checkout：干净无会话路径是 fire-and-forget，连点两行会在同一 worktree 上
  // 叠两条 `git checkout`，第二条大概率撞 index.lock。进入置位、finally 复位。
  const checkoutInFlightRef = useRef(false)
  const worktree = useAppStore((s) => findWorktreeById(s.worktreesByRepo, worktreeId))
  const openModal = useAppStore((s) => s.openModal)

  // owner 只算**当前 repo** 的 worktree。全仓扫会让另一个仓的同名分支
  // （两个仓都有 `main` 是多仓用户的常态）顶掉本仓的归属，点一下跳去错仓的 workspace。
  const repoId = worktree?.repoId ?? ''
  const repoWorktrees = useAppStore((s) => s.worktreesByRepo[repoId])

  // 确认框数据源（宿主 A 独立于 panel foundation，直接从 store 取，spec §13.7）：
  // 全部走可选链 —— 生产 store 恒有这些 slice，但 switcher 会被面板测试以部分 store 桩
  // 渲染（见 SourceControl.*.test），不加 ? 会在 `undefined[worktreeId]` 上抛。
  const dirtyByWt = useAppStore((s) => s.gitStatusByWorktree?.[worktreeId])
  const terminalCount = useAppStore((s) => s.tabsByWorktree?.[worktreeId]?.length ?? 0)
  const agentCount = useAppStore(
    (s) => s.unifiedTabsByWorktree?.[worktreeId]?.filter((t) => t.contentType === 'agent-session').length ?? 0
  )
  const dirty = dirtyByWt ?? EMPTY_ENTRIES
  const sessionCount = terminalCount + agentCount

  const { refs, truncated, loading, error } = useBranchRefs(
    open ? (worktree?.path ?? null) : null
  )

  const owners = useMemo(() => ownerByBranch(repoWorktrees ?? []), [repoWorktrees])

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
        // settings: null = 本地（getActiveRuntimeTarget(null)→local）。**本增量只服务本地
        // worktree**：refs 取数、checkout、跟刷均按本机路由，未接 runtime/connectionId。
        const outcome = await checkoutRuntimeGit(
          { settings: null, worktreeId, worktreePath: worktree.path },
          target
        )
        // F1 竞态/外部占用：占用信息是结构化软错误（不经 BridgeError）。
        // 优先按短名跳 ade 已知的占用工作区；owners 命中不到（ade 未知的外部 worktree）
        // 才回落到用 worktreePath 提示 —— 这也是「worktreePath 被用上」的落点。
        if (
          typeof outcome.status === 'object' &&
          'refused' in outcome.status &&
          outcome.status.refused === 'checked-out-elsewhere'
        ) {
          const occupant = outcome.branch ? owners.get(outcome.branch) ?? null : null
          if (occupant) {
            setOpen(false)
            activateAndRevealWorktree(occupant.worktreeId, { executionHostId: occupant.hostId })
          } else {
            toast.warning(
              translate(
                'auto.components.git-branch.checkoutOccupiedExternally',
                'This branch is checked out in another worktree: {{path}}',
                { path: outcome.worktreePath ?? '' }
              )
            )
            setOpen(false)
          }
          return
        }
        if (outcome.status === 'invalid-target') {
          toast.error(
            translate('auto.components.git-branch.checkoutInvalid', 'Invalid checkout target.')
          )
          return
        }
        // 'switched' | 'already-on-branch'：检出已落地。立即刷状态（复用既有链路跟刷
        // Worktree.branch），不等最长 60s 且被 isWindowVisible() 门控的轮询。
        setOpen(false)
        // 跟刷失败不能伪装成"检出失败"：分支已真切过去，故只 console.warn，交给重开取数/轮询兜底
        // （对齐 source-control 既有 post-mutation 语义 use-status-refresh.ts 的 refreshActiveGitStatusAfterMutation）。
        try {
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
        } catch (e) {
          console.warn('[git-branch] post-checkout status refresh failed', e)
        }
        // 分支树 refs 在下次打开弹层时按 `open ? path : null` 重新取数；此处收起后无需显式 invalidate。
      } catch (e) {
        // 环境级失败（git 拒绝覆盖本地改动等）原样上抛；后端 message 是英文，这里只做兜底提示。
        toast.error(e instanceof Error ? e.message : String(e))
      } finally {
        checkoutInFlightRef.current = false
      }
    },
    [worktree, worktreeId, owners]
  )

  const ctx = useMemo<BranchActionContext>(() => {
    return {
      // `Worktree.branch` 生产形态是**完整 ref**（`refs/heads/main`），而树里比的是短名。
      // 不剥前缀的话 `headUpstream` 永远查不到，★ 一条都不出。
      headBranch: branchName(worktree?.branch ?? '') || null,
      ownerOf: (shortName) => owners.get(shortName) ?? null,
      activateWorkspace: (owner) => {
        setOpen(false)
        // 走 activateAndRevealWorktree 而不是裸 setActiveWorktree：后者把
        // `activeWorkspaceExecutionHostId` 写成 `executionHostId ?? null`（**不**从
        // worktree identity 推导），漏传宿主会把跨宿主 workspace 的 tab 打戳成 local。
        // 返回 false = 弹层打开到点击之间该 workspace 已被删，静默吞掉。
        activateAndRevealWorktree(owner.worktreeId, { executionHostId: owner.hostId })
      },
      newWorkspaceFrom: (ref: GitRefEntry) => {
        setOpen(false)
        openModal('new-workspace-composer', {
          initialRepoId: repoId,
          // remote / tag 条目也走这里：composer 把它当 base branch / start-point 解析。
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

  // 解析不出 worktree 时不接弹层 —— 一个打不开的触发器比没有触发器更糟。
  // 注意：这**不是**一条不可达分支。面板门禁走 `getKnownWorktreeById`（会合成 folder 行
  // 并回落 detectedWorktreesByRepo），本组件走只扫 `worktreesByRepo` 的 `findWorktreeById`，
  // 两个 lookup 不等价，detected-only 行与 worktrees:changed 的 purge 窗口都能走到这里。
  // 已知遗留：此时 children 仍带着宿主的 aria-haspopup="tree"，是个打不开的死触发器。
  if (!worktree) return <>{children}</>

  return (
    <Popover open={open} onOpenChange={setOpen}>
      <PopoverTrigger asChild>{children}</PopoverTrigger>
      <PopoverContent align="start" side="bottom" sideOffset={4} className="w-64 p-1.5">
        {loading && refs.length === 0 ? (
          <p className="px-1 py-1 text-xs text-muted-foreground">
            {translate('auto.components.git-branch.loading', 'Loading branches…')}
          </p>
        ) : error && refs.length === 0 ? (
          // 失败不弹错也不自动收起：popover 里一行文案（spec §7.5）。
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
      </PopoverContent>
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
    </Popover>
  )
}
