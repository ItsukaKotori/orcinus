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
 * Synchronous read of the init-script payload. Returns `null` when the app runs
 * outside the Tauri shell (browser dev server, tests) or the script failed;
 * callers fall back to the async command path.
 */
export function getBootstrap(): AdeBootstrap | null {
  return (globalThis as BootstrapScope).__ADE_BOOTSTRAP__ ?? null
}

declare global {
  interface Window {
    __ADE_BOOTSTRAP__?: AdeBootstrap
  }
}
