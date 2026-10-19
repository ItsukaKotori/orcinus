import { describe, expect, it } from 'vitest'
import type { GitHistoryItem, GitHistoryResult } from '@shared/git-history-types'
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
    expect(logParams({}, 0)).toEqual({ skip: 0, limit: PAGE_SIZE })
    const filter: GitLogFilter = { refs: 'all', author: 'Lin', path: 'src/a.ts' }
    expect(logParams(filter, 100)).toEqual({
      skip: 100,
      limit: PAGE_SIZE,
      refs: 'all',
      author: 'Lin',
      path: 'src/a.ts',
    })
  })

  it('mergePages appends a page, de-dupes by id, keeps the latest metadata', () => {
    const merged = mergePages([item('a'), item('b')], page([item('b'), item('c')], true))
    expect(merged.items.map((i) => i.id)).toEqual(['a', 'b', 'c'])
    expect(merged.hasMore).toBe(true)
  })
})
