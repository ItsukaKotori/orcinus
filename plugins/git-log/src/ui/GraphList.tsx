import { useRef } from 'react'
import { useVirtualizer } from '@tanstack/react-virtual'
import { GitHistoryGraphSvg, graphColor } from '../../../../src/renderer/src/components/right-sidebar/source-control/sync/git-history-graph-svg'
import { dedupeRemoteTrackingRefs } from '@shared/git-history-ref-display'
import type { GraphRow } from '../graph/git-graph-model'
import { formatRelativeDate, shortHash } from './format'

type Props = {
  rows: GraphRow[]
  selectedOid: string | null
  onSelect: (oid: string) => void
  hasMore: boolean
  loadingMore: boolean
  loadMoreError: string | null
  onLoadMore: () => void
}

function isBoundary(row: GraphRow): boolean {
  const kind = row.viewModel.kind
  return kind === 'incoming-changes' || kind === 'outgoing-changes'
}

export function GraphList({ rows, selectedOid, onSelect, hasMore, loadingMore, loadMoreError, onLoadMore }: Props): React.JSX.Element {
  const parentRef = useRef<HTMLDivElement>(null)
  const virt = useVirtualizer({ count: rows.length, getScrollElement: () => parentRef.current, estimateSize: () => 24, overscan: 12 })
  const onScroll = (): void => {
    const el = parentRef.current
    if (!el || !hasMore || loadingMore) return
    if (el.scrollTop + el.clientHeight >= el.scrollHeight - 48) onLoadMore()
  }
  return (
    <div ref={parentRef} onScroll={onScroll} className="h-full min-h-0 flex-1 overflow-auto">
      <div style={{ height: virt.getTotalSize(), position: 'relative' }}>
        {virt.getVirtualItems().map((vi) => {
          const row = rows[vi.index]!
          const item = row.viewModel.historyItem
          const boundary = isBoundary(row)
          const selected = item.id === selectedOid
          const refs = dedupeRemoteTrackingRefs(item.references ?? [])
          const visibleRefs = refs.slice(0, 2)
          const hidden = refs.slice(2)
          return (
            <div
              key={item.id}
              data-testid="graph-row"
              role={boundary ? undefined : 'button'}
              tabIndex={boundary ? -1 : 0}
              onClick={boundary ? undefined : () => onSelect(item.id)}
              className={`flex items-center gap-2 px-2 text-xs ${boundary ? 'text-muted-foreground' : 'cursor-pointer ' + (selected ? 'bg-accent' : 'hover:bg-accent/40')}`}
              style={{ position: 'absolute', top: 0, left: 0, height: vi.size, width: '100%', transform: `translateY(${vi.start}px)` }}
            >
              <GitHistoryGraphSvg viewModel={row.viewModel} />
              <span className="font-mono text-muted-foreground w-16 shrink-0">{shortHash(item.displayId ?? item.id)}</span>
              <span className="truncate flex-1 text-foreground">{item.subject}</span>
              {visibleRefs.length > 0 && (
                <span className="flex shrink-0 items-center gap-1 overflow-hidden">
                  {visibleRefs.map((r) => (
                    <span
                      key={r.id}
                      title={r.category ? `${r.name} (${r.category})` : r.name}
                      className="max-w-[8rem] truncate rounded-full border px-1.5 py-0.5 text-[10px] leading-none"
                      style={{ borderColor: r.color ? graphColor(r.color) : 'var(--border)', color: r.color ? graphColor(r.color) : 'var(--muted-foreground)' }}
                    >
                      {r.name}
                    </span>
                  ))}
                  {hidden.length > 0 && <span title={hidden.map((r) => r.name).join(', ')} className="text-[10px] text-muted-foreground">+{hidden.length}</span>}
                </span>
              )}
              <span className="w-24 shrink-0 truncate text-muted-foreground">{item.author}</span>
              <span className="w-16 shrink-0 text-right text-muted-foreground">{formatRelativeDate(item.timestamp)}</span>
            </div>
          )
        })}
      </div>
      {(loadingMore || hasMore || loadMoreError) && (
        <div className="px-2 py-1 text-center text-xs text-muted-foreground">
          {loadingMore ? (
            'Loading more commits…'
          ) : loadMoreError ? (
            <span className="flex items-center justify-center gap-2">
              <span>Failed to load more: {loadMoreError}</span>
              <button type="button" onClick={onLoadMore} className="rounded border border-border px-2 py-0.5 hover:bg-accent">Retry</button>
            </span>
          ) : (
            <button type="button" onClick={onLoadMore} className="rounded border border-border px-2 py-0.5 hover:bg-accent">Load more</button>
          )}
        </div>
      )}
    </div>
  )
}
