import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { Boxes, ChevronDown, ChevronRight, CircleDot, GitBranch, Search, Star, Tag } from 'lucide-react'
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
  /** §7.3：F2 对焦点 ref 行触发改名请求；不传 → F2 无操作。 */
  onRenameRequest?: (row: BranchTreeRow) => void
  /**
   * 每行菜单接缝（增量 2B）。提供时**仅 ref 行**把已渲染好的 `<button>` 主体作为 `content`
   * 交给接缝，由接缝（宿主的 `BranchRowMenu`）包进触发区并追加 `⋯`。不提供时 ref 行
   * 逐字节走原 `<button>`（宿主 B / 既有测试零回归）。树本身不 import 动作语义（spec §5）。
   */
  renderRowMenu?: (row: BranchTreeRow, content: React.ReactNode) => React.ReactNode
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
  onRenameRequest,
  renderRowMenu,
  className,
}: BranchRefTreeProps): React.JSX.Element {
  const [query, setQuery] = useState('')
  const [collapsed, setCollapsed] = useState<ReadonlySet<string>>(new Set())
  const containerRef = useRef<HTMLDivElement>(null)
  const inputRef = useRef<HTMLInputElement>(null)

  // Why: `autoFocus` 在此活不下来 —— 它在 commit 期触发，而宿主 Radix Popover
  // 的焦点陷阱 scope 会把焦点拉回 trigger。仓库同一场景已有结论与修法
  // （`sidebar/SidebarProjectFilterPanel.tsx` 顶部注释）。晚一帧时两个 scope
  // 都已 settle，focus 才站得住。spec §7.3 要求「弹层打开即聚焦搜索框」。
  useEffect(() => {
    const frame = requestAnimationFrame(() => inputRef.current?.focus())
    return () => cancelAnimationFrame(frame)
  }, [])

  // 折叠态与查询各自独立：搜索只换 refs，不换 collapsed —— 清掉搜索后
  // 用户刚才折起来的组必须还是折着的。
  const visibleRefs = useMemo(
    () => (query.trim() ? refs.filter((r) => matchesBranchQuery(r, query)) : [...refs]),
    [refs, query]
  )

  /**
   * ★ 的源必须来自**未过滤**的 refs：否则搜 `log` 时 `main` 被抹掉，
   * 它的 upstream 也查不到，★ 跟着消失 —— 而同一行的 `HEAD (Current Branch)`
   * 导航行却是刻意设计成过滤态存活的，两者口径不自洽。
   */
  const headUpstream = useMemo(() => {
    const head = refs.find((r) => r.isHead)
    const name = headBranch?.trim() || head?.name || null
    if (!name) return null
    return refs.find((r) => r.category === 'local' && r.name === name)?.upstream ?? null
  }, [refs, headBranch])

  const rows = useMemo(
    () =>
      buildBranchTreeRows({
        refs: visibleRefs,
        collapsed,
        headBranch,
        headUpstream,
        checkedOutByBranch,
        includeAllBranchesRow,
      }),
    [visibleRefs, collapsed, headBranch, headUpstream, checkedOutByBranch, includeAllBranchesRow]
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
      // §7.3：F2 对焦点 ref 行请求 Rename（事件目标即焦点行）。group/synthetic 或无行 → 吞掉。
      if (event.key === 'F2') {
        event.preventDefault()
        const indexed = Number((event.target as HTMLElement).dataset.rowIndex)
        const hit = Number.isNaN(indexed) ? undefined : rows[indexed]
        if (hit?.kind === 'ref') onRenameRequest?.(hit)
        return
      }
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
    [rows, focusRow, onRenameRequest]
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
          ref={inputRef}
          type="search"
          value={query}
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
            renderRowMenu={renderRowMenu}
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
  renderRowMenu,
}: {
  row: BranchTreeRow
  index: number
  active: boolean
  onActivate: (row: BranchTreeRow) => void
  onToggleGroup: (id: string) => void
  renderRowMenu?: (row: BranchTreeRow, content: React.ReactNode) => React.ReactNode
}): React.JSX.Element {
  const onClick =
    row.kind === 'group' ? () => onToggleGroup(row.id) : () => onActivate(row)

  const button = (
    <button
      type="button"
      role="treeitem"
      // Why: 行是扁平兄弟（靠 paddingLeft 表达层级），子行不在父 treeitem 的
      // role="group" 里。不加 aria-level 的话，屏幕阅读器只能读到一串层级不明的
      // treeitem，aria-expanded 与视觉层级对不上。扁平结构是为 ↑↓ 导航服务的，
      // 因此补 aria-level 而不是改成嵌套 group。
      aria-level={row.depth + 1}
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

  // 接缝只作用 ref 行；其余（group/synthetic/无接缝）逐字节走原 <button>。
  // data-row-index 始终留在 button 上，既有 ↑↓ 导航 querySelector('[data-row-index]')?.focus() 口径不变。
  if (renderRowMenu && row.kind === 'ref') return <>{renderRowMenu(row, button)}</>
  return button
}

/**
 * 行首图标映射（spec §7.1）：🏷=HEAD 分支、★=当前分支的 upstream、▣=被某 ade workspace 检出。
 * 增量5 后续：用 lucide 线性图标而非裸 unicode/emoji（🏷 在 macOS 渲染为彩色 emoji，与宿主其余 lucide 风格不一致）。
 * 尺寸 size-3 适配 ~20px 行高；w-3 容器与旧字形的列宽一致。
 */
function RowLeading({ row }: { row: BranchTreeRow }): React.JSX.Element {
  if (row.kind === 'group') {
    const Icon = row.expanded ? ChevronDown : ChevronRight
    return (
      <span aria-hidden="true" className="flex w-3 shrink-0 justify-center text-muted-foreground">
        <Icon className="size-3" />
      </span>
    )
  }
  if (row.kind === 'synthetic') {
    return (
      <span aria-hidden="true" className="flex w-3 shrink-0 justify-center text-muted-foreground">
        <CircleDot className="size-3" />
      </span>
    )
  }
  if (row.ref.category === 'tag') {
    return (
      <span aria-hidden="true" className="flex w-3 shrink-0 justify-center text-muted-foreground">
        <Tag className="size-3" />
      </span>
    )
  }
  if (row.ref.isHead) {
    return (
      <span aria-hidden="true" className="flex w-3 shrink-0 justify-center text-muted-foreground">
        <GitBranch className="size-3" />
      </span>
    )
  }
  if (row.isUpstreamOfHead) {
    return (
      <span
        aria-hidden="true"
        data-testid="branch-upstream-star"
        className="flex w-3 shrink-0 justify-center text-muted-foreground"
      >
        <Star className="size-3" />
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
      className="flex shrink-0 items-center text-muted-foreground"
      aria-hidden="true"
    >
      <Boxes className="size-3" />
    </span>
  ) : null
  const { ahead, behind } = row.ref
  if (ahead == null && behind == null) return badge
  return (
    <>
      {badge}
      <span
        className="flex shrink-0 items-center gap-1 tabular-nums text-[10px] text-muted-foreground"
        aria-hidden="true"
      >
        {/* 单边缺省时只渲染该边。`?? 0` 会把「仅领先」显示成 `↑2 ↓0`，
            与同一行的 tooltip（走 aheadOnlyLabel → "2 ahead"）口径不一致，
            且 `↓0` 会被读成「落后 0」的误导。
            各自包 span：`gap-1` 只作用于**元素**子节点，两个裸文本节点会黏成 `↑2↓1`。 */}
        {ahead != null ? <span>{`↑${ahead}`}</span> : null}
        {behind != null ? <span>{`↓${behind}`}</span> : null}
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
  // 远程条目已按 `<remote>` 二级组分组（组头就是 remote 名），组内再显示 `origin/` 前缀冗余 → 剥掉。
  // 仅影响显示；行 id（fullName）、排序（compareName 用全名）、tooltip 口径不变。
  if (row.ref.category === 'remote') {
    const prefix = row.ref.remote ?? row.ref.name.split('/')[0] ?? ''
    const head = `${prefix}/`
    if (prefix && row.ref.name.startsWith(head)) return row.ref.name.slice(head.length)
  }
  return row.ref.name
}

function rowTitle(row: BranchTreeRow): string | undefined {
  if (row.kind === 'ref') {
    const { ahead, behind, upstream } = row.ref
    // 计数 tooltip 与 ▣ tooltip **不互斥**：旧写法在 owner 分支提前 return，
    // 让既有 ▣ 又显示 `↑2 ↓1` 的行丢掉计数解释 —— 同一行内已渲染的信息被覆盖。
    const parts: string[] = []
    if (ahead != null && behind != null) {
      parts.push(
        translate(
          'auto.components.git-branch.aheadBehindLabel',
          '{{ahead}} ahead, {{behind}} behind',
          { ahead, behind }
        )
      )
    } else if (ahead != null) {
      parts.push(
        translate('auto.components.git-branch.aheadOnlyLabel', '{{ahead}} ahead', { ahead })
      )
    } else if (behind != null) {
      parts.push(
        translate('auto.components.git-branch.behindOnlyLabel', '{{behind}} behind', { behind })
      )
    } else if (upstream) {
      // 有 upstream 且无偏差 = 同步。旧写法 `return upstream` 把裸 refname 当 tooltip，
      // 信息量为零，也让 inSyncLabel 沦为死 key。
      parts.push(
        translate('auto.components.git-branch.inSyncLabel', 'in sync with {{upstream}}', {
          upstream,
        })
      )
    }
    if (row.checkedOutBy) {
      parts.push(
        translate('auto.components.git-branch.checkedOutIn', 'checked out in {{workspace}}', {
          // spec §7.1 要求 tooltip 给 displayName **+ path**：多仓同名 workspace 时
          // 只给名字分不开。
          workspace: `${row.checkedOutBy.displayName} (${row.checkedOutBy.path})`,
        })
      )
    }
    return parts.length ? parts.join(' · ') : undefined
  }
  if (row.kind === 'group') return `${row.labelFallback} (${row.childCount})`
  return undefined
}
