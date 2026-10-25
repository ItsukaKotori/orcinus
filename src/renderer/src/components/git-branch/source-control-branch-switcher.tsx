import { useMemo, useState } from 'react'
import { useAppStore } from '@/store'
import { findWorktreeById } from '@/store/slices/worktree-helpers'
import { activateAndRevealWorktree } from '@/lib/worktree-activation'
import { branchName } from '@/lib/git-utils'
import { translate } from '@/i18n/i18n'
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover'
import type { GitRefEntry } from '../../../../shared/git-ref-types'
import { BranchRefTree } from './branch-ref-tree'
import { ownerByBranch } from './branch-tree-model'
import { primaryBranchAction, type BranchActionContext } from './branch-actions'
import { useBranchRefs } from './use-branch-refs'

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
 * 这里**不注入** `viewInGitLog` —— 该动作只属于 Git Log 宿主（spec §7.4），
 * 宿主裁剪靠少传回调而不是额外的 opts。
 */
export function SourceControlBranchSwitcher({
  worktreeId,
  children,
}: SourceControlBranchSwitcherProps): React.JSX.Element {
  const [open, setOpen] = useState(false)
  const worktree = useAppStore((s) => findWorktreeById(s.worktreesByRepo, worktreeId))
  const openModal = useAppStore((s) => s.openModal)

  // owner 只算**当前 repo** 的 worktree。全仓扫会让另一个仓的同名分支
  // （两个仓都有 `main` 是多仓用户的常态）顶掉本仓的归属，点一下跳去错仓的 workspace。
  const repoId = worktree?.repoId ?? ''
  const repoWorktrees = useAppStore((s) => s.worktreesByRepo[repoId])

  const { refs, truncated, loading, error } = useBranchRefs(
    open ? (worktree?.path ?? null) : null
  )

  const owners = useMemo(() => ownerByBranch(repoWorktrees ?? []), [repoWorktrees])

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
        // worktree identity 推导），漏传宿主会把跨宿主 workspace 的 tab 打戳成 local；
        // 它也不管 setActiveRepo / setActiveView / reveal / 访问记录。
        // 返回 false = 弹层打开到点击之间该 workspace 已被删，静默吞掉。
        activateAndRevealWorktree(owner.worktreeId, { executionHostId: owner.hostId })
      },
      newWorkspaceFrom: (ref: GitRefEntry) => {
        setOpen(false)
        openModal('new-workspace-composer', {
          initialRepoId: repoId,
          // remote / tag 条目也走这里：composer 把它当 base branch / start-point 解析
          // （既有测试用 `origin/feature/x` 与 `refs/pull/42/head` 作 base 通过）。
          initialBaseBranch: ref.name,
          telemetrySource: 'source_control_branch_menu',
        })
      },
    }
  }, [owners, openModal, repoId, worktree?.branch])

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
          />
        )}
      </PopoverContent>
    </Popover>
  )
}
