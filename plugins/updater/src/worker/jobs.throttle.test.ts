import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { ManagerAdapter } from './managers'
import type { OutdatedEntry, UpdaterState } from '../shared/types'
import {
  LOG_FLUSH_INTERVAL_MS,
  MAX_LOG_CHUNK_BYTES,
  STATE_COALESCE_INTERVAL_MS
} from '../shared/types'
import { Updater, type UpdaterDeps } from './jobs'

/**
 * I1 coverage: production throttle path. Unlike jobs.test.ts (which injects
 * 0/0), these cases construct the Updater with the real defaults and drive
 * time with fake timers, so the budget-aligned intervals are actually
 * exercised.
 */

function entry(name: string, overrides: Partial<OutdatedEntry> = {}): OutdatedEntry {
  return { manager: 'npm', kind: 'global', name, current: '1.0.0', latest: '2.0.0', ...overrides }
}

type FakeAdapterOptions = {
  entries?: OutdatedEntry[]
  failUpgrade?: boolean
  upgradeDelayMs?: number
}

function fakeAdapter(id: 'npm' | 'brew', options: FakeAdapterOptions = {}): ManagerAdapter {
  return {
    id,
    label: id === 'npm' ? 'npm' : 'Homebrew',
    kinds: id === 'npm' ? ['global'] : ['formula', 'cask'],
    detect: async () => true,
    outdated: async () => ({
      entries: options.entries ?? [],
      raw: { exitCode: 0, signal: null, stdout: '', stderr: '' }
    }),
    upgradeArgs: () => [],
    upgrade: async (item: OutdatedEntry) => {
      if (options.upgradeDelayMs) {
        await new Promise((resolve) => setTimeout(resolve, options.upgradeDelayMs))
      }
      if (options.failUpgrade) {
        return { exitCode: 1, signal: null, stdout: '', stderr: `${item.name} exploded` }
      }
      return { exitCode: 0, signal: null, stdout: '', stderr: '' }
    }
  } as unknown as ManagerAdapter
}

type Post = { token: string; topic: 'state' | 'log'; payload: unknown; at: number }

function throttleHarness(adapters: ManagerAdapter[], tokens: string[] = ['tok']) {
  const posts: Post[] = []
  const deps: UpdaterDeps = {
    adapters,
    storageGet: async () => null,
    storageSet: async () => {},
    postTo: async (token, topic, payload) => {
      posts.push({ token, topic, payload, at: Date.now() })
    },
    now: () => Date.now()
    // No stateCoalesceMs / logFlushMs overrides: exercise production defaults.
  }
  const updater = new Updater(deps)
  for (const token of tokens) {
    updater.rememberSession(token)
  }
  return { updater, posts }
}

const statePosts = (posts: Post[]): UpdaterState[] =>
  posts.filter((post) => post.topic === 'state').map((post) => post.payload as UpdaterState)
const logPosts = (posts: Post[]): string[][] =>
  posts
    .filter((post) => post.topic === 'log')
    .map((post) => (post.payload as { lines: string[] }).lines)

/** Drain pending microtasks (the job loops are promise-driven) without moving
 *  the clock, so coalesce/flush timers stay pending. */
async function drainMicrotasks(): Promise<void> {
  for (let i = 0; i < 12; i += 1) {
    await vi.advanceTimersByTimeAsync(0)
  }
}

describe('Updater default throttle (fake timers)', () => {
  beforeEach(() => {
    vi.useFakeTimers()
    vi.setSystemTime(1_000_000)
  })

  afterEach(() => {
    vi.useRealTimers()
  })

  it('coalesces rapid state transitions and pushes the latest state', async () => {
    const { updater, posts } = throttleHarness([fakeAdapter('npm'), fakeAdapter('brew')])

    await updater.setManagers(['npm']) // first push: window elapsed since t=0
    expect(statePosts(posts)).toHaveLength(1)

    const before = posts.length
    await updater.setManagers(['brew'])
    await updater.setManagers(['npm'])
    // Inside the window nothing is pushed one-by-one.
    expect(posts.length).toBe(before)

    await vi.advanceTimersByTimeAsync(STATE_COALESCE_INTERVAL_MS)
    expect(posts.length).toBe(before + 1)
    const latest = statePosts(posts).at(-1)!
    expect(latest.managers.map((manager) => [manager.id, manager.enabled])).toEqual([
      ['npm', true],
      ['brew', false]
    ])
  })

  it('flushes a log batch only after the flush interval', async () => {
    const { updater, posts } = throttleHarness([fakeAdapter('npm')])
    const push = (line: string): void =>
      (updater as unknown as { pushLogLine(line: string): void }).pushLogLine(line)

    push('one')
    push('two')
    expect(logPosts(posts)).toHaveLength(0)

    await vi.advanceTimersByTimeAsync(LOG_FLUSH_INTERVAL_MS)
    expect(logPosts(posts)).toEqual([['one', 'two']])
  })

  it('flushes immediately on a 200-line burst', async () => {
    const { updater, posts } = throttleHarness([fakeAdapter('npm')])
    const push = (line: string): void =>
      (updater as unknown as { pushLogLine(line: string): void }).pushLogLine(line)

    for (let i = 0; i < 200; i += 1) {
      push(`line-${i}`)
    }
    // No timer advance: the length cap forced a flush.
    expect(logPosts(posts)).toHaveLength(1)
    expect(logPosts(posts)[0]).toHaveLength(200)
  })

  it('meters buffered log bytes in UTF-8, not code units', async () => {
    const { updater, posts } = throttleHarness([fakeAdapter('npm')])
    const push = (line: string): void =>
      (updater as unknown as { pushLogLine(line: string): void }).pushLogLine(line)

    // 1000 code units but 3000 UTF-8 bytes per line. 16 lines = 16_016 code
    // units (< cap) yet 48_016 bytes; the 17th crosses MAX_LOG_CHUNK_BYTES.
    const heavy = '你'.repeat(1000)
    for (let i = 0; i < 16; i += 1) {
      push(heavy)
    }
    expect(logPosts(posts)).toHaveLength(0)
    expect(16 * (heavy.length + 1)).toBeLessThan(MAX_LOG_CHUNK_BYTES)

    push(heavy)
    expect(logPosts(posts)).toHaveLength(1)
    expect(logPosts(posts)[0]).toHaveLength(17)
    expect(17 * (Buffer.byteLength(heavy, 'utf8') + 1)).toBeGreaterThanOrEqual(MAX_LOG_CHUNK_BYTES)
  })

  it('delivers the terminal check state without waiting for the coalesce window', async () => {
    const { updater, posts } = throttleHarness([
      fakeAdapter('npm', { entries: [entry('a')] })
    ])

    await updater.check()
    await drainMicrotasks()

    // No advance of STATE_COALESCE_INTERVAL_MS: `done` must already be here.
    expect(statePosts(posts).at(-1)?.phase).toBe('done')
  })

  it('delivers the terminal update state (summary+done) in the same round', async () => {
    const { updater, posts } = throttleHarness([
      fakeAdapter('npm', { entries: [entry('a'), entry('b')] })
    ])

    await updater.check()
    await drainMicrotasks()
    await updater.updateAll()
    await drainMicrotasks()

    const last = statePosts(posts).at(-1)!
    expect(last.phase).toBe('done')
    expect(last.summary).toEqual({ updated: 2, failed: 0, skipped: 0, elapsedMs: expect.any(Number) })
  })

  it('keeps per-token state+log pushes inside the 30/10s host budget and still delivers done', async () => {
    const rows = Array.from({ length: 30 }, (_, index) => entry(`pkg-${index}`))
    // Each package takes 300ms of (fake) wall time, so the update spans ~9s.
    const npm = fakeAdapter('npm', { entries: rows, failUpgrade: true, upgradeDelayMs: 300 })
    const { updater, posts } = throttleHarness([npm])

    await updater.updateAll()
    await vi.advanceTimersByTimeAsync(30_000)

    const last = statePosts(posts).at(-1)!
    expect(last.phase).toBe('done')
    expect(last.summary).toEqual({ updated: 0, failed: 30, skipped: 0, elapsedMs: expect.any(Number) })

    // Rolling 10s window per token: state + log share the budget.
    const windowMs = 10_000
    let worst = 0
    for (const anchor of posts) {
      const inWindow = posts.filter(
        (post) => post.token === anchor.token && post.at >= anchor.at && post.at < anchor.at + windowMs
      ).length
      worst = Math.max(worst, inWindow)
    }
    expect(worst).toBeGreaterThan(1)
    expect(worst).toBeLessThanOrEqual(30)
  })
})
