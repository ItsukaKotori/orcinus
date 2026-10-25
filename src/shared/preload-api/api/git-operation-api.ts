import type { GitForkSyncExpectedUpstream, GitForkSyncResult } from '../../git-fork-sync'
import type { GitCheckoutOutcome, GitCheckoutTarget } from '../../git-checkout-types'
import type { TuiAgent } from '../../tui-agent'
import type { GitPushTarget } from '../../worktree/types'
import type { HostedReviewProvider } from '../../hosted-review'
import type { ResolvedSourceControlAiGenerationParams } from '../../source-control-ai'
import type { SourceControlAiSettings } from '../../source-control-ai-types'

export type GitOperationApi = {
  appendGitignore: (args: { worktreePath: string; folderName: string }) => Promise<boolean>
  abortMerge: (args: { worktreePath: string; connectionId?: string }) => Promise<void>
  abortRebase: (args: { worktreePath: string; connectionId?: string }) => Promise<void>
  fetch: (args: {
    worktreePath: string
    connectionId?: string
    pushTarget?: GitPushTarget
  }) => Promise<void>
  syncFork: (args: {
    worktreePath: string
    connectionId?: string
    expectedUpstream: GitForkSyncExpectedUpstream
  }) => Promise<GitForkSyncResult>
  push: (args: {
    worktreePath: string
    publish?: boolean
    forceWithLease?: boolean
    connectionId?: string
    pushTarget?: GitPushTarget
  }) => Promise<void>
  pull: (args: {
    worktreePath: string
    connectionId?: string
    pushTarget?: GitPushTarget
  }) => Promise<void>
  fastForward: (args: {
    worktreePath: string
    connectionId?: string
    pushTarget?: GitPushTarget
  }) => Promise<void>
  /**
   * 原地检出分支（spec §13.5）。
   *
   * 业务级拒绝在 **resolve 出来的 `outcome.status`** 里，**不** reject ——
   * `BridgeError` 的 Serialize 只留 `{message}`，会把「被别的 workspace 占用」
   * 这种需要给跳转的语义降级成一句普通错误。只有环境级失败
   * （git 不可用、路径不允许、会覆盖本地改动）才 reject。
   */
  checkout: (args: {
    worktreePath: string
    target: GitCheckoutTarget
  }) => Promise<GitCheckoutOutcome>
  rebaseFromBase: (args: {
    worktreePath: string
    baseRef: string
    connectionId?: string
  }) => Promise<void>
  commit: (args: {
    worktreePath: string
    message: string
    connectionId?: string
  }) => Promise<{ success: boolean; error?: string }>
  generateCommitMessage: (args: {
    worktreePath: string
    /** Raw (unstripped) worktree meta key; validated against worktreePath in main. */
    worktreeId?: string
    repoId?: string
    connectionId?: string
    sourceControlAiResolvedParams?: ResolvedSourceControlAiGenerationParams
    sourceControlAi?: SourceControlAiSettings
    agentCmdOverrides?: Partial<Record<TuiAgent, string>>
  }) => Promise<
    | { success: true; message: string; agentLabel?: string }
    | { success: false; error: string; canceled?: boolean }
  >
  cancelGenerateCommitMessage: (args: {
    worktreePath: string
    connectionId?: string
  }) => Promise<void>
  generatePullRequestFields: (args: {
    worktreePath: string
    /** Raw (unstripped) worktree meta key; validated against worktreePath in main. */
    worktreeId?: string
    repoId?: string
    base: string
    title: string
    body: string
    draft: boolean
    provider?: HostedReviewProvider
    useTemplate?: boolean
    connectionId?: string
    sourceControlAiResolvedParams?: ResolvedSourceControlAiGenerationParams
    sourceControlAi?: SourceControlAiSettings
    agentCmdOverrides?: Partial<Record<TuiAgent, string>>
  }) => Promise<
    | {
        success: true
        fields: { base: string; title: string; body: string; draft: boolean }
        agentLabel?: string
        branchChangedByPreparation?: boolean
      }
    | {
        success: false
        error: string
        canceled?: boolean
        branchChangedByPreparation?: boolean
      }
  >
  cancelGeneratePullRequestFields: (args: {
    worktreePath: string
    connectionId?: string
  }) => Promise<void>
  stage: (args: { worktreePath: string; filePath: string; connectionId?: string }) => Promise<void>
  bulkStage: (args: {
    worktreePath: string
    filePaths: string[]
    connectionId?: string
  }) => Promise<void>
  unstage: (args: {
    worktreePath: string
    filePath: string
    connectionId?: string
  }) => Promise<void>
  bulkUnstage: (args: {
    worktreePath: string
    filePaths: string[]
    connectionId?: string
  }) => Promise<void>
  discard: (args: {
    worktreePath: string
    filePath: string
    connectionId?: string
  }) => Promise<void>
  bulkDiscard: (args: {
    worktreePath: string
    filePaths: string[]
    connectionId?: string
  }) => Promise<void>
}
