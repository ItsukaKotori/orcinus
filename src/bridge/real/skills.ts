import type { SkillDiscoveryResult, SkillDiscoveryTarget } from '../../shared/skills'
import type { PreloadApi } from '../../shared/preload-api/api-types'
import { withMethodFallback } from '../unimplemented-fallback'
import { invokeCommand } from './invoke'

/**
 * The `skills_discover` wire result: `skills`/`sources` are the shared
 * `src/shared/skills.ts` shapes shipped as opaque JSON, and `scannedAt` is an
 * ISO-8601 UTC string (the bridge convention for timestamps); the adapter
 * converts it to the epoch-millis `SkillDiscoveryResult` contract.
 */
type SkillsDiscoverCommandResult = {
  skills: SkillDiscoveryResult['skills']
  sources: SkillDiscoveryResult['sources']
  scannedAt: string
}

/**
 * Real `skills` adapter (spec §3.9): minimal home-root discovery. This host
 * only scans its own home skill roots, so every host-identifying field of the
 * discovery target (`runtime`, `wslDistro`, `cwd`, `worktreeId`,
 * `projectRuntime`) is dropped; `refresh` describes the request and is the one
 * field forwarded. Every other `skills.*` method (freshness, install, share,
 * delete, WSL listing) has no host surface in this slice and stays on the
 * rejecting fallback for the renderer to feature-detect.
 */
export function createSkillsRealApi(): PreloadApi['skills'] {
  return withMethodFallback<PreloadApi['skills']>('skills', {
    discover: async (target?: SkillDiscoveryTarget): Promise<SkillDiscoveryResult> => {
      const args = target?.refresh === undefined ? undefined : { refresh: target.refresh }
      const result = await invokeCommand<SkillsDiscoverCommandResult>('skills_discover', { args })
      return {
        skills: result.skills,
        sources: result.sources,
        scannedAt: Date.parse(result.scannedAt)
      }
    }
  })
}
