import type { GitUpstreamStatus } from './git-status-types'

export type SourceControlPrimaryActionKind =
  | 'commit'
  | 'stage'
  | 'push'
  | 'pull'
  | 'sync'
  | 'publish'

export type SourceControlRemoteOpKind =
  | 'push'
  | 'force_push'
  | 'pull'
  | 'sync'
  | 'fetch'
  | 'fast_forward'
  | 'publish'
  | 'rebase'

export type SourceControlPrimaryActionTitleIntent =
  | 'commit_in_progress'
  | 'force_push_in_progress'
  | 'action_in_progress'
  | 'remote_operation_in_progress'
  | 'remote_operation_blocks_commit'
  | 'resolve_conflicts_before_commit'
  | 'commit_staged_changes'
  | 'enter_commit_message'
  | 'stage_all_changes'
  | 'stage_file_to_commit'
  | 'checkout_branch_before_publish'
  | 'publish_branch'
  | 'force_push_with_lease'
  | 'sync_counts'
  | 'pull_count'
  | 'push_count'
  | 'nothing_to_commit_up_to_date'

export type SourceControlPrimaryActionDecision = {
  kind: SourceControlPrimaryActionKind
  disabled: boolean
  labelIntent: SourceControlPrimaryActionKind | 'force_push'
  titleIntent: SourceControlPrimaryActionTitleIntent
  count?: number
  ahead?: number
  behind?: number
  upstreamName?: string
  requiresForceWithLease?: boolean
}

export type SourceControlCommitAreaPrimaryActionDecision = SourceControlPrimaryActionDecision

export type SourceControlPrimaryActionDecisionInputs = {
  stagedCount: number
  hasUnstagedChanges: boolean
  hasStageableChanges: boolean
  hasPartiallyStagedChanges: boolean
  hasMessage: boolean
  hasUnresolvedConflicts: boolean
  isCommitting: boolean
  isRemoteOperationActive: boolean
  upstreamStatus: GitUpstreamStatus | undefined
  inFlightRemoteOpKind?: SourceControlRemoteOpKind | null
  branchCommitsAhead?: number
  hasCurrentBranch?: boolean
}
