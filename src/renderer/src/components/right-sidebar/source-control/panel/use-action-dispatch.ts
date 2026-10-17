import type React from 'react'
import { useCallback } from 'react'
import { shouldForcePushWithLeaseForUpstream } from '../../../../../../shared/git-upstream-status'
import type { DropdownActionKind } from '../../source-control-dropdown-item-types'
import type { SourceControlCommitAction } from '../commit/use-commit-action'
import { handleSourceControlCommitShortcut } from '../commit/commit-shortcut'
import type { SourceControlFileListing } from '../listing/use-file-listing'
import type { SourceControlWorktreeContext } from '../listing/use-worktree-context'
import type { SourceControlConflictAbort } from '../sync/use-conflict-abort'
import type { SourceControlRemoteActionRunner } from '../sync/use-remote-action-runner'
import type { SourceControlActionModel } from './use-action-model'

/**
 * Routes the primary button, the dropdown and the commit shortcut to one shared
 * dispatcher, so every entry point obeys the same in-flight guard.
 */
export function useSourceControlActionDispatch({
  handleAbortMerge,
  handleAbortRebase,
  handleCommit,
  handleStageAllPrimary,
  primaryAction,
  remoteStatus,
  runCompoundCommitAction,
  runRemoteAction
}: {
  handleAbortMerge: SourceControlConflictAbort['handleAbortMerge']
  handleAbortRebase: SourceControlConflictAbort['handleAbortRebase']
  handleCommit: SourceControlCommitAction['handleCommit']
  handleStageAllPrimary: SourceControlFileListing['handleStageAllPrimary']
  primaryAction: SourceControlActionModel['primaryAction']
  remoteStatus: SourceControlWorktreeContext['remoteStatus']
  runCompoundCommitAction: SourceControlRemoteActionRunner['runCompoundCommitAction']
  runRemoteAction: SourceControlRemoteActionRunner['runRemoteAction']
}) {
  // Dispatch primary + dropdown action kinds to their handlers.
  const handleActionInvoke = useCallback(
    (kind: DropdownActionKind): void => {
      switch (kind) {
        case 'commit':
          void handleCommit()
          return
        case 'commit_push':
          void runCompoundCommitAction('push')
          return
        case 'commit_sync':
          void runCompoundCommitAction('sync')
          return
        case 'abort_merge':
          void handleAbortMerge()
          return
        case 'abort_rebase':
          void handleAbortRebase()
          return
        case 'push':
        case 'force_push':
        case 'pull':
        case 'fast_forward':
        case 'sync':
        case 'fetch':
        case 'publish':
        case 'rebase_base':
          void runRemoteAction(kind === 'rebase_base' ? 'rebase' : kind)
      }
    },
    [
      handleCommit,
      handleAbortMerge,
      handleAbortRebase,
      runCompoundCommitAction,
      runRemoteAction
    ]
  )

  // Why: 'stage' routes to a primary-only handler since handleActionInvoke is typed to DropdownActionKind (compound commit_* kinds are dropdown-only).
  const handlePrimaryClick = useCallback((): void => {
    switch (primaryAction.kind) {
      case 'stage':
        void handleStageAllPrimary()
        return
      case 'push':
        // Why: primary labels "Force Push" but keeps kind 'push', so invoke the explicit force path when lease force is required.
        handleActionInvoke(
          shouldForcePushWithLeaseForUpstream(remoteStatus)
            ? 'force_push'
            : 'push'
        )
        return
      case 'commit':
      case 'pull':
      case 'sync':
      case 'publish':
        handleActionInvoke(primaryAction.kind)
    }
  }, [
    handleActionInvoke,
    handleStageAllPrimary,
    primaryAction.kind,
    remoteStatus
  ])

  const handleSourceControlKeyDown = useCallback(
    (event: React.KeyboardEvent<HTMLDivElement>): void => {
      handleSourceControlCommitShortcut(event, primaryAction, handlePrimaryClick)
    },
    [handlePrimaryClick, primaryAction]
  )

  return {
    handleActionInvoke,
    handlePrimaryClick,
    handleSourceControlKeyDown
  }
}

export type SourceControlActionDispatch = ReturnType<typeof useSourceControlActionDispatch>
