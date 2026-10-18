import { expect, it } from 'vitest'
import { selectVisibleRows } from './select-rows'
import { buildGraphRows } from '../graph/git-graph-model'
import { buildFixtureSnapshot } from '../fixtures/fixture-snapshot'

const rows = buildGraphRows(buildFixtureSnapshot())

it('无过滤返回全部行', () => expect(selectVisibleRows(rows, '', null).length).toBe(rows.length))
it('query 收窄到匹配 subject', () => {
  const feat = selectVisibleRows(rows, 'experiment', null)
  expect(feat.length).toBeGreaterThan(0)
  expect(feat.length).toBeLessThan(rows.length)
  expect(feat.every((r) => r.viewModel.historyItem.subject.includes('experiment'))).toBe(true)
})
it('activeRef 过滤到携带该 ref 的提交', () => {
  expect(selectVisibleRows(rows, '', 'refs/tags/v1.0.0').length).toBe(1)
})
