import type {
  GitBranchCompareResult,
  GitCommitCompareResult,
  GitDiffResult
} from '../../git-diff-compare-types'
import type {
  GitConflictOperation,
  GitStagingArea,
  GitStatusResult,
  GitUpstreamStatus
} from '../../git-status-types'
import type { GitPushTarget } from '../../worktree/types'
import type { GitHistoryOptions, GitHistoryResult } from '../../git-history'
import type { GitRefEntry } from '../../git-ref-types'

/** Native Git Log view query (maps 1:1 to the `git_log_query` command). */
export type GitLogQuery = {
  refs?: string
  author?: string
  since?: string
  until?: string
  grep?: string
  path?: string
  skip?: number
  limit?: number
}

/** `git_refs` result: capped ref list + truncation flag. */
export type GitRefsResult = { refs: GitRefEntry[]; truncated: boolean }
import type {
  CommitMessageAgentCapability,
  CommitMessageModelCapability
} from '../../commit-message-agent-spec'

export type GitInspectionApi = {
  status: (args: {
    worktreePath: string
    connectionId?: string
    admissionTier?: 'interactive' | 'status' | 'background'
    includeIgnored?: boolean
    includeLineStats?: boolean
    bypassEffectiveUpstreamNegativeCache?: boolean
    reuseLineStats?: boolean
    /** Merge-base OID to measure the branch line total against; omit to skip the work. */
    branchLineTotalMergeBase?: string
    requestToken?: string
  }) => Promise<GitStatusResult>
  cancelStatus: (args: { requestToken: string }) => Promise<void>
  setStatusUpstreamRefWatch: (args: {
    worktreeId: string
    worktreePath: string
    executionHostId: string
    connectionId?: string
    branch?: string
    upstreamName?: string
  }) => Promise<void>
  submoduleStatus: (args: {
    worktreePath: string
    submodulePath: string
    connectionId?: string
    area?: GitStagingArea
  }) => Promise<GitStatusResult>
  checkIgnored: (args: {
    worktreePath: string
    paths: string[]
    connectionId?: string
  }) => Promise<string[]>
  findHugeFoldersToIgnore: (args: { worktreePath: string }) => Promise<string[]>
  history: (
    args: { worktreePath: string; connectionId?: string } & GitHistoryOptions
  ) => Promise<GitHistoryResult>
  logQuery: (args: { worktreePath: string; query: GitLogQuery }) => Promise<GitHistoryResult>
  refs: (args: { worktreePath: string }) => Promise<GitRefsResult>
  conflictOperation: (args: {
    worktreePath: string
    connectionId?: string
  }) => Promise<GitConflictOperation>
  diff: (args: {
    worktreePath: string
    filePath: string
    staged: boolean
    compareAgainstHead?: boolean
    connectionId?: string
  }) => Promise<GitDiffResult>
  branchCompare: (args: {
    worktreePath: string
    baseRef: string
    connectionId?: string
    admissionTier?: 'interactive' | 'status' | 'background'
  }) => Promise<GitBranchCompareResult>
  commitCompare: (args: {
    worktreePath: string
    commitId: string
    connectionId?: string
  }) => Promise<GitCommitCompareResult>
  upstreamStatus: (args: {
    worktreePath: string
    connectionId?: string
    pushTarget?: GitPushTarget
  }) => Promise<GitUpstreamStatus>
  branchDiff: (args: {
    worktreePath: string
    compare: {
      baseRef: string
      baseOid: string
      headOid: string
      mergeBase: string
    }
    filePath: string
    oldPath?: string
    connectionId?: string
  }) => Promise<GitDiffResult>
  commitDiff: (args: {
    worktreePath: string
    commitOid: string
    parentOid?: string | null
    filePath: string
    oldPath?: string
    connectionId?: string
  }) => Promise<GitDiffResult>
  discoverCommitMessageModels: (args: {
    agentId: string
    worktreePath?: string
    connectionId?: string
  }) => Promise<
    | {
        success: true
        capability: CommitMessageAgentCapability
        models: CommitMessageModelCapability[]
        defaultModelId: string
        catalogOrigin: 'probe' | 'spec'
      }
    | { success: false; error: string }
  >
  remoteFileUrl: (args: {
    worktreePath: string
    relativePath: string
    line: number
    connectionId?: string
  }) => Promise<string | null>
  remoteCommitUrl: (args: {
    worktreePath: string
    sha: string
    connectionId?: string
  }) => Promise<string | null>
}
