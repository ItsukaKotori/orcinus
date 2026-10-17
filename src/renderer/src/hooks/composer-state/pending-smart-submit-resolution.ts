import type { PendingSmartGitHubSubmitResolution } from './source-selection-decisions'

/** No hosted-provider resolution survives the GitHub trim: every submit settles
 *  to the plain local path, and the downstream union branches stay type-checked. */
export function resolvePendingSmartGitHubSubmit(): Promise<PendingSmartGitHubSubmitResolution> {
  return Promise.resolve({ kind: 'none' })
}
