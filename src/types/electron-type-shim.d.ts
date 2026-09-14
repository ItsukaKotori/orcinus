// Phase 0 typecheck shim; removed in Phase 1
// The preload tree is copied whole to keep the fork layout-identical, but only its
// `api-types`/`api/**` contract is type-included. Those parts still import Electron, and
// Tauri owns the runtime in this fork, so the minimal surface they touch is aliased here.

declare module 'electron' {
  export const app: any
  export const ipcRenderer: any
  export const webFrame: any
  export const webUtils: any
  export const contextBridge: {
    exposeInMainWorld(apiKey: string, api: unknown): void
  }
  export type App = any
  export type BrowserWindow = any
  export type ContextBridge = any
  export type IpcRenderer = any
  export type IpcRendererEvent = any
}

declare namespace Electron {
  type BlinkMemoryInfo = { allocated: number }
  type HeapStatistics = {
    usedHeapSize: number
    totalHeapSize: number
    heapSizeLimit: number
    mallocedMemory: number
  }
  type ProcessMemoryInfo = { private: number; residentSet?: number }
  type DidFailLoadEvent = any
  type DidRedirectNavigationEvent = any
  type DidStartNavigationEvent = any
  type FindInPageOptions = any
  type FoundInPageEvent = any
  type PageTitleUpdatedEvent = any
  type IpcRendererEvent = any
  type WebviewTag = any
}

declare namespace NodeJS {
  interface Process {
    getHeapStatistics(): Electron.HeapStatistics
    getBlinkMemoryInfo(): Electron.BlinkMemoryInfo
    getProcessMemoryInfo(): Promise<Electron.ProcessMemoryInfo>
  }
}
