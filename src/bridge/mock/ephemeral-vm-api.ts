// Phase 0 mock; A has no ephemeral-VM support, so runtime-facing methods no-op and
// the rest reject loudly. resumeWorkspace must resolve null (not reject): the
// worktree activation path probes it with `typeof` and turns a rejection into a
// spurious "Failed to wake ephemeral VM workspace" toast.
import type { PreloadApi } from '../../preload/api-types'
import { withMethodFallback } from '../unimplemented-fallback'

export function createEphemeralVmApi(): PreloadApi['ephemeralVm'] {
  return withMethodFallback<PreloadApi['ephemeralVm']>('ephemeralVm', {
    resumeWorkspace: async () => null,
    suspendWorkspace: async () => null,
    listRuntimes: async () => [],
    listRecipes: async () => ({
      status: 'ok',
      repoPath: null,
      recipes: [],
      diagnostics: []
    })
  })
}
