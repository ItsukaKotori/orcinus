import { describe, expect, it } from 'vitest'
import { buildGraphRows } from './git-graph-model'
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

  it('laneIndex 全部落在合理泳道范围（本 fixture 并发泳道 ≤ 6）', () => {
    for (const r of rows) expect(r.laneIndex).toBeGreaterThanOrEqual(0)
    expect(Math.max(...rows.map((r) => r.laneIndex))).toBeLessThanOrEqual(6)
  })

  it('视图 kind 含真实提交(node/HEAD)，供选中/导航过滤', () => {
    const kinds = new Set(rows.map((r) => r.viewModel.kind))
    expect([...kinds].some((k) => k === 'node' || k === 'HEAD')).toBe(true)
  })
})
