import type { GraphRow } from '../graph/git-graph-model'

export function selectVisibleRows(rows: GraphRow[], query: string, activeRef: string | null = null): GraphRow[] {
  const q = query.trim().toLowerCase()
  return rows.filter((r) => {
    const it = r.viewModel.historyItem
    if (activeRef && !(it.references ?? []).some((x) => x.id === activeRef)) return false
    if (!q) return true
    return it.subject.toLowerCase().includes(q) || (it.author ?? '').toLowerCase().includes(q)
  })
}
