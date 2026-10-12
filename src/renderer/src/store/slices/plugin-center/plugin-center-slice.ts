import type { StateCreator } from 'zustand'
import type { PluginHostListEntry } from '../../../../../preload/api-types'

export type PluginCenterSlice = {
  pluginCenterEntries: PluginHostListEntry[]
  pluginCenterStatus: 'idle' | 'loading' | 'error'
  loadPluginCenterEntries: () => Promise<void>
}

export const createPluginCenterSlice: StateCreator<PluginCenterSlice> = (set) => ({
  pluginCenterEntries: [],
  pluginCenterStatus: 'idle',
  loadPluginCenterEntries: async () => {
    set({ pluginCenterStatus: 'loading' })
    try {
      const entries: PluginHostListEntry[] = await window.api.plugins.list()
      set({ pluginCenterEntries: entries, pluginCenterStatus: 'idle' })
    } catch {
      set({ pluginCenterStatus: 'error' })
    }
  }
})
