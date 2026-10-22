import { useState } from 'react'
import { translate } from '@/i18n/i18n'
import type { GitHistoryItemRef } from '../../../../shared/git-history-types'

export function groupRefs(refs: GitHistoryItemRef[]): Record<string, GitHistoryItemRef[]> {
  const out: Record<string, GitHistoryItemRef[]> = {}
  for (const r of refs) (out[r.category ?? 'commits'] ??= []).push(r)
  return out
}

// Why: the backend ref category is a stable machine key, but the section header
// shown to users must be localised — map each to a translate() call.
const GROUP_LABELS: Record<string, { key: string; fallback: string }> = {
  branches: { key: 'auto.components.git-log.groupBranches', fallback: 'Branches' },
  'remote branches': { key: 'auto.components.git-log.groupRemoteBranches', fallback: 'Remote branches' },
  tags: { key: 'auto.components.git-log.groupTags', fallback: 'Tags' }
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
  const [term, setTerm] = useState('')
  const needle = term.trim().toLowerCase()
  const groups = groupRefs(needle ? refs.filter((r) => r.name.toLowerCase().includes(needle)) : refs)
  const order = ['branches', 'remote branches', 'tags'] as const
  const current = activeName == null || activeName === 'head'
  return (
    <div className="w-44 shrink-0 overflow-auto border-r border-border p-2 text-xs">
      <input
        value={term}
        onChange={(e) => setTerm(e.target.value)}
        placeholder={translate('auto.components.git-log.filterBranches', 'Filter branches…')}
        className="mb-2 w-full rounded border border-input bg-transparent px-1.5 py-0.5 text-xs outline-none focus:ring-1 focus:ring-ring"
      />
      <div className="mb-2 flex gap-1">
        <button
          type="button"
          onClick={() => onPickName(undefined)}
          className={`flex-1 rounded px-1.5 py-0.5 ${current ? 'bg-accent text-foreground' : 'text-muted-foreground hover:bg-accent/40'}`}
        >
          {translate('auto.components.git-log.current', 'Current')}
        </button>
        <button
          type="button"
          onClick={() => onPickName('all')}
          className={`flex-1 rounded px-1.5 py-0.5 ${activeName === 'all' ? 'bg-accent text-foreground' : 'text-muted-foreground hover:bg-accent/40'}`}
        >
          {translate('auto.components.git-log.all', 'All')}
        </button>
      </div>
      {truncated && (
        <p className="mb-2 text-[11px] leading-4 text-muted-foreground">
          {translate('auto.components.git-log.refsTruncated', 'Some refs are not shown (list limit reached).')}
        </p>
      )}
      {order.map((cat) => (groups[cat]?.length ? (
        <div key={cat} className="mb-2">
          <div className="mb-1 uppercase tracking-wide text-muted-foreground">
            {translate(GROUP_LABELS[cat]!.key, GROUP_LABELS[cat]!.fallback)}
          </div>
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
