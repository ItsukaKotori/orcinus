import { useAppStore } from '@/store'

export function recordCreatedTerminalPaneSplit(createdPane: unknown): boolean {
  if (!createdPane) {
    return false
  }
  useAppStore.getState().recordFeatureInteraction('terminal-pane-split')
  return true
}
