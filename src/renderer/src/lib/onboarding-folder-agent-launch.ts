import type { ExecutionHostId } from '../../../shared/execution-host'
import type { GlobalSettings } from '../../../shared/global-settings-types'
import type { OnboardingState } from '../../../shared/onboarding-state-types'
import type { TuiAgent } from '../../../shared/tui-agent'
import {
  buildDismissedOnboardingFolderAgentStartup,
  type OnboardingFolderAgentStartup
} from '@/lib/onboarding-folder-agent-startup'
import { activateAndRevealWorktree } from '@/lib/worktree-activation'

export type OnboardingFolderAgentLaunch = {
  agent: TuiAgent | null
  startup?: OnboardingFolderAgentStartup
}

/** Why: lives beside the launch, not the startup builder, because the store root imports that
 *  builder eagerly and the planner's launch graph reaches back to the store root. */
export function resolveDismissedOnboardingFolderAgentLaunch(args: {
  store: { settings?: GlobalSettings | null }
  onboarding: OnboardingState | null
  hasExistingProject: boolean
  executionHostId?: string
}): OnboardingFolderAgentLaunch {
  const startup = buildDismissedOnboardingFolderAgentStartup(
    args.store.settings ?? null,
    args.onboarding,
    args.hasExistingProject
  )
  return { agent: startup?.launchAgent ?? null, startup }
}

/** Reveal a folder just added after dismissed onboarding and start its default agent on the
 *  planned route. Both add-folder paths (local store action, SSH dialog) share this; the store
 *  path must import it lazily because the launch graph reaches the store root. */
export async function revealOnboardingFolderWithAgentLaunch(args: {
  worktreeId: string
  executionHostId: ExecutionHostId | undefined
  launch: OnboardingFolderAgentLaunch
}): Promise<void> {
  activateAndRevealWorktree(args.worktreeId, {
    sidebarRevealBehavior: 'auto',
    ...(args.executionHostId ? { executionHostId: args.executionHostId } : {}),
    ...(args.launch.startup ? { startup: args.launch.startup } : {})
  })
}
