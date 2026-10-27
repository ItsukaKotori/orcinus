import { useMemo } from 'react'
import { MoreHorizontal } from 'lucide-react'
import { cn } from '@/lib/utils'
import { translate } from '@/i18n/i18n'
import {
  ContextMenu,
  ContextMenuTrigger,
  ContextMenuContent,
  ContextMenuItem,
  ContextMenuSub,
  ContextMenuSubTrigger,
  ContextMenuSubContent,
} from '@/components/ui/context-menu'
import {
  DropdownMenu,
  DropdownMenuTrigger,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSub,
  DropdownMenuSubTrigger,
  DropdownMenuSubContent,
} from '@/components/ui/dropdown-menu'
import type { BranchTreeRow } from './branch-tree-model'
import { BRANCH_ACTIONS, type BranchActionContext, type BranchActionId } from './branch-actions'

export type BranchRowMenuProps = {
  row: BranchTreeRow
  ctx: BranchActionContext
  /** 宿主声明的可用动作 id 集合（spec §13.6：裁剪机制从「不传某回调」扩成「声明 id 集合」）。 */
  allowedActionIds: ReadonlySet<BranchActionId>
  /** 行的可点主体（树经 renderRowMenu 传入的 content）；同时作右键触发区。 */
  children: React.ReactNode
  className?: string
}

/** 已过滤 + 判定后的菜单条目：叶子项，或带子项的展开区（Set Upstream To…）。 */
type MenuEntry =
  | { kind: 'item'; id: string; label: string; enabled: boolean; run: () => void }
  | { kind: 'sub'; id: string; label: string; sub: { id: string; label: string; enabled: boolean; run: () => void }[] }

/**
 * 分支行的右键（ContextMenu）+ 行尾 `⋯`（DropdownMenu）菜单。
 * 两个 Radix 浮层各用自己的 Item/Sub 原语（混用会把 ContextMenu.Item 渲染到无 Root 的上下文），
 * 故共享的是「过滤 + 判定 + submenu 展开」算出的 entries，而不是共享 Content 节点。
 * 没有任何可见项时**不渲染 `⋯`**：一个打不开的空菜单比没有更糟。
 */
export function BranchRowMenu({
  row,
  ctx,
  allowedActionIds,
  children,
  className,
}: BranchRowMenuProps): React.JSX.Element {
  const entries = useMemo<MenuEntry[]>(
    () =>
      BRANCH_ACTIONS.filter((a) => allowedActionIds.has(a.id) && a.visible(row, ctx)).map((a) => {
        if (a.submenu) {
          return {
            kind: 'sub',
            id: a.id,
            label: a.label(),
            sub: a.submenu(row, ctx).map((s) => ({
              id: s.id,
              label: s.label(),
              enabled: s.enabled,
              run: s.run,
            })),
          }
        }
        return {
          kind: 'item',
          id: a.id,
          label: a.label(),
          enabled: !a.enabled || a.enabled(row, ctx),
          run: () => a.run(row, ctx),
        }
      }),
    [allowedActionIds, ctx, row]
  )

  if (entries.length === 0) return <>{children}</>

  return (
    <div className={cn('flex w-full items-center gap-1', className)}>
      <ContextMenu>
        <ContextMenuTrigger asChild className="min-w-0 flex-1">
          {children}
        </ContextMenuTrigger>
        <ContextMenuContent>
          {entries.map((e) =>
            e.kind === 'item' ? (
              <ContextMenuItem
                key={e.id}
                disabled={!e.enabled}
                onSelect={() => {
                  if (e.enabled) e.run()
                }}
              >
                {e.label}
              </ContextMenuItem>
            ) : (
              <ContextMenuSub key={e.id}>
                <ContextMenuSubTrigger>{e.label}</ContextMenuSubTrigger>
                <ContextMenuSubContent>
                  {e.sub.map((s) => (
                    <ContextMenuItem
                      key={s.id}
                      disabled={!s.enabled}
                      onSelect={() => {
                        if (s.enabled) s.run()
                      }}
                    >
                      {s.label}
                    </ContextMenuItem>
                  ))}
                </ContextMenuSubContent>
              </ContextMenuSub>
            )
          )}
        </ContextMenuContent>
      </ContextMenu>
      <DropdownMenu>
        <DropdownMenuTrigger asChild>
          <button
            type="button"
            data-testid="branch-row-more"
            aria-label={translate('auto.components.git-branch.moreActions', 'More actions')}
            className="shrink-0 rounded p-0.5 text-muted-foreground hover:bg-accent/60"
          >
            <MoreHorizontal className="size-3.5" />
          </button>
        </DropdownMenuTrigger>
        <DropdownMenuContent align="end">
          {entries.map((e) =>
            e.kind === 'item' ? (
              <DropdownMenuItem
                key={e.id}
                disabled={!e.enabled}
                onSelect={() => {
                  if (e.enabled) e.run()
                }}
              >
                {e.label}
              </DropdownMenuItem>
            ) : (
              <DropdownMenuSub key={e.id}>
                <DropdownMenuSubTrigger>{e.label}</DropdownMenuSubTrigger>
                <DropdownMenuSubContent>
                  {e.sub.map((s) => (
                    <DropdownMenuItem
                      key={s.id}
                      disabled={!s.enabled}
                      onSelect={() => {
                        if (s.enabled) s.run()
                      }}
                    >
                      {s.label}
                    </DropdownMenuItem>
                  ))}
                </DropdownMenuSubContent>
              </DropdownMenuSub>
            )
          )}
        </DropdownMenuContent>
      </DropdownMenu>
    </div>
  )
}
