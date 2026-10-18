import { useCallback, useEffect, useMemo, useState } from 'react'
import type { GitBranchChangeEntry } from '@shared/git-diff-compare-types'
import type { GitHistoryItemRef } from '@shared/git-history-types'
import type { GitLogRepository } from '../domain/types'
import { buildGraphRows, type GraphRow } from '../graph/git-graph-model'
import { selectVisibleRows } from './select-rows'
import { Toolbar } from './Toolbar'
import { RefPanel } from './RefPanel'
import { GraphList } from './GraphList'
import { CommitDetail } from './CommitDetail'

function isBoundaryRow(row: GraphRow): boolean {
  return row.viewModel.kind === 'incoming-changes' || row.viewModel.kind === 'outgoing-changes'
}

export function GitLogView({ repository }: { repository: GitLogRepository }): React.JSX.Element {
  const [rows, setRows] = useState<GraphRow[]>([])
  const [refs, setRefs] = useState<GitHistoryItemRef[]>([])
  const [branch, setBranch] = useState('')
  const [ahead, setAhead] = useState(0)
  const [behind, setBehind] = useState(0)
  const [query, setQuery] = useState('')
  const [activeRef, setActiveRef] = useState<string | null>(null)
  const [selected, setSelected] = useState<string | null>(null)
  const [files, setFiles] = useState<GitBranchChangeEntry[]>([])

  const load = useCallback(async () => {
    const snap = await repository.loadSnapshot()
    const next = buildGraphRows(snap)
    setRows(next)
    setBranch(snap.currentRef?.name ?? 'HEAD')
    setAhead(snap.hasOutgoingChanges ? 1 : 0)
    setBehind(snap.hasIncomingChanges ? 1 : 0)
    const seen = new Map<string, GitHistoryItemRef>()
    for (const item of snap.items) for (const r of item.references ?? []) seen.set(r.id, r)
    setRefs([...seen.values()])
    setSelected((cur) => cur ?? next.find((r) => !isBoundaryRow(r))?.viewModel.historyItem.id ?? null)
  }, [repository])

  useEffect(() => {
    void load()
  }, [load])

  useEffect(() => {
    if (!selected) {
      setFiles([])
      return
    }
    let alive = true
    repository.loadCommitFiles(selected).then((f) => {
      if (alive) setFiles(f)
    })
    return () => {
      alive = false
    }
  }, [selected, repository])

  // 拓扑几何在全量 rows 上计算（稳定）；过滤仅影响可见子集。
  const visible = useMemo(() => selectVisibleRows(rows, query, activeRef), [rows, query, activeRef])

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
    [visible, selected]
  )

  const selectedItem = rows.find((r) => r.viewModel.historyItem.id === selected)?.viewModel.historyItem ?? null

  return (
    <div className="flex h-screen flex-col bg-background text-foreground" tabIndex={0} onKeyDown={onKey}>
      <Toolbar branch={branch} query={query} onQuery={setQuery} localCount={ahead} remoteCount={behind} onRefresh={() => void load()} />
      <div className="flex min-h-0 flex-1">
        <RefPanel refs={refs} activeRef={activeRef} onPick={setActiveRef} />
        <GraphList rows={visible} selectedOid={selected} onSelect={onSelect} />
      </div>
      <CommitDetail item={selectedItem} files={files} />
    </div>
  )
}
