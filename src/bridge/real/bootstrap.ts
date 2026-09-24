import type { PlatformApi } from '../../shared/preload-api/api/app-api'
import type { GlobalSettings } from '../../shared/global-settings-types'

/** Snapshot injected by `orcinus-app` before the document parses. */
export type AdeBootstrap = {
  settings: GlobalSettings
  platform: ReturnType<PlatformApi['get']>
  schemaVersion: number
}

type BootstrapScope = typeof globalThis & {
  __ADE_BOOTSTRAP__?: AdeBootstrap
}

/**
 * In-memory copy of the injected snapshot, kept in step with settings writes so
 * `settings.getSync()` reflects prior `set` calls (spec §5.4). The injected
 * `window.__ADE_BOOTSTRAP__` object itself is never mutated.
 */
let snapshotSource: AdeBootstrap | null = null
let snapshot: AdeBootstrap | null = null

function readInjected(): AdeBootstrap | null {
  return (globalThis as BootstrapScope).__ADE_BOOTSTRAP__ ?? null
}

/**
 * Synchronous read of the init-script payload. Returns `null` when the app runs
 * outside the Tauri shell (browser dev server, tests) or the script failed;
 * callers fall back to the async command path.
 */
export function getBootstrap(): AdeBootstrap | null {
  const injected = readInjected()
  if (!injected) {
    snapshotSource = null
    snapshot = null
    return null
  }
  if (snapshotSource !== injected) {
    snapshotSource = injected
    snapshot = injected
  }
  return snapshot
}

/** Refresh the synchronous snapshot after a successful `settings.set`. */
export function updateBootstrapSettings(settings: GlobalSettings): void {
  const current = getBootstrap()
  if (!current) {
    return
  }
  snapshot = { ...current, settings }
}

declare global {
  interface Window {
    __ADE_BOOTSTRAP__?: AdeBootstrap
  }
}
