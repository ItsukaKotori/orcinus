import { useCallback, useMemo, useRef, useState } from 'react'
import { Search } from 'lucide-react'
import { cn } from '@/lib/utils'
import { translate } from '@/i18n/i18n'
import type { GitRefEntry } from '../../../../shared/git-ref-types'
import {
  buildBranchTreeRows,
  type BranchTreeRow,
  type BranchWorkspaceOwner,
} from './branch-tree-model'
import { matchesBranchQuery } from './branch-tree-filter'

/** 行 id 里的 `:` `/` 原样进 data-testid（querySelector 侧按字面量拼，无需转义）。 */
export function branchRowTestId(row: BranchTreeRow): string {
  return `branch-row-${row.id}`
}

export type BranchRefTreeProps = {
  refs: readonly GitRefEntry[]
  /** 后端截断过 ref 列表时出行内提示。 */
  truncated?: boolean
  headBranch?: string | null
  checkedOutByBranch?: ReadonlyMap<string, BranchWorkspaceOwner>
  /** Git Log 宿主注入 `All Branches`；Source Control 宿主不注入。 */
  includeAllBranchesRow?: boolean
  /** 高亮行 id（合成人物用 `row:*`，ref 用 `ref:<fullName>`）。 */
  activeValue?: string | null
  onActivate: (row: BranchTreeRow) => void
  className?: string
}

/**
 * IDEA 风格的分支树。刻意**不携带任何业务语义** —— 单击做什么由宿主的
 * `onActivate` 决定（Source Control 头部 = 切 workspace，Git Log 工具栏 = 设查看 ref），
 * 这是「全仓库只留一份树」却不产生语义冲突的前提（spec §5）。
 */
export function BranchRefTree({
  refs,
  truncated = false,
  headBranch = null,
  checkedOutByBranch,
  includeAllBranchesRow = false,
  activeValue = null,
  onActivate,
  className,
}: BranchRefTreeProps): React.JSX.Element {
  const [query, setQuery] = useState('')
  const [collapsed, setCollapsed] = useState<ReadonlySet<string>>(new Set())
  const containerRef = useRef<HTMLDivElement>(null)

  // 折叠态与查询各自独立：搜索只换 refs，不换 collapsed —— 清掉搜索后
  // 用户刚才折起来的组必须还是折着的。
  const visibleRefs = useMemo(
    () => (query.trim() ? refs.filter((r) => matchesBranchQuery(r, query)) : [...refs]),
    [refs, query]
  )

  const rows = useMemo(
    () =>
      buildBranchTreeRows({
        refs: visibleRefs,
        collapsed,
        headBranch,
        checkedOutByBranch,
        includeAllBranchesRow,
      }),
    [visibleRefs, collapsed, headBranch, checkedOutByBranch, includeAllBranchesRow]
  )

  // 空态判据是「没有 ref 行」而不是「没有任何行」：`HEAD (Current Branch)` 这类
  // 导航行在过滤无命中时仍要留在顶上，否则用户得先清空搜索才能跳回当前分支范围。
  const hasRefRows = rows.some((r) => r.kind === 'ref')

  const toggleGroup = useCallback((id: string) => {
    setCollapsed((cur) => {
      const next = new Set(cur)
      if (next.has(id)) next.delete(id)
      else next.add(id)
      return next
    })
  }, [])

  const focusRow = useCallback((index: number) => {
    containerRef.current?.querySelector<HTMLElement>(`[data-row-index="${index}"]`)?.focus()
  }, [])

  const handleKeyDown = useCallback(
    (event: React.KeyboardEvent<HTMLDivElement>) => {
      if (
        event.key !== 'ArrowDown' &&
        event.key !== 'ArrowUp' &&
        event.key !== 'ArrowRight' &&
        event.key !== 'ArrowLeft'
      ) {
        return
      }
      const target = event.target as HTMLElement
      const indexed = Number(target.dataset.rowIndex)
      const current = Number.isNaN(indexed) ? -1 : indexed

      // 焦点在搜索框（无 data-row-index）时 ↓ 进首行：弹层打开即聚焦搜索框，
      // 不能要求用户先用鼠标点一下才能键盘导航。
      if (current === -1) {
        if (event.key !== 'ArrowDown') return
        event.preventDefault()
        focusRow(0)
        return
      }

      event.preventDefault()
      if (event.key === 'ArrowDown') {
        focusRow(Math.min(rows.length - 1, current + 1))
        return
      }
      if (event.key === 'ArrowUp') {
        focusRow(Math.max(0, current - 1))
        return
      }
      const row = rows[current]
      if (row?.kind !== 'group') return
      // 与单击组头共用同一份 collapsed state：两条路径必须互逆，否则会出现
      // 「单击能折叠、←→ 展不开」这类只有真键盘才暴露的偏差。
      const open = event.key === 'ArrowRight'
      setCollapsed((cur) => {
        const next = new Set(cur)
        if (open) next.delete(row.id)
        else next.add(row.id)
        return next
      })
    },
    [rows, focusRow]
  )

  return (
    <div
      ref={containerRef}
      data-testid="branch-tree"
      className={cn('flex flex-col', className)}
      onKeyDown={handleKeyDown}
    >
      <label className="mb-1 flex items-center gap-1.5 rounded border border-input px-1.5 py-0.5">
        <Search className="size-3 shrink-0 text-muted-foreground" aria-hidden="true" />
        <input
          type="search"
          value={query}
          autoFocus
          onChange={(event) => setQuery(event.target.value)}
          placeholder={translate('auto.components.git-branch.searchPlaceholder', 'Branch or tag')}
          className="min-w-0 flex-1 bg-transparent text-xs outline-none placeholder:text-muted-foreground/60"
        />
      </label>
      {truncated && (
        <p className="px-1 pb-1 text-[11px] leading-4 text-muted-foreground">
          {translate(
            'auto.components.git-branch.refsTruncated',
            'Some refs are not shown (list limit reached).'
          )}
        </p>
      )}
      <div role="tree" className="max-h-72 overflow-auto outline-none">
        {rows.map((row, index) => (
          <BranchRowButton
            key={row.id}
            row={row}
            index={index}
            active={activeValue === row.id}
            onActivate={onActivate}
            onToggleGroup={toggleGroup}
          />
        ))}
      </div>
      {!hasRefRows && (
        <p className="px-1 py-1 text-xs text-muted-foreground">
          {translate('auto.components.git-branch.empty', 'No matching branches.')}
        </p>
      )}
    </div>
  )
}

function BranchRowButton({
  row,
  index,
  active,
  onActivate,
  onToggleGroup,
}: {
  row: BranchTreeRow
  index: number
  active: boolean
  onActivate: (row: BranchTreeRow) => void
  onToggleGroup: (id: string) => void
}): React.JSX.Element {
  const onClick =
    row.kind === 'group' ? () => onToggleGroup(row.id) : () => onActivate(row)

  return (
    <button
      type="button"
      role="treeitem"
      aria-selected={active}
      aria-expanded={row.kind === 'group' ? row.expanded : undefined}
      data-testid={branchRowTestId(row)}
      data-row-index={index}
      title={rowTitle(row)}
      onClick={onClick}
      className={cn(
        'flex w-full items-center gap-1.5 rounded px-1 py-0.5 text-left text-xs outline-none focus-visible:ring-1 focus-visible:ring-ring',
        active ? 'bg-accent text-foreground' : 'hover:bg-accent/40'
      )}
      style={{ paddingLeft: `${row.depth * 12 + 4}px` }}
    >
      <RowLeading row={row} />
      <span className="min-w-0 flex-1 truncate">{rowLabel(row)}</span>
      <RowTrailing row={row} />
    </button>
  )
}

/**
 * 图标映射是本设计新定义的（spec §7.1），不是「IDEA 就是这样」：
 *  🏷 = git 报告的 HEAD 分支；★ = 当前分支的 upstream；▣ = 被某 ade workspace 检出。
 * 用字符而非 lucide 图标：行高 ~20px，lucide 在 text-xs 下视觉重量过大。
 */
function RowLeading({ row }: { row: BranchTreeRow }): React.JSX.Element {
  if (row.kind === 'group') {
    return (
      <span aria-hidden="true" className="w-3 shrink-0 text-center text-muted-foreground">
        {row.expanded ? '⌄' : '›'}
      </span>
    )
  }
  if (row.kind === 'synthetic') {
    return <span aria-hidden="true" className="w-3 shrink-0 text-center">◉</span>
  }
  if (row.ref.isHead || row.ref.category === 'tag') {
    return <span aria-hidden="true" className="w-3 shrink-0 text-center">🏷</span>
  }
  if (row.isUpstreamOfHead) {
    return (
      <span
        aria-hidden="true"
        data-testid="branch-upstream-star"
        className="w-3 shrink-0 text-center"
      >
        ★
      </span>
    )
  }
  return <span aria-hidden="true" className="w-3 shrink-0" />
}

function RowTrailing({ row }: { row: BranchTreeRow }): React.JSX.Element | null {
  if (row.kind !== 'ref') return null
  const badge = row.checkedOutBy ? (
    <span
      data-testid="branch-workspace-badge"
      className="shrink-0 text-[10px] text-muted-foreground"
      aria-hidden="true"
    >
      ▣
    </span>
  ) : null
  const { ahead, behind } = row.ref
  if (ahead == null && behind == null) return badge
  return (
    <>
      {badge}
      <span className="shrink-0 tabular-nums text-[10px] text-muted-foreground" aria-hidden="true">
        {translate('auto.components.git-branch.aheadBehind', '↑{{ahead}} ↓{{behind}}', {
          ahead: ahead ?? 0,
          behind: behind ?? 0,
        })}
      </span>
    </>
  )
}

function rowLabel(row: BranchTreeRow): string {
  if (row.kind === 'group') {
    // remote 二级组头就是 remote 名本身（labelKey 形如 `group:remote/origin`），
    // 不是可翻译文案 —— 走 fallback 分支会去查一个不存在的 key。
    return row.labelKey.startsWith('group:remote/')
      ? row.labelFallback
      : translate(row.labelKey, row.labelFallback)
  }
  if (row.kind === 'synthetic') return translate(row.labelKey, row.labelFallback)
  return row.ref.name
}

function rowTitle(row: BranchTreeRow): string | undefined {
  if (row.kind === 'ref') {
    if (row.checkedOutBy) {
      return translate('auto.components.git-branch.checkedOutIn', 'checked out in {{workspace}}', {
        workspace: row.checkedOutBy.displayName,
      })
    }
    const { ahead, behind, upstream } = row.ref
    if (ahead != null && behind != null) {
      return translate(
        'auto.components.git-branch.aheadBehindLabel',
        '{{ahead}} ahead, {{behind}} behind',
        { ahead, behind }
      )
    }
    if (ahead != null) {
      return translate('auto.components.git-branch.aheadOnlyLabel', '{{ahead}} ahead', { ahead })
    }
    if (behind != null) {
      return translate('auto.components.git-branch.behindOnlyLabel', '{{behind}} behind', { behind })
    }
    return upstream ?? undefined
  }
  if (row.kind === 'group') return `${row.labelFallback} (${row.childCount})`
  return undefined
}
