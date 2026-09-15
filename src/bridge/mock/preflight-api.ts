// Phase 0 mock; replaced by Tauri IPC per view.
import type { PreloadApi } from '../../preload/api-types'
import type { PreflightStatus, RefreshAgentsResult } from '../../preload/api/preflight-api'
import { withMethodFallback } from '../unimplemented-fallback'

// Why all-negative: the Phase 0 shell has not probed the machine, and "not installed" is the
// status the UI already handles by advertising setup steps instead of claiming tooling exists.
const NOT_PROBED: PreflightStatus = {
  git: { installed: false },
  gh: { installed: false, authenticated: false }
}

const NO_AGENTS: RefreshAgentsResult = {
  agents: [],
  addedPathSegments: [],
  shellHydrationOk: false,
  pathSource: 'sync_seed_only',
  pathFailureReason: 'none'
}

export function createPreflightApi(): PreloadApi['preflight'] {
  return withMethodFallback<PreloadApi['preflight']>('preflight', {
    check: async () => NOT_PROBED,
    refreshAgents: async () => NO_AGENTS
  })
}
