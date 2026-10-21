import type { GitHistoryItem, GitHistoryResult } from '../../../../shared/git-history-types'

/** Backend-driven filter for `git.log`. `refs` selects the range: omitted or
 *  `'head'` → current branch, `'all'` → every ref, otherwise a revision string. */
export interface GitLogFilter {
  refs?: 'head' | 'all' | (string & {})
  author?: string
  since?: string
  until?: string
  grep?: string
  path?: string
}

/** Commits requested per page. Kept under the host result budget. */
export const PAGE_SIZE = 100

/** Build the `git.log` host params from a filter + paging offset. Empty filter
 *  fields are omitted so the backend applies its own defaults (`refs` absent → HEAD). */
export function logParams(
  filter: GitLogFilter,
  skip: number,
  limit: number = PAGE_SIZE,
): Record<string, unknown> {
  const params: Record<string, unknown> = { skip, limit }
  if (filter.refs) params.refs = filter.refs
  if (filter.author) params.author = filter.author
  if (filter.since) params.since = filter.since
  if (filter.until) params.until = filter.until
  if (filter.grep) params.grep = filter.grep
  if (filter.path) params.path = filter.path
  return params
}

/** Append a freshly-loaded page after the items already shown, de-duplicating by
 *  commit id (offset pages can overlap if history moves between requests).
 *  Ahead/behind and ref metadata always come from the latest page. */
export function mergePages(existing: GitHistoryItem[], next: GitHistoryResult): GitHistoryResult {
  const seen = new Set(existing.map((item) => item.id))
  const appended = next.items.filter((item) => !seen.has(item.id))
  return { ...next, items: [...existing, ...appended] }
}
