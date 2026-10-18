import type { ChangeEvent } from 'react'

type Props = {
  branch: string
  query: string
  onQuery: (v: string) => void
  remoteCount: number
  localCount: number
  onRefresh: () => void
}

export function Toolbar({ branch, query, onQuery, remoteCount, localCount, onRefresh }: Props): React.JSX.Element {
  return (
    <div className="flex items-center gap-2 border-b border-border px-3 py-2 text-sm">
      <span className="font-medium text-foreground">Git Log</span>
      <span className="rounded bg-secondary px-1.5 py-0.5 text-xs text-secondary-foreground">{branch}</span>
      <span className="text-xs text-muted-foreground">↑{localCount} ↓{remoteCount}</span>
      <input
        data-testid="search"
        value={query}
        placeholder="Filter commits…"
        onChange={(e: ChangeEvent<HTMLInputElement>) => onQuery(e.target.value)}
        className="ml-auto w-48 rounded border border-input bg-transparent px-2 py-1 text-xs text-foreground outline-none focus:ring-1 focus:ring-ring"
      />
      <button type="button" onClick={onRefresh} className="rounded border border-border px-2 py-1 text-xs hover:bg-accent">Refresh</button>
    </div>
  )
}
