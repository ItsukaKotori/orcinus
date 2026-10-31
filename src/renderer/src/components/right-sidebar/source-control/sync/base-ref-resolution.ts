import type { GitUpstreamStatus } from '../../../../../../shared/git-status-types'

export function resolveSourceControlBaseRef(input: {
  worktreeBaseRef?: string | null
  repoBaseRef?: string | null
  defaultBaseRef?: string | null
}): string | null {
  const worktreeBaseRef = input.worktreeBaseRef?.trim() || null
  return worktreeBaseRef || input.repoBaseRef?.trim() || input.defaultBaseRef?.trim() || null
}

// Why: the compare base is distinct from the PR/rebase target; when enabled it defaults to the branch upstream (else effectiveBaseRef) to surface local changes.
export function resolveSourceControlCompareBaseRef(input: {
  enabled: boolean
  worktreeBaseRef?: string | null
  repoBaseRef?: string | null
  upstreamName?: string | null
  fallbackBaseRef?: string | null
}): string | null {
  if (!input.enabled) {
    return input.fallbackBaseRef?.trim() || null
  }
  const pinned = input.worktreeBaseRef?.trim() || input.repoBaseRef?.trim()
  if (pinned) {
    return pinned
  }
  return input.upstreamName?.trim() || input.fallbackBaseRef?.trim() || null
}

// Why: only clear the branch-compare summary once we truly have no base; compareBaseRef is momentarily null while upstream loads, which would flicker.
export function shouldClearBranchCompareForMissingBase(input: {
  isFolder: boolean
  compareBaseRef: string | null
  remoteStatus: GitUpstreamStatus | undefined
}): boolean {
  if (input.isFolder || input.compareBaseRef) {
    return false
  }
  return input.remoteStatus !== undefined
}

export function resolveSourceControlPickerBaseRef(input: {
  pinnedBaseRef?: string | null
  effectiveBaseRef?: string | null
}): string | undefined {
  const pinnedBaseRef = input.pinnedBaseRef?.trim()
  if (!pinnedBaseRef) {
    return undefined
  }
  return input.effectiveBaseRef?.trim() || pinnedBaseRef
}
