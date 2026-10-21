import type { GraphRow } from './git-graph-model'

export function selectVisibleRows(rows: GraphRow[], query: string): GraphRow[] {
  const q = query.trim().toLowerCase()
  if (!q) return rows
  return rows.filter((r) => {
    const it = r.viewModel.historyItem
    return it.subject.toLowerCase().includes(q) || (it.author ?? '').toLowerCase().includes(q)
  })
}
