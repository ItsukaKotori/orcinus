import { describe, expect, it } from 'vitest'
import { buildGraphRows, graphColorVar } from './git-graph-model'
import { buildFixtureSnapshot } from '../fixtures/fixture-snapshot'

describe('buildGraphRows', () => {
  const rows = buildGraphRows(buildFixtureSnapshot())

  it('行数 ≥ 提交数（shared 会补 incoming/outgoing 边界行）', () => {
    expect(rows.length).toBeGreaterThanOrEqual(30)
  })

  it('merge 提交有 ≥2 条 output 泳道', () => {
    const merge = rows.find((r) => r.viewModel.historyItem.subject.startsWith('Merge branch'))
    expect(merge?.viewModel.outputSwimlanes.length).toBeGreaterThanOrEqual(2)
  })

  it('laneIndex 全部 ≥ 0 且合理（≤ 提交数）', () => {
    for (const r of rows) expect(r.laneIndex).toBeGreaterThanOrEqual(0)
    expect(Math.max(...rows.map((r) => r.laneIndex))).toBeLessThanOrEqual(rows.length)
  })

  it('色 id 映射到 --git-graph-* CSS 变量', () => {
    expect(graphColorVar('git-graph-lane-1')).toBe('var(--git-graph-lane-1)')
    expect(graphColorVar('git-graph-ref')).toBe('var(--git-graph-ref)')
    expect(graphColorVar('git-graph-remote-ref')).toBe('var(--git-graph-remote-ref)')
  })
})
