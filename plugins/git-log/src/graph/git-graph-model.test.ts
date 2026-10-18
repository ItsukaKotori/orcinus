import { describe, expect, it } from 'vitest'
import { buildGraphRows, graphColorVar, graphLaneCellCount } from './git-graph-model'
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

  it('全部 8 个 GitHistoryGraphColorId 映射到对应 --git-graph-* CSS 变量', () => {
    const ids = [
      'git-graph-ref',
      'git-graph-remote-ref',
      'git-graph-base-ref',
      'git-graph-lane-1',
      'git-graph-lane-2',
      'git-graph-lane-3',
      'git-graph-lane-4',
      'git-graph-lane-5'
    ] as const
    for (const id of ids) expect(graphColorVar(id)).toBe(`var(--${id})`)
  })

  it('泳道宽度盖住输入/输出泳道（顶端合并分叉线不被裁）', () => {
    const merge = rows.find((r) => r.viewModel.historyItem.subject.startsWith('Merge branch'))!
    expect(graphLaneCellCount(merge)).toBeGreaterThanOrEqual(merge.viewModel.outputSwimlanes.length)
    for (const r of rows) expect(graphLaneCellCount(r)).toBeGreaterThan(r.laneIndex)
  })
})
