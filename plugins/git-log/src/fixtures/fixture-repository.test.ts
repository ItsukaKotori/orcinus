import { describe, expect, it } from 'vitest'
import { FixtureRepository } from './fixture-repository'
import { gitRefsSchema } from '../domain/types'

describe('FixtureRepository', () => {
  it('loadRefs returns the branch / remote / tag refs from the snapshot', async () => {
    const refs = await new FixtureRepository().loadRefs()
    const names = refs.map((r) => r.name)
    expect(names).toContain('main')
    expect(names).toContain('feature')
    // De-duplicated by id.
    expect(new Set(refs.map((r) => r.id)).size).toBe(refs.length)
    // Locks the gitRefsSchema against the fixture shape (§6 TS contract).
    expect(() => gitRefsSchema.parse(refs)).not.toThrow()
  })

  it('loadSnapshot at offset > 0 yields an empty terminal page', async () => {
    const snap = await new FixtureRepository().loadSnapshot({}, 1)
    expect(snap.items).toHaveLength(0)
    expect(snap.hasMore).toBe(false)
  })
})
