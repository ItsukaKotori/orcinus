import type { GitHistoryItemRef } from '@shared/git-history-types'

export function groupRefs(refs: GitHistoryItemRef[]): Record<string, GitHistoryItemRef[]> {
  const out: Record<string, GitHistoryItemRef[]> = {}
  for (const r of refs) (out[r.category ?? 'commits'] ??= []).push(r)
  return out
}

type Props = { refs: GitHistoryItemRef[]; activeRef: string | null; onPick: (id: string | null) => void }

export function RefPanel({ refs, activeRef, onPick }: Props): React.JSX.Element {
  const groups = groupRefs(refs)
  const order = ['branches', 'remote branches', 'tags'] as const
  return (
    <div className="w-44 shrink-0 overflow-auto border-r border-border p-2 text-xs">
      {order.map((cat) => (groups[cat]?.length ? (
        <div key={cat} className="mb-2">
          <div className="mb-1 uppercase tracking-wide text-muted-foreground">{cat}</div>
          {groups[cat]!.map((r) => (
            <button
              key={r.id}
              type="button"
              onClick={() => onPick(activeRef === r.id ? null : r.id)}
              className={`block w-full truncate rounded px-1.5 py-0.5 text-left ${activeRef === r.id ? 'bg-accent text-foreground' : 'hover:bg-accent/40 text-muted-foreground'}`}
            >
              {r.name}
            </button>
          ))}
        </div>
      ) : null))}
    </div>
  )
}
