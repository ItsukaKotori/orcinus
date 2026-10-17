import { useMemo } from 'react'
import type { GitBranchCompareSummary } from '../../../../../../shared/git-diff-compare-types'
import { isStageableStatusEntry } from '../commit/discard-all-sequence'
import { resolveDropdownItems } from '../../source-control-dropdown-items'
import { resolveCommitAreaPrimaryAction } from '../../source-control-primary-action'
import type { SourceControlEntryGroups } from '../listing/section-order'

type PrimaryInput = Parameters<typeof resolveCommitAreaPrimaryAction>[0]
type DropdownInput = Parameters<typeof resolveDropdownItems>[0]

export function useSourceControlActionModel({
  grouped,
  commitMessage,
  unresolvedConflictCount,
  isCommitting,
  isRemoteOperationActive,
  isAbortingOperation,
  remoteStatus,
  inFlightRemoteOpKind,
  branchSummary,
  branchName,
  conflictOperation,
  effectiveBaseRef
}: {
  grouped: SourceControlEntryGroups
  commitMessage: string
  unresolvedConflictCount: number
  isCommitting: boolean
  isRemoteOperationActive: boolean
  isAbortingOperation: boolean
  remoteStatus: PrimaryInput['upstreamStatus']
  inFlightRemoteOpKind: PrimaryInput['inFlightRemoteOpKind']
  branchSummary: GitBranchCompareSummary | null
  branchName: string
  conflictOperation: DropdownInput['conflictOperation']
  effectiveBaseRef: string | null
}) {
  const hasUnstagedChanges = grouped.unstaged.length > 0 || grouped.untracked.length > 0
  const hasStageableChanges = useMemo(
    () =>
      grouped.unstaged.some(isStageableStatusEntry) ||
      grouped.untracked.some(isStageableStatusEntry),
    [grouped.unstaged, grouped.untracked]
  )
  const hasPartiallyStagedChanges = useMemo(() => {
    if (grouped.staged.length === 0 || grouped.unstaged.length === 0) {
      return false
    }
    const unstagedPaths = new Set(grouped.unstaged.map((entry) => entry.path))
    return grouped.staged.some((entry) => unstagedPaths.has(entry.path))
  }, [grouped.staged, grouped.unstaged])

  const primaryAction = useMemo(
    () =>
      resolveCommitAreaPrimaryAction({
        stagedCount: grouped.staged.length,
        hasUnstagedChanges,
        hasStageableChanges,
        hasPartiallyStagedChanges,
        hasMessage: commitMessage.trim().length > 0,
        hasUnresolvedConflicts: unresolvedConflictCount > 0,
        isCommitting,
        isRemoteOperationActive: isRemoteOperationActive || isAbortingOperation,
        upstreamStatus: remoteStatus,
        inFlightRemoteOpKind,
        branchCommitsAhead:
          branchSummary?.status === 'ready' ? (branchSummary.commitsAhead ?? 0) : undefined,
        hasCurrentBranch: Boolean(branchName)
      }),
    [
      commitMessage,
      grouped.staged.length,
      hasStageableChanges,
      hasUnstagedChanges,
      hasPartiallyStagedChanges,
      isCommitting,
      isAbortingOperation,
      isRemoteOperationActive,
      inFlightRemoteOpKind,
      branchSummary?.commitsAhead,
      branchSummary?.status,
      branchName,
      remoteStatus,
      unresolvedConflictCount
    ]
  )

  const dropdownItems = useMemo(
    () =>
      resolveDropdownItems({
        stagedCount: grouped.staged.length,
        hasUnstagedChanges,
        hasStageableChanges,
        hasPartiallyStagedChanges,
        hasMessage: commitMessage.trim().length > 0,
        hasUnresolvedConflicts: unresolvedConflictCount > 0,
        isCommitting,
        isRemoteOperationActive: isRemoteOperationActive || isAbortingOperation,
        conflictOperation,
        upstreamStatus: remoteStatus,
        inFlightRemoteOpKind,
        branchCommitsAhead:
          branchSummary?.status === 'ready' ? (branchSummary.commitsAhead ?? 0) : undefined,
        hasCurrentBranch: Boolean(branchName),
        rebaseBaseRef: effectiveBaseRef
      }),
    [
      commitMessage,
      grouped.staged.length,
      hasStageableChanges,
      hasUnstagedChanges,
      hasPartiallyStagedChanges,
      isCommitting,
      conflictOperation,
      isAbortingOperation,
      isRemoteOperationActive,
      inFlightRemoteOpKind,
      branchSummary?.commitsAhead,
      branchSummary?.status,
      branchName,
      effectiveBaseRef,
      remoteStatus,
      unresolvedConflictCount
    ]
  )
  return {
    hasPartiallyStagedChanges,
    primaryAction,
    dropdownItems
  }
}

export type SourceControlActionModel = ReturnType<typeof useSourceControlActionModel>
