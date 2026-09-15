// Phase 0 mock; replaced by Tauri IPC per view.
import type { PreloadApi } from '../../preload/api-types'
import {
  DEFAULT_LOCAL_ORCA_PROFILE_ID,
  createDefaultLocalOrcaProfile
} from '../../shared/orca-profiles'
import { withMethodFallback } from '../unimplemented-fallback'
import { noopUnsubscribe } from './noop-unsubscribe'

export function createOrcaProfilesApi(): PreloadApi['orcaProfiles'] {
  // Why unconfigured: Phase 0 has local-only profiles and no Orca Cloud session, which is also what
  // the browser fallback reports, so the account UI renders its signed-out state without erroring.
  const authStatus = async () => ({
    activeProfileId: DEFAULT_LOCAL_ORCA_PROFILE_ID,
    configured: false,
    state: 'unconfigured' as const,
    persistence: 'none' as const,
    setupMessage: 'Orca Cloud sign-in is not available in the Phase 0 shell.'
  })
  return withMethodFallback<PreloadApi['orcaProfiles']>('orcaProfiles', {
    list: async () => ({
      activeProfileId: DEFAULT_LOCAL_ORCA_PROFILE_ID,
      profiles: [createDefaultLocalOrcaProfile(0)],
      multiProfileUi: false
    }),
    authStatus,
    onAuthStatusChanged: () => noopUnsubscribe
  })
}
