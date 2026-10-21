import { describe, expect, it } from 'vitest'
import type { GitHistoryItem, GitHistoryResult } from '../../../../shared/git-history-types'
import { logParams, mergePages, PAGE_SIZE, type GitLogFilter } from './query'

const item = (id: string): GitHistoryItem => ({ id, parentIds: [], subject: id, message: id })
const page = (items: GitHistoryItem[], hasMore = false): GitHistoryResult => ({
  items,
  hasIncomingChanges: false,
  hasOutgoingChanges: false,
  hasMore,
  limit: PAGE_SIZE,
})

describe('query', () => {
  it('logParams always sets skip/limit and omits empty filter fields', () => {
    // toStrictEqual: no stray `undefined` keys when the filter is empty.
    expect(logParams({}, 0)).toStrictEqual({ skip: 0, limit: PAGE_SIZE })
    const filter: GitLogFilter = {
      refs: 'all',
      author: 'Lin',
      since: '2026-01-01',
      until: '2026-02-01',
      grep: 'fix',
      path: 'src/a.ts',
    }
    expect(logParams(filter, 100)).toStrictEqual({ skip: 100, limit: PAGE_SIZE, ...filter })
  })

  it('mergePages appends a page, de-dupes by id, keeps the latest metadata', () => {
    const next = page([item('b'), item('c')], true)
    const merged = mergePages([item('a'), item('b')], next)
    expect(merged.items.map((i) => i.id)).toEqual(['a', 'b', 'c'])
    expect(merged.hasMore).toBe(true)
    expect(merged.limit).toBe(next.limit)
  })
})
