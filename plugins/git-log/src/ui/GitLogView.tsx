import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import type { GitBranchChangeEntry } from '@shared/git-diff-compare-types'
import type { GitHistoryItemRef, GitHistoryResult } from '@shared/git-history-types'
import type { GitLogRepository } from '../domain/types'
import { PAGE_SIZE, mergePages, type GitLogFilter } from '../domain/query'
import { buildGraphRows, type GraphRow } from '../graph/git-graph-model'
import { selectVisibleRows } from './select-rows'
import { Toolbar } from './Toolbar'
import { RefPanel } from './RefPanel'
import { GraphList } from './GraphList'
import { CommitDetail } from './CommitDetail'

function isBoundaryRow(row: GraphRow): boolean {
  return row.viewModel.kind === 'incoming-changes' || row.viewModel.kind === 'outgoing-changes'
}

/** Coalesce rapid filter edits into one backend query per pause. The panel host
 *  rate-limits calls (30 / 10 s), so "type → immediate re-query" would storm it. */
const FILTER_DEBOUNCE_MS = 300

export function GitLogView({ repository }: { repository: GitLogRepository }): React.JSX.Element {
  const [filter, setFilter] = useState<GitLogFilter>({})
  const [debounced, setDebounced] = useState<GitLogFilter>({})
  const [result, setResult] = useState<GitHistoryResult | null>(null)
  const [refs, setRefs] = useState<GitHistoryItemRef[]>([])
  const [branch, setBranch] = useState('')
  const [ahead, setAhead] = useState(0)
  const [behind, setBehind] = useState(0)
  const [query, setQuery] = useState('')
  const [selected, setSelected] = useState<string | null>(null)
  const [files, setFiles] = useState<GitBranchChangeEntry[]>([])
  const [status, setStatus] = useState<'loading' | 'ready' | 'error'>('loading')
  const [error, setError] = useState<string | null>(null)
  const [skip, setSkip] = useState(PAGE_SIZE)
  const [hasMore, setHasMore] = useState(false)
  const [loadingMore, setLoadingMore] = useState(false)
  // Latest filter the in-flight page belongs to, so a late response from a
  // superseded query can't overwrite newer results.
  const requestSeq = useRef(0)

  const applyFilter = useCallback((patch: Partial<GitLogFilter>) => {
    setFilter((cur) => ({ ...cur, ...patch }))
  }, [])

  // Debounce filter → the query that actually hits the backend.
  useEffect(() => {
    const t = setTimeout(() => setDebounced(filter), FILTER_DEBOUNCE_MS)
    return () => clearTimeout(t)
  }, [filter])

  const reload = useCallback(
    async (f: GitLogFilter) => {
      const seq = ++requestSeq.current
      setStatus('loading')
      setError(null)
      try {
        const snap = await repository.loadSnapshot(f, 0)
        if (seq !== requestSeq.current) return
        setResult(snap)
        setBranch(snap.currentRef?.name ?? 'HEAD')
        setAhead(snap.hasOutgoingChanges ? 1 : 0)
        setBehind(snap.hasIncomingChanges ? 1 : 0)
        setSkip(PAGE_SIZE)
        setHasMore(snap.hasMore)
        setStatus('ready')
      } catch (e) {
        if (seq !== requestSeq.current) return
        setError(e instanceof Error ? e.message : 'failed to load git history')
        setStatus('error')
      }
    },
    [repository],
  )

  useEffect(() => {
    void reload(debounced)
  }, [debounced, reload])

  // Refs load once; a failure (or an over-large ref list) degrades the picker to
  // the built-in Current / All ranges rather than blocking the log.
  useEffect(() => {
    let alive = true
    repository
      .loadRefs()
      .then((r) => {
        if (alive) setRefs(r)
      })
      .catch(() => {
        if (alive) setRefs([])
      })
    return () => {
      alive = false
    }
  }, [repository])

  const loadMore = useCallback(async () => {
    if (loadingMore || !hasMore || !result) return
    setLoadingMore(true)
    const seq = requestSeq.current
    try {
      const next = await repository.loadSnapshot(debounced, skip)
      if (seq !== requestSeq.current) return // a filter reload started meanwhile
      const merged = mergePages(result.items, next)
      setResult(merged)
      setSkip((s) => s + PAGE_SIZE)
      setHasMore(next.hasMore)
    } catch {
      // Keep what's loaded; stop paging rather than erroring out the view.
      setHasMore(false)
    } finally {
      setLoadingMore(false)
    }
  }, [loadingMore, hasMore, result, repository, debounced, skip])

  const rows = useMemo(() => (result ? buildGraphRows(result) : []), [result])

  // Re-elect a selection when the loaded set changes (keep it if still present).
  useEffect(() => {
    setSelected((cur) =>
      cur && rows.some((r) => r.viewModel.historyItem.id === cur)
        ? cur
        : rows.find((r) => !isBoundaryRow(r))?.viewModel.historyItem.id ?? null,
    )
  }, [rows])

  useEffect(() => {
    if (!selected) {
      setFiles([])
      return
    }
    let alive = true
    repository
      .loadCommitFiles(selected)
      .then((f) => {
        if (alive) setFiles(f)
      })
      .catch(() => {
        if (alive) setFiles([])
      })
    return () => {
      alive = false
    }
  }, [selected, repository])

  // Topology is computed over all loaded rows (stable); the search box narrows
  // the visible subject set client-side for instant feedback.
  const visible = useMemo(() => selectVisibleRows(rows, query), [rows, query])

  const onSelect = useCallback((oid: string) => setSelected(oid), [])
  const onKey = useCallback(
    (e: React.KeyboardEvent) => {
      if (e.key !== 'ArrowDown' && e.key !== 'ArrowUp') return
      e.preventDefault()
      const navigable = visible.filter((r) => !isBoundaryRow(r))
      const idx = navigable.findIndex((r) => r.viewModel.historyItem.id === selected)
      const nextIdx = e.key === 'ArrowDown' ? Math.min(navigable.length - 1, idx + 1) : Math.max(0, idx - 1)
      const nxt = navigable[nextIdx]
      if (nxt) setSelected(nxt.viewModel.historyItem.id)
    },
    [visible, selected],
  )

  const selectedItem = rows.find((r) => r.viewModel.historyItem.id === selected)?.viewModel.historyItem ?? null

  return (
    <div className="flex h-screen flex-col bg-background text-foreground" tabIndex={0} onKeyDown={onKey}>
      <Toolbar
        branch={branch}
        query={query}
        filter={filter}
        localCount={ahead}
        remoteCount={behind}
        onQuery={setQuery}
        onFilterChange={applyFilter}
        onRefresh={() => void reload(filter)}
      />
      <div className="flex min-h-0 flex-1">
        {status === 'error' ? (
          <div className="flex flex-1 items-center justify-center p-6 text-center text-sm text-muted-foreground">
            Git history unavailable: {error}
          </div>
        ) : status === 'loading' ? (
          <div className="flex flex-1 items-center justify-center text-sm text-muted-foreground">Loading commits…</div>
        ) : (
          <>
            <RefPanel refs={refs} activeName={filter.refs ?? null} onPickName={(name) => applyFilter({ refs: name })} />
            <GraphList
              rows={visible}
              selectedOid={selected}
              onSelect={onSelect}
              hasMore={hasMore}
              loadingMore={loadingMore}
              onLoadMore={() => void loadMore()}
            />
          </>
        )}
      </div>
      <CommitDetail item={selectedItem} files={files} />
    </div>
  )
}
