import type { WorktreeMeta } from '../../../../shared/worktree/meta-types'
import type { SparsePreset } from '../../../../shared/worktree/create-types'
import type { GlobalSettings } from '../../../../shared/global-settings-types'
import type { PendingSmartGitHubSubmitResolution } from './source-selection-decisions'
import type { SmartWorkspaceNameSelection } from '@/components/new-workspace/SmartWorkspaceNameField'
export type ComposerSourceModel = {
  addComposerAttachments: (paths: string[]) => void
  applyLocalComposerDrop: (paths: string[], canApply?: () => boolean) => Promise<void>
  applyWorktreeMeta: (worktreeId: string, meta: Partial<WorktreeMeta>) => Promise<void>
  canPrefetchSelectedRepoWorkItems: boolean
  folderCreateDisabled: boolean
  handleAddAttachment: () => Promise<void>
  handleBaseBranchChange: (next: string | undefined) => void
  handleBranchNameOverrideChange: (value: string | undefined) => void
  handleClearSmartNameSelection: () => void
  handleFolderSourceRepoChange: (value: string) => void
  handleNameValueChange: (nextName: string) => void
  handleOpenAgentSettings: () => void
  handleProjectChange: (projectId: string) => void
  handleProjectHostSetupChange: (setupId: string) => void
  handleRepoChange: (
    value: string,
    options?: { preserveStartFrom?: boolean | undefined; forceResetStartFrom?: boolean | undefined }
  ) => void
  handleReuseSelectedBranchChange: (next: boolean) => void
  handleSmartBranchSelect: (refName: string, localBranchName: string) => void
  handleSparseSelectPreset: (preset: SparsePreset | null) => void
  insertComposerFolderPaths: (folderPaths: string[]) => void
  onConnectSelectedProjectGroup: () => Promise<void>
  onConnectSelectedRepo: () => Promise<void>
  prefetchSshConnectedGeneration: number
  resolvePendingSmartGitHubSubmit: () => Promise<PendingSmartGitHubSubmitResolution>
  selectAddedProjectRepo: (nextRepoId: string) => void
  showProjectRequiredError: () => void
  smartNameSelection: SmartWorkspaceNameSelection | null
  uploadComposerPaths: (
    sourcePaths: string[],
    targetSettings?: Pick<GlobalSettings, 'activeRuntimeEnvironmentId'> | null,
    targetConnectionId?: string | null,
    targetRepoPath?: string | null,
    canReportFailure?: () => boolean
  ) => Promise<{ filePaths: string[]; folderPaths: string[] } | null>
}
