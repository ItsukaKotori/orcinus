import { describe, expect, it } from 'vitest'
import { buildFixtureSnapshot } from '../fixtures/fixture-snapshot'

const isOid = (s: string) => /^[0-9a-f]{40}$/.test(s)

describe('fixture snapshot', () => {
  const snap = buildFixtureSnapshot()

  it('至少 30 提交且 id 为 40-hex、parentIds 均存在于集合', () => {
    expect(snap.items.length).toBeGreaterThanOrEqual(30)
    const ids = new Set(snap.items.map((i) => i.id))
    for (const item of snap.items) {
      expect(isOid(item.id)).toBe(true)
      for (const pid of item.parentIds) expect(ids.has(pid)).toBe(true)
    }
  })

  it('包含一个 merge（2 parents）、一个 tag、remote ref、ahead/behind 标志', () => {
    expect(snap.items.some((i) => i.parentIds.length >= 2)).toBe(true)
    expect(snap.items.some((i) => (i.references ?? []).some((r) => r.category === 'tags'))).toBe(true)
    expect(snap.remoteRef?.category).toBe('remote branches')
    expect(typeof snap.hasIncomingChanges).toBe('boolean')
    expect(typeof snap.hasOutgoingChanges).toBe('boolean')
  })

  it('items 按 topo 序：每个 item 的 parent 出现在其后', () => {
    const order = new Map(snap.items.map((i, idx) => [i.id, idx]))
    for (const item of snap.items) {
      for (const pid of item.parentIds) expect(order.get(pid)!).toBeGreaterThan(order.get(item.id)!)
    }
  })
})
