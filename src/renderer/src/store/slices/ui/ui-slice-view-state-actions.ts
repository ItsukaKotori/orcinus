import type { UISlice, UISliceSet } from './ui-slice-contract'

export function createUiViewStateActions(set: UISliceSet): Partial<UISlice> {
  return {
    activeView: 'terminal',
    previousViewBeforeSettings: 'terminal',
    previousViewBeforeSpace: 'terminal',
    previousViewBeforeArtifacts: 'terminal',
    setActiveView: (view) => set({ activeView: view }),
    newWorkspaceDraft: null
  }
}
