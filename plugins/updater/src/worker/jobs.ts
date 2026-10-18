import type { ManagerAdapter } from './managers'
import type {
  JobPhase,
  ManagerId,
  ManagerState,
  OutdatedEntry,
  UpdateRow,
  UpdaterState,
  UpdateSummary
} from '../shared/types'
import {
  LOG_FLUSH_INTERVAL_MS,
  MANAGERS_STORAGE_KEY,
  MAX_LOG_CHUNK_BYTES,
  MAX_LOG_LINE_CHARS
} from '../shared/types'

export type UpdaterDeps = {
  adapters: ManagerAdapter[]
  storageGet: (key: string) => Promise<unknown>
  storageSet: (key: string, value: unknown) => Promise<void>
  postTo: (sessionToken: string, topic: 'state' | 'log', payload: unknown) => Promise<void>
  now: () => number
  /** Broadcast coalescing window; 0 flushes synchronously (tests). */
  stateCoalesceMs?: number
  /** Log flush interval; 0 flushes per line (tests). */
  logFlushMs?: number
  setIntervalFn?: (fn: () => void, ms: number) => unknown
  clearIntervalFn?: (handle: unknown) => void
}

export type SetManagersResult =
  | { ok: true }
  | { ok: false; error: 'unknown_manager'; manager: string }

type RunningJob = { abort: AbortController }

/**
 * Single-flight updater job machine: detect → check → per-package sequential
 * update with live pushes to every session that has ever invoked a command.
 */
export class Updater {
  private readonly deps: UpdaterDeps
  private readonly stateCoalesceMs: number
  private readonly logFlushMs: number
  private phase: JobPhase = 'idle'
  private checkedAt: number | null = null
  private rows: UpdateRow[] = []
  private summary: UpdateSummary | undefined
  private error: string | undefined
  private detected = new Map<ManagerId, boolean>()
  private counts = new Map<ManagerId, number | null>()
  private managerErrors = new Map<ManagerId, string>()
  private enabledIds: Set<string> | null = null
  private running: RunningJob | null = null
  private sessions = new Set<string>()
  private logBuffer: string[] = []
  private flushHandle: unknown = null
  private stateTimer: ReturnType<typeof setTimeout> | null = null
  private lastStateAt = 0
  private loaded = false

  constructor(deps: UpdaterDeps) {
    this.deps = deps
    this.stateCoalesceMs = deps.stateCoalesceMs ?? 250
    this.logFlushMs = deps.logFlushMs ?? LOG_FLUSH_INTERVAL_MS
  }

  rememberSession(token: string): void {
    if (token.length > 0) {
      this.sessions.add(token)
    }
  }

  async snapshot(): Promise<{ managers: ManagerState[]; state: UpdaterState }> {
    await this.ensureLoaded()
    return { managers: this.buildManagerStates(), state: this.buildState() }
  }

  async check(): Promise<{ started: boolean; running: boolean }> {
    await this.ensureLoaded()
    if (this.phase === 'checking' || this.phase === 'updating') {
      this.broadcastState()
      return { started: false, running: true }
    }
    void this.runCheck()
    return { started: true, running: false }
  }

  async updateAll(): Promise<{ started: boolean; running: boolean }> {
    await this.ensureLoaded()
    if (this.running) {
      this.broadcastState()
      return { started: false, running: true }
    }
    void this.runUpdate()
    return { started: true, running: false }
  }

  async cancel(): Promise<{ canceled: boolean }> {
    if (!this.running) {
      return { canceled: false }
    }
    this.running.abort.abort()
    return { canceled: true }
  }

  async setManagers(ids: string[]): Promise<SetManagersResult> {
    await this.ensureLoaded()
    const known = new Set(this.deps.adapters.map((adapter) => adapter.id as string))
    for (const id of ids) {
      if (!known.has(id)) {
        return { ok: false, error: 'unknown_manager', manager: id }
      }
    }
    const unique = [...new Set(ids)]
    await this.deps.storageSet(MANAGERS_STORAGE_KEY, unique)
    this.enabledIds = new Set(unique)
    this.broadcastState()
    return { ok: true }
  }

  private async ensureLoaded(): Promise<void> {
    if (this.loaded) {
      return
    }
    let stored: unknown = null
    try {
      stored = await this.deps.storageGet(MANAGERS_STORAGE_KEY)
    } catch {
      stored = null
    }
    if (Array.isArray(stored)) {
      const known = new Set(this.deps.adapters.map((adapter) => adapter.id as string))
      this.enabledIds = new Set(stored.filter((id): id is string => typeof id === 'string' && known.has(id)))
    }
    this.loaded = true
  }

  private isEnabled(id: ManagerId, detected: boolean): boolean {
    if (this.enabledIds !== null) {
      return this.enabledIds.has(id)
    }
    return detected
  }

  private activeAdapters(): ManagerAdapter[] {
    return this.deps.adapters.filter((adapter) => {
      const detected = this.detected.get(adapter.id) ?? false
      return detected && this.isEnabled(adapter.id, detected)
    })
  }

  private buildManagerStates(): ManagerState[] {
    return this.deps.adapters.map((adapter) => {
      const detected = this.detected.get(adapter.id) ?? false
      const error = this.managerErrors.get(adapter.id)
      return {
        id: adapter.id,
        label: adapter.label,
        detected,
        enabled: this.isEnabled(adapter.id, detected),
        count: this.counts.get(adapter.id) ?? null,
        ...(error !== undefined ? { error } : {})
      }
    })
  }

  private buildState(): UpdaterState {
    return {
      phase: this.phase,
      checkedAt: this.checkedAt,
      managers: this.buildManagerStates(),
      rows: this.rows,
      ...(this.summary !== undefined ? { summary: this.summary } : {}),
      ...(this.error !== undefined ? { error: this.error } : {})
    }
  }

  private async runCheck(): Promise<void> {
    this.phase = 'checking'
    this.error = undefined
    this.summary = undefined
    this.counts.clear()
    this.managerErrors.clear()
    await Promise.all(
      this.deps.adapters.map(async (adapter) => {
        this.detected.set(adapter.id, await adapter.detect())
      })
    )
    this.broadcastState()

    const rows: OutdatedEntry[] = []
    const active = this.activeAdapters()
    let failures = 0
    for (const adapter of active) {
      try {
        const result = await adapter.outdated()
        this.counts.set(adapter.id, result.entries.length)
        rows.push(...result.entries)
      } catch (error) {
        failures += 1
        this.managerErrors.set(adapter.id, messageOf(error))
        this.counts.set(adapter.id, null)
      }
      this.broadcastState()
    }
    this.rows = rows
      .map((row): UpdateRow => ({ ...row, state: 'pending' }))
      .sort((a, b) => a.manager.localeCompare(b.manager) || a.name.localeCompare(b.name))
    this.checkedAt = this.deps.now()
    this.phase = active.length > 0 && failures === active.length ? 'failed' : 'done'
    if (this.phase === 'failed') {
      this.error = active.map((adapter) => this.managerErrors.get(adapter.id)).filter(Boolean).join('; ')
    }
    this.broadcastState()
  }

  private async runUpdate(): Promise<void> {
    if (this.checkedAt === null || this.rows.every((row) => row.state !== 'pending')) {
      await this.runCheck()
    }
    const pending = this.rows.filter((row) => row.state === 'pending')
    if (pending.length === 0) {
      this.phase = 'done'
      this.broadcastState()
      return
    }
    const running: RunningJob = { abort: new AbortController() }
    this.running = running
    this.phase = 'updating'
    this.summary = undefined
    this.error = undefined
    this.broadcastState()
    const startedAt = this.deps.now()
    let updated = 0
    let failed = 0
    let skipped = 0
    for (const row of this.rows) {
      if (row.state !== 'pending') {
        continue
      }
      if (running.abort.signal.aborted) {
        row.state = 'skipped'
        skipped += 1
        continue
      }
      const adapter = this.deps.adapters.find((candidate) => candidate.id === row.manager)
      if (adapter === undefined) {
        row.state = 'failed'
        row.error = 'manager is not registered'
        failed += 1
        this.broadcastState()
        continue
      }
      row.state = 'running'
      this.broadcastState()
      this.pushLogLine(`$ ${adapter.id} ${adapter.upgradeArgs(row).join(' ')}`)
      try {
        const result = await adapter.upgrade(row, running.abort.signal)
        if (result.exitCode === 0) {
          row.state = 'updated'
          updated += 1
          this.pushLogLine(`ok: ${row.name} ${row.latest}`)
        } else if (running.abort.signal.aborted) {
          row.state = 'skipped'
          skipped += 1
          this.pushLogLine(`cancelled: ${row.name}`)
        } else {
          const tail = (result.stderr.trim() || result.stdout.trim()).split('\n').slice(-3).join('\n')
          row.state = 'failed'
          row.error = tail || `exit ${result.exitCode}`
          failed += 1
          this.pushLogLine(`error: ${row.name}: ${row.error}`)
        }
      } catch (error) {
        row.state = 'failed'
        row.error = messageOf(error)
        failed += 1
        this.pushLogLine(`error: ${row.name}: ${row.error}`)
      }
      this.broadcastState()
    }
    this.summary = { updated, failed, skipped, elapsedMs: this.deps.now() - startedAt }
    this.phase = 'done'
    this.running = null
    this.broadcastState()
    this.stopLogFlush()
  }

  private broadcastState(): void {
    if (this.stateCoalesceMs === 0) {
      this.flushState()
      return
    }
    const now = this.deps.now()
    const elapsed = now - this.lastStateAt
    if (elapsed >= this.stateCoalesceMs) {
      this.flushState()
      return
    }
    if (this.stateTimer === null) {
      this.stateTimer = setTimeout(() => {
        this.stateTimer = null
        this.flushState()
      }, this.stateCoalesceMs - elapsed)
    }
  }

  private flushState(): void {
    this.lastStateAt = this.deps.now()
    const state = this.buildState()
    for (const token of [...this.sessions]) {
      void this.postTo(token, 'state', state)
    }
  }

  private pushLogLine(line: string): void {
    const bounded = line.length > MAX_LOG_LINE_CHARS ? line.slice(0, MAX_LOG_LINE_CHARS) : line
    this.logBuffer.push(bounded)
    const bytes = this.logBuffer.reduce((total, entry) => total + entry.length + 1, 0)
    if (this.logFlushMs === 0 || bytes >= MAX_LOG_CHUNK_BYTES || this.logBuffer.length >= 200) {
      this.flushLogs()
      return
    }
    if (this.flushHandle === null) {
      const setIntervalFn =
        this.deps.setIntervalFn ?? ((fn: () => void, ms: number) => setInterval(fn, ms))
      this.flushHandle = setIntervalFn(() => this.flushLogs(), this.logFlushMs)
    }
  }

  private flushLogs(): void {
    if (this.logBuffer.length === 0) {
      return
    }
    const lines = this.logBuffer
    this.logBuffer = []
    for (const token of [...this.sessions]) {
      void this.postTo(token, 'log', { lines })
    }
  }

  private stopLogFlush(): void {
    if (this.flushHandle !== null) {
      const clearIntervalFn =
        this.deps.clearIntervalFn ?? ((handle: unknown) => clearInterval(handle as NodeJS.Timeout))
      clearIntervalFn(this.flushHandle)
      this.flushHandle = null
    }
    this.flushLogs()
  }

  private async postTo(token: string, topic: 'state' | 'log', payload: unknown): Promise<void> {
    try {
      await this.deps.postTo(token, topic, payload)
    } catch (error) {
      if ((error as { code?: unknown })?.code === 'unavailable') {
        this.sessions.delete(token)
      }
    }
  }
}

function messageOf(error: unknown): string {
  return error instanceof Error ? error.message : String(error)
}
