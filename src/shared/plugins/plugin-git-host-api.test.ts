import { describe, expect, it } from 'vitest'
import { PLUGIN_HOST_API_V0 } from './plugin-host-api'
import { PLUGIN_CAPABILITY_KINDS } from './plugin-capabilities'

const byName = (n: string) => PLUGIN_HOST_API_V0.find((m) => m.name === n)

describe('git:read host surface', () => {
  it('capability kind 含 git:read', () => {
    expect(PLUGIN_CAPABILITY_KINDS as readonly string[]).toContain('git:read')
  })
  it('git.log panel-callable、capability=git:read、limit 上限 200', () => {
    const m = byName('git.log')
    expect(m).toMatchObject({ panel: true, capability: 'git:read', scope: 'active-worktree', mutation: false })
    expect(m!.params.safeParse({ limit: 200 }).success).toBe(true)
    expect(m!.params.safeParse({ limit: 201 }).success).toBe(false)
    expect(m!.params.safeParse({}).success).toBe(true)
  })
  it('git.commitFiles oid 长度 7..40', () => {
    const m = byName('git.commitFiles')
    expect(m).toMatchObject({ panel: true, capability: 'git:read', mutation: false })
    expect(m!.params.safeParse({ oid: 'abc' }).success).toBe(false)
    expect(m!.params.safeParse({ oid: 'a'.repeat(40) }).success).toBe(true)
  })
})
