import type { StateCreator } from 'zustand'
import type { PluginCenterEntry } from '../../../../../bridge/mock/fixtures'

export type PluginCenterSlice = {
  pluginCenterEntries: PluginCenterEntry[]
  pluginCenterStatus: 'idle' | 'loading' | 'error'
  loadPluginCenterEntries: () => Promise<void>
}

export const createPluginCenterSlice: StateCreator<PluginCenterSlice> = (set) => ({
  pluginCenterEntries: [],
  pluginCenterStatus: 'idle',
  loadPluginCenterEntries: async () => {
    set({ pluginCenterStatus: 'loading' })
    try {
      // SAFETY: Phase 0 mock 在 PluginHostListEntry 上附带 scope 字段；真实 scope 字段由 Phase 3 后端补齐。
      const entries = (await window.api.plugins.list()) as PluginCenterEntry[]
      set({ pluginCenterEntries: entries, pluginCenterStatus: 'idle' })
    } catch {
      set({ pluginCenterStatus: 'error' })
    }
  }
})
