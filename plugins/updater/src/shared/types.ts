/** Shared domain types between the updater worker and its UI bundles. */

export type ManagerId = 'npm' | 'brew'

export type PackageKind = 'global' | 'formula' | 'cask'

export type OutdatedEntry = {
  manager: ManagerId
  kind: PackageKind
  name: string
  current: string
  latest: string
}

export type JobPhase = 'idle' | 'checking' | 'updating' | 'done' | 'failed'

export type RowState = 'pending' | 'running' | 'updated' | 'failed' | 'skipped'

export type UpdateRow = OutdatedEntry & { state: RowState; error?: string }

export type ManagerState = {
  id: ManagerId
  label: string
  detected: boolean
  enabled: boolean
  count: number | null
  error?: string
}

export type UpdateSummary = {
  updated: number
  failed: number
  skipped: number
  elapsedMs: number
}

export type UpdaterState = {
  phase: JobPhase
  checkedAt: number | null
  managers: ManagerState[]
  rows: UpdateRow[]
  summary?: UpdateSummary
  error?: string
}

export const MANAGERS_STORAGE_KEY = 'managers'
export const LOG_FLUSH_INTERVAL_MS = 500
export const MAX_LOG_LINE_CHARS = 4096
export const MAX_LOG_CHUNK_BYTES = 48 * 1024
