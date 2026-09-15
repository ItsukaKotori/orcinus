// Phase 0 mock; replaced by Tauri IPC per view.
import type { CrashReportsApi } from '../../preload/api/crash-report-api'

export function createCrashReportsApi(): CrashReportsApi {
  return {
    getLatestPending: async () => null,
    getLatestReport: async () => null,
    dismiss: async () => null,
    recordRendererError: async () => ({ ok: true, report: null, deduped: false }),
    // Why: the preload contract is fire-and-forget (`ipcRenderer.send`, void return). A rejecting
    // call here would re-enter the renderer's unhandledrejection handler and self-amplify.
    recordBreadcrumb: () => {},
    submit: async () => ({ ok: true, report: null }),
    copyLatestDiagnostics: async () => ({ ok: true }),
    // Why: WebKit exposes no V8/Blink heap counters, which the contract already models as null.
    readHeapStatistics: () => null,
    readProcessMemory: async () => null
  }
}
