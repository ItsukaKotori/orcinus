import { useMemo, useState } from 'react'
import { useAppStore } from '@/store'
import { findWorktreeById } from '@/store/slices/worktree-helpers'
import { translate } from '@/i18n/i18n'
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover'
import type { GitRefEntry } from '../../../../shared/git-ref-types'
import { BranchRefTree } from './branch-ref-tree'
import { ownerByBranch, type BranchWorkspaceOwner } from './branch-tree-model'
import { primaryBranchAction, type BranchActionContext } from './branch-actions'
import { useBranchRefs } from './use-branch-refs'

export type SourceControlBranchSwitcherProps = {
  worktreeId: string
  /** 触发器内容（分支名元素）。由宿主渲染，保持 `HeadIdentity` 现有样式与 tooltip。 */
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
  const worktreesByRepo = useAppStore((s) => s.worktreesByRepo)
  const setActiveWorktree = useAppStore((s) => s.setActiveWorktree)
  const openModal = useAppStore((s) => s.openModal)

  const { refs, truncated, loading, error } = useBranchRefs(
    open ? (worktree?.path ?? null) : null
  )

  const owners = useMemo(() => ownerByBranch(Object.values(worktreesByRepo).flat()), [worktreesByRepo])

  const ctx = useMemo<BranchActionContext>(() => {
    const ownerOf = (branchName: string): BranchWorkspaceOwner | null =>
      owners.get(branchName) ?? null
    return {
      headBranch: worktree?.branch || null,
      ownerOf,
      activateWorkspace: (owner) => {
        setOpen(false)
        // 不传 executionHostId：store 会从该 worktree 自己的 identity 推出宿主，
        // 与侧栏跳转同一条路径。
        setActiveWorktree(owner.worktreeId)
      },
      newWorkspaceFrom: (ref: GitRefEntry) => {
        setOpen(false)
        openModal('new-workspace-composer', {
          initialRepoId: worktree?.repoId,
          // remote / tag 条目也走这里：composer 把它当 start-point 解析。
          initialBaseBranch: ref.name,
          telemetrySource: 'source_control_branch_menu',
        })
      },
    }
  }, [owners, setActiveWorktree, openModal, worktree?.branch, worktree?.repoId])

  // 解析不出 worktree（元数据未到位 / 已被移除）时不接弹层 —— 一个打不开的
  // 触发器比没有触发器更糟。
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
