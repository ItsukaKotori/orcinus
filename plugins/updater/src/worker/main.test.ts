import { describe, expect, it, vi } from 'vitest'
import { createUpdater, registerCommands, type CommandContext, type OrcaSdk } from './main'

type Handler = (args: unknown, ctx?: CommandContext) => unknown

function createFakeOrca() {
  const handlers = new Map<string, Handler>()
  const hostCalls: Array<{ method: string; params: unknown }> = []
  const orca: OrcaSdk = {
    commands: {
      register: (id, handler) => {
        handlers.set(id, handler)
      }
    },
    host: {
      call: async (method, params) => {
        hostCalls.push({ method, params })
        if (method === 'storage.get') return { value: null }
        if (method === 'storage.set') return { ok: true }
        if (method === 'session.post') return { ok: true }
        if (method === 'workspace.openTab') return { instanceId: 'i1', worktreeId: 'wt' }
        throw new Error(`unexpected host call: ${method}`)
      }
    },
    log: () => {},
    events: { on: () => {} }
  }
  return { orca, handlers, hostCalls }
}

describe('registerCommands', () => {
  it('registers the six updater commands and remembers the calling session', async () => {
    const { orca, handlers, hostCalls } = createFakeOrca()
    const updater = {
      rememberSession: vi.fn(),
      check: vi.fn(async () => ({ started: true, running: false })),
      updateAll: vi.fn(async () => ({ started: true, running: false })),
      cancel: vi.fn(async () => ({ canceled: false })),
      snapshot: vi.fn(async () => ({ managers: [], state: {} })),
      setManagers: vi.fn(async () => ({ ok: true as const }))
    }
    registerCommands(orca, updater as never)
    expect([...handlers.keys()].sort()).toEqual([
      'cancel',
      'check',
      'open',
      'set-managers',
      'snapshot',
      'update-all'
    ])

    const ctx: CommandContext = {
      session: { token: 'tok-1', post: async () => ({ ok: true }) }
    }
    await handlers.get('check')!(undefined, ctx)
    expect(updater.rememberSession).toHaveBeenCalledWith('tok-1')
    expect(updater.check).toHaveBeenCalled()

    await handlers.get('set-managers')!({ managers: ['npm', 42] }, ctx)
    expect(updater.setManagers).toHaveBeenCalledWith(['npm'])

    await handlers.get('open')!(undefined, ctx)
    expect(hostCalls.some((call) => call.method === 'workspace.openTab')).toBe(true)
  })
})

describe('createUpdater', () => {
  it('routes storage through the orca host API', async () => {
    const { orca, hostCalls } = createFakeOrca()
    const updater = createUpdater(orca)
    await expect(updater.setManagers(['npm'])).resolves.toEqual({ ok: true })
    expect(
      hostCalls.some(
        (call) => call.method === 'storage.set' && (call.params as { key?: string }).key === 'managers'
      )
    ).toBe(true)
  })
})
