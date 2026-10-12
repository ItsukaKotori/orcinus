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
      const entries: PluginCenterEntry[] = await window.api.plugins.list()
      set({ pluginCenterEntries: entries, pluginCenterStatus: 'idle' })
    } catch {
      set({ pluginCenterStatus: 'error' })
    }
  }
})
