import type { ChangeEvent } from 'react'
import { translate } from '@/i18n/i18n'
import type { GitLogFilter } from './query'

type Props = {
  branch: string
  query: string
  filter: GitLogFilter
  remoteCount: number
  localCount: number
  onQuery: (v: string) => void
  onFilterChange: (patch: Partial<GitLogFilter>) => void
  onRefresh: () => void
}

const chip =
  'rounded border border-input bg-transparent px-2 py-1 text-xs text-foreground outline-none focus:ring-1 focus:ring-ring'

export function Toolbar({ branch, query, filter, remoteCount, localCount, onQuery, onFilterChange, onRefresh }: Props): React.JSX.Element {
  const text = (key: 'author' | 'grep' | 'path') => (e: ChangeEvent<HTMLInputElement>) =>
    onFilterChange({ [key]: e.target.value || undefined })
  const date = (key: 'since' | 'until') => (e: ChangeEvent<HTMLInputElement>) =>
    onFilterChange({ [key]: e.target.value || undefined })

  return (
    <div className="flex flex-col gap-2 border-b border-border px-3 py-2 text-sm">
      <div className="flex items-center gap-2">
        <span className="font-medium text-foreground">{translate('auto.components.git-log.title', 'Git Log')}</span>
        <span className="rounded bg-secondary px-1.5 py-0.5 text-xs text-secondary-foreground">{branch}</span>
        <span className="text-xs text-muted-foreground">↑{localCount} ↓{remoteCount}</span>
        <input
          data-testid="search"
          value={query}
          placeholder={translate('auto.components.git-log.filterVisible', 'Filter visible…')}
          onChange={(e) => onQuery(e.target.value)}
          className={`ml-auto w-48 ${chip}`}
        />
        <button type="button" onClick={onRefresh} className="rounded border border-border px-2 py-1 text-xs hover:bg-accent">{translate('auto.components.git-log.refresh', 'Refresh')}</button>
      </div>
      {/* Backend filters (debounced). Empty fields are omitted so the host applies defaults. */}
      <div className="flex flex-wrap items-center gap-2 text-xs">
        <input data-testid="filter-author" value={filter.author ?? ''} placeholder={translate('auto.components.git-log.author', 'Author')} onChange={text('author')} className={chip} />
        <input data-testid="filter-grep" value={filter.grep ?? ''} placeholder={translate('auto.components.git-log.grep', 'Message (grep)')} onChange={text('grep')} className={chip} />
        <input data-testid="filter-path" value={filter.path ?? ''} placeholder={translate('auto.components.git-log.path', 'Path')} onChange={text('path')} className={chip} />
        <label className="flex items-center gap-1 text-muted-foreground">
          {translate('auto.components.git-log.since', 'Since')}
          <input data-testid="filter-since" type="date" value={filter.since ?? ''} onChange={date('since')} className={chip} />
        </label>
        <label className="flex items-center gap-1 text-muted-foreground">
          {translate('auto.components.git-log.until', 'Until')}
          <input data-testid="filter-until" type="date" value={filter.until ?? ''} onChange={date('until')} className={chip} />
        </label>
      </div>
    </div>
  )
}
