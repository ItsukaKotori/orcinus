// Phase 0 mock for the local git inspection subset A needs: the SCM status poll
// must answer a well-formed empty status instead of rejecting. B implements the
// real git domain; the rest of the surface rejects loudly until then.
import type { PreloadApi } from '../../preload/api-types'
import { withMethodFallback } from '../unimplemented-fallback'

export function createGitApi(): PreloadApi['git'] {
  return withMethodFallback<PreloadApi['git']>('git', {
    // Why 'unknown': `GitConflictOperation` has no 'none' member; `unknown` is
    // the renderer's no-conflict sentinel (`conflictOperation === 'unknown'`).
    status: async () => ({ entries: [], conflictOperation: 'unknown' }),
    cancelStatus: async () => {},
    setStatusUpstreamRefWatch: async () => {}
  })
}
