import type { OpenFile } from '@/store/slices/editor'
import { canAutoSaveOpenFile } from './editor-autosave'

type ChangedOnDiskMarkState = {
  setExternalMutation: (fileId: string, mutation: 'deleted' | 'renamed' | 'changed' | null) => void
}

// Why: the changed-on-disk mark has one rule — dirty, banner-capable tab —
// but three writers (the watch hook at fs-event time, the autosave
// controller's notification backstop, and the restored-tab conflict scan).
// Centralizing keeps the rule from drifting; each writer keeps its own
// echo/eligibility guard at the call site.
export function markFileChangedOnDisk(
  state: ChangedOnDiskMarkState,
  file: OpenFile
): void {
  if (!file.isDirty || !canAutoSaveOpenFile(file)) {
    return
  }
  state.setExternalMutation(file.id, 'changed')
}
