import { useRef } from 'react'
import { useVirtualizer } from '@tanstack/react-virtual'
import type { GraphRow } from '../graph/git-graph-model'
import { GraphLane } from './GraphLane'
import { formatRelativeDate, shortHash } from './format'

type Props = {
  rows: GraphRow[]
  selectedOid: string | null
  onSelect: (oid: string) => void
}

export function GraphList({ rows, selectedOid, onSelect }: Props): React.JSX.Element {
  const parentRef = useRef<HTMLDivElement>(null)
  const virt = useVirtualizer({ count: rows.length, getScrollElement: () => parentRef.current, estimateSize: () => 24, overscan: 12 })
  return (
    <div ref={parentRef} className="h-full min-h-0 flex-1 overflow-auto">
      <div style={{ height: virt.getTotalSize(), position: 'relative' }}>
        {virt.getVirtualItems().map((vi) => {
          const row = rows[vi.index]!
          const item = row.viewModel.historyItem
          const selected = item.id === selectedOid
          return (
            <div
              key={item.id}
              data-testid="graph-row"
              role="button"
              tabIndex={0}
              onClick={() => onSelect(item.id)}
              className={`flex items-center gap-2 px-2 text-xs ${selected ? 'bg-accent' : 'hover:bg-accent/40'}`}
              style={{ position: 'absolute', top: 0, left: 0, height: vi.size, width: '100%', transform: `translateY(${vi.start}px)` }}
            >
              <GraphLane row={row} />
              <span className="font-mono text-muted-foreground w-16 shrink-0">{shortHash(item.id)}</span>
              <span className="truncate flex-1 text-foreground">{item.subject}</span>
              <span className="w-24 shrink-0 truncate text-muted-foreground">{item.author}</span>
              <span className="w-16 shrink-0 text-right text-muted-foreground">{formatRelativeDate(item.timestamp)}</span>
            </div>
          )
        })}
      </div>
    </div>
  )
}
