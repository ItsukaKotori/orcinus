// Phase 0 mock; replaced by Tauri IPC per view.
import type { PreloadApi } from '../../preload/api-types'
import { withMethodFallback } from '../unimplemented-fallback'

export function createSkillsApi(): PreloadApi['skills'] {
  // Why empty: no skill directories have been scanned in Phase 0; consumers render the
  // discovered-skills empty state instead of treating discovery as a failure.
  return withMethodFallback<PreloadApi['skills']>('skills', {
    discover: async () => ({ skills: [], sources: [], scannedAt: Date.now() })
  })
}
