import type { GitPushTarget } from '../../../../shared/worktree/types'
import type { LinkedWorkItemSummary } from '@/lib/new-workspace'

/** Legacy carrier for a smart-field PR pick; nothing produces one after the
 *  GitHub trim, so the ref stays null and the submit path ignores it. */
export type SmartGitHubPrStartPointSelection = {
  repoId: string
  item: unknown
}

/** The submit path resolves no hosted provider anymore; keep the settled union so
 *  the orchestration branches stay typed and take the plain local path. */
export type SmartGitHubSubmitResolutionMetadata = {
  linkedWorkItem: LinkedWorkItemSummary | null
  linkedIssueNumber: number | null
  linkedPR: number | null
  workspaceName: string
  displayName?: string
  branchNameOverride?: string
}

export type PendingSmartGitHubSubmitResolution =
  | { kind: 'none' }
  | (SmartGitHubSubmitResolutionMetadata & { kind: 'metadata-only' })
  | (SmartGitHubSubmitResolutionMetadata & {
      kind: 'pr-start-point'
      baseBranch: string
      compareBaseRef?: string
      pushTarget?: GitPushTarget
    })
