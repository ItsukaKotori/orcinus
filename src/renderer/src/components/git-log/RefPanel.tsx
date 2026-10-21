import type { GitHistoryItemRef } from '../../../../shared/git-history-types'

export function groupRefs(refs: GitHistoryItemRef[]): Record<string, GitHistoryItemRef[]> {
  const out: Record<string, GitHistoryItemRef[]> = {}
  for (const r of refs) (out[r.category ?? 'commits'] ??= []).push(r)
  return out
}

type Props = {
  refs: GitHistoryItemRef[]
  /** True when the host capped the ref list; shows a partial-results note. */
  truncated: boolean
  /** Current backend range: undefined = HEAD, 'all' = every ref, else a revision name. */
  activeName: 'head' | 'all' | (string & {}) | null | undefined
  onPickName: (name: 'head' | 'all' | (string & {}) | undefined) => void
}

export function RefPanel({ refs, truncated, activeName, onPickName }: Props): React.JSX.Element {
  const groups = groupRefs(refs)
  const order = ['branches', 'remote branches', 'tags'] as const
  const current = activeName == null || activeName === 'head'
  return (
    <div className="w-44 shrink-0 overflow-auto border-r border-border p-2 text-xs">
      <div className="mb-2 flex gap-1">
        <button
          type="button"
          onClick={() => onPickName(undefined)}
          className={`flex-1 rounded px-1.5 py-0.5 ${current ? 'bg-accent text-foreground' : 'text-muted-foreground hover:bg-accent/40'}`}
        >
          Current
        </button>
        <button
          type="button"
          onClick={() => onPickName('all')}
          className={`flex-1 rounded px-1.5 py-0.5 ${activeName === 'all' ? 'bg-accent text-foreground' : 'text-muted-foreground hover:bg-accent/40'}`}
        >
          All
        </button>
      </div>
      {truncated && (
        <p className="mb-2 text-[11px] leading-4 text-muted-foreground">
          Some refs are not shown (list limit reached).
        </p>
      )}
      {order.map((cat) => (groups[cat]?.length ? (
        <div key={cat} className="mb-2">
          <div className="mb-1 uppercase tracking-wide text-muted-foreground">{cat}</div>
          {groups[cat]!.map((r) => (
            <button
              key={r.id}
              type="button"
              onClick={() => onPickName(activeName === r.name ? undefined : r.name)}
              className={`block w-full truncate rounded px-1.5 py-0.5 text-left ${activeName === r.name ? 'bg-accent text-foreground' : 'hover:bg-accent/40 text-muted-foreground'}`}
            >
              {r.name}
            </button>
          ))}
        </div>
      ) : null))}
    </div>
  )
}
