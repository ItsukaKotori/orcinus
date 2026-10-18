import { describe, expect, it, vi } from 'vitest'
import type { ManagerAdapter } from './managers'
import type { OutdatedEntry, UpdaterState } from '../shared/types'
import { Updater, type UpdaterDeps } from './jobs'

function entry(name: string, overrides: Partial<OutdatedEntry> = {}): OutdatedEntry {
  return { manager: 'npm', kind: 'global', name, current: '1.0.0', latest: '2.0.0', ...overrides }
}

type FakeAdapterOptions = {
  detected?: boolean
  entries?: OutdatedEntry[]
  outdatedDelayMs?: number
  failUpgradeFor?: string
  failOutdated?: boolean
}

function fakeAdapter(id: 'npm' | 'brew', options: FakeAdapterOptions = {}): ManagerAdapter & { upgrades: string[] } {
  const upgrades: string[] = []
  const adapter = {
    id,
    label: id === 'npm' ? 'npm' : 'Homebrew',
    kinds: id === 'npm' ? (['global'] as const).slice() : (['formula', 'cask'] as const).slice(),
    upgrades,
    detect: async () => options.detected ?? true,
    outdated: async () => {
      if (options.outdatedDelayMs) {
        await new Promise((resolve) => setTimeout(resolve, options.outdatedDelayMs))
      }
      if (options.failOutdated) {
        throw new Error(`${id} outdated exploded`)
      }
      return {
        entries: options.entries ?? [],
        raw: { exitCode: 0, signal: null, stdout: '', stderr: '' }
      }
    },
    upgradeArgs: () => [],
    upgrade: async (item: OutdatedEntry) => {
      upgrades.push(item.name)
      if (options.failUpgradeFor === item.name) {
        return { exitCode: 1, signal: null, stdout: '', stderr: `${item.name} exploded` }
      }
      return { exitCode: 0, signal: null, stdout: '', stderr: '' }
    }
  }
  return adapter as unknown as ManagerAdapter & { upgrades: string[] }
}

function harness(adapters: ManagerAdapter[], options: { sessions?: string[] } = {}) {
  const storage = new Map<string, unknown>()
  const posts: Array<{ token: string; topic: string; payload: unknown }> = []
  const failingTokens = new Set<string>()
  let clock = 1_000
  const deps: UpdaterDeps = {
    adapters,
    storageGet: async (key) => storage.get(key) ?? null,
    storageSet: async (key, value) => {
      storage.set(key, value)
    },
    postTo: async (token, topic, payload) => {
      if (failingTokens.has(token)) {
        const error = new Error('session is not available') as Error & { code?: string }
        error.code = 'unavailable'
        throw error
      }
      posts.push({ token, topic, payload })
    },
    now: () => (clock += 10),
    stateCoalesceMs: 0,
    logFlushMs: 0
  }
  const updater = new Updater(deps)
  for (const token of options.sessions ?? []) {
    updater.rememberSession(token)
  }
  return { updater, posts, storage, failingTokens }
}

describe('Updater.check', () => {
  it('detects managers, collects rows, and broadcasts the report', async () => {
    const npm = fakeAdapter('npm', { entries: [entry('a'), entry('b')] })
    const brew = fakeAdapter('brew', { entries: [entry('n', { manager: 'brew', kind: 'formula' })] })
    const { updater, posts } = harness([npm, brew], { sessions: ['tok-1'] })

    await expect(updater.check()).resolves.toEqual({ started: true, running: false })
    // 任务异步执行：轮询直到 phase 离开 checking
    await vi.waitFor(() => {
      const last = [...posts].reverse().find((post) => post.topic === 'state')
      const state = last?.payload as UpdaterState
      expect(state?.phase).toBe('done')
    })
    const state = posts.filter((post) => post.topic === 'state').at(-1)?.payload as UpdaterState
    expect(state.rows.map((row) => row.name).sort()).toEqual(['a', 'b', 'n'])
    expect(state.managers.map((manager) => manager.count)).toEqual([2, 1])
  })

  it('is single-flight: a second check while running reports running', async () => {
    const npm = fakeAdapter('npm', { entries: [], outdatedDelayMs: 50 })
    const { updater } = harness([npm])
    await updater.check()
    await expect(updater.check()).resolves.toEqual({ started: false, running: true })
  })

  it('surfaces per-manager failures without failing the whole report', async () => {
    const npm = fakeAdapter('npm', { failOutdated: true })
    const brew = fakeAdapter('brew', { entries: [entry('n', { manager: 'brew', kind: 'formula' })] })
    const { updater, posts } = harness([npm, brew], { sessions: ['tok-1'] })
    await updater.check()
    await vi.waitFor(() => {
      const state = posts.filter((post) => post.topic === 'state').at(-1)?.payload as UpdaterState
      expect(state?.phase).toBe('done')
      expect(state.managers.find((manager) => manager.id === 'npm')?.error).toContain('npm outdated exploded')
      expect(state.managers.find((manager) => manager.id === 'brew')?.count).toBe(1)
    })
  })
})

describe('Updater.updateAll', () => {
  it('upgrades sequentially, isolates failures, and flushes logs', async () => {
    const npm = fakeAdapter('npm', {
      entries: [entry('a'), entry('b'), entry('c')],
      failUpgradeFor: 'b'
    })
    const { updater, posts } = harness([npm], { sessions: ['tok-1'] })
    await updater.check()
    await vi.waitFor(() => {
      const state = posts.filter((post) => post.topic === 'state').at(-1)?.payload as UpdaterState
      expect(state?.phase).toBe('done')
    })
    await expect(updater.updateAll()).resolves.toEqual({ started: true, running: false })
    await vi.waitFor(() => {
      const state = posts.filter((post) => post.topic === 'state').at(-1)?.payload as UpdaterState
      expect(state?.phase).toBe('done')
      expect(state?.summary).toEqual({ updated: 2, failed: 1, skipped: 0, elapsedMs: expect.any(Number) })
    })
    expect(npm.upgrades).toEqual(['a', 'b', 'c'])
    const logLines = posts.filter((post) => post.topic === 'log').flatMap((post) => (post.payload as { lines: string[] }).lines)
    expect(logLines.some((line) => line.includes('error: b'))).toBe(true)
  })

  it('cancels the running update and skips the rest', async () => {
    const npm = fakeAdapter('npm', { entries: [entry('a'), entry('b'), entry('c')] })
    // 让 a 的升级挂起直到 abort：
    const originalUpgrade = npm.upgrade
    npm.upgrade = (item, signal) =>
      item.name === 'a'
        ? new Promise((resolve) => {
            signal?.addEventListener('abort', () =>
              resolve({ exitCode: null, signal: 'SIGTERM', stdout: '', stderr: '' })
            )
          })
        : originalUpgrade(item, signal)
    const { updater, posts } = harness([npm], { sessions: ['tok-1'] })
    await updater.check()
    await vi.waitFor(() => {
      const state = posts.filter((post) => post.topic === 'state').at(-1)?.payload as UpdaterState
      expect(state?.phase).toBe('done')
    })
    await updater.updateAll()
    await vi.waitFor(() => {
      const state = posts.filter((post) => post.topic === 'state').at(-1)?.payload as UpdaterState
      expect(state?.phase).toBe('updating')
    })
    await expect(updater.cancel()).resolves.toEqual({ canceled: true })
    await vi.waitFor(() => {
      const state = posts.filter((post) => post.topic === 'state').at(-1)?.payload as UpdaterState
      expect(state?.phase).toBe('done')
      expect(state?.summary).toEqual({ updated: 0, failed: 0, skipped: 3, elapsedMs: expect.any(Number) })
    })
    await expect(updater.cancel()).resolves.toEqual({ canceled: false })
  })
})

describe('Updater.setManagers / snapshot / sessions', () => {
  it('persists enabled managers and broadcasts', async () => {
    const npm = fakeAdapter('npm')
    const brew = fakeAdapter('brew')
    const { updater, posts, storage } = harness([npm, brew], { sessions: ['tok-1'] })
    await expect(updater.setManagers(['brew'])).resolves.toEqual({ ok: true })
    expect(storage.get('managers')).toEqual(['brew'])
    const state = posts.filter((post) => post.topic === 'state').at(-1)?.payload as UpdaterState
    expect(state.managers.map((manager) => ({ id: manager.id, enabled: manager.enabled }))).toEqual([
      { id: 'npm', enabled: false },
      { id: 'brew', enabled: true }
    ])
    await expect(updater.setManagers(['nope'])).resolves.toEqual({
      ok: false,
      error: 'unknown_manager',
      manager: 'nope'
    })
  })

  it('restores persisted manager selection and reports counts as null before a check', async () => {
    const npm = fakeAdapter('npm')
    const brew = fakeAdapter('brew')
    const { updater, storage } = harness([npm, brew])
    storage.set('managers', ['npm'])
    const snapshot = await updater.snapshot()
    expect(snapshot.managers.map((manager) => manager.enabled)).toEqual([true, false])
    expect(snapshot.state.checkedAt).toBeNull()
    expect(snapshot.managers.every((manager) => manager.count === null)).toBe(true)
  })

  it('prunes sessions that report unavailable and stops posting to them', async () => {
    const npm = fakeAdapter('npm')
    const { updater, posts, failingTokens } = harness([npm], { sessions: ['dead', 'live'] })
    failingTokens.add('dead')
    await updater.setManagers(['npm'])
    expect(posts.every((post) => post.token === 'live')).toBe(true)
    await updater.setManagers(['npm'])
    expect(posts.every((post) => post.token === 'live')).toBe(true)
    expect(posts.length).toBeGreaterThan(0)
  })
})
