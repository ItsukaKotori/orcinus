// Phase 0 mock; replaced by Tauri IPC per view.
import type { PreloadApi } from '../../preload/api-types'
import type { MemorySnapshot } from '../../shared/process-stats-types'
import { withMethodFallback } from '../unimplemented-fallback'
import { detectBrowserPlatform } from './platform-api'

// Why a zeroed snapshot: the always-mounted resource segment reads this to render its meters;
// returning a well-formed zero snapshot keeps the meters in their "nothing measured yet" state
// without pretending a daemon responded with real data.
function createEmptyMemorySnapshot(): MemorySnapshot {
  const emptyUsage = { cpu: 0, memory: 0 }
  return {
    app: { ...emptyUsage, main: emptyUsage, renderer: emptyUsage, other: emptyUsage, history: [] },
    worktrees: [],
    host: {
      totalMemory: 0,
      freeMemory: 0,
      availableMemory: 0,
      availableMemorySource: 'free-memory',
      usedMemory: 0,
      memoryUsagePercent: 0,
      cpuCoreCount: navigator.hardwareConcurrency || 1,
      loadAverage1m: 0
    },
    processMemoryMetric: detectBrowserPlatform() === 'win32' ? 'working-set' : 'rss',
    totalCpu: 0,
    totalMemory: 0,
    collectedAt: Date.now()
  }
}

export function createMemoryApi(): PreloadApi['memory'] {
  return withMethodFallback<PreloadApi['memory']>('memory', {
    getSnapshot: async () => createEmptyMemorySnapshot()
  })
}
