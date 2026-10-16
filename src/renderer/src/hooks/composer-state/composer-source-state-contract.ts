import type { ComposerModel } from './composer-model'
export type ComposerSourceState = {
  sourceIdentityActions: Pick<
    ComposerModel,
    | 'handleSelectLinkedItem'
    | 'handleLinkPopoverChange'
    | 'handleRemoveLinkedWorkItem'
    | 'handleNameValueChange'
    | 'handleBranchNameOverrideChange'
  >
  attachmentDropState: Pick<
    ComposerModel,
    | 'addComposerAttachments'
    | 'insertComposerFolderPaths'
    | 'uploadComposerPaths'
    | 'handleAddAttachment'
    | 'applyLocalComposerDrop'
  >
  targetChangeActions: Pick<
    ComposerModel,
    'handleRepoChange' | 'handleFolderSourceRepoChange' | 'handleProjectHostSetupChange'
  >
  projectTargetActions: Pick<ComposerModel, 'handleProjectChange'>
  branchStartPointActions: Pick<
    ComposerModel,
    | 'showProjectRequiredError'
    | 'handleSparseSelectPreset'
    | 'handleBaseBranchChange'
    | 'handleBaseBranchPrSelect'
    | 'selectAddedProjectRepo'
  >
  githubProviderSelection: Pick<ComposerModel, 'handleSmartGitHubItemSelect'>
  workItemSourceActions: Pick<
    ComposerModel,
    'handleSmartBranchSelect' | 'handleReuseSelectedBranchChange'
  >
  issueSourceActions: Pick<
    ComposerModel,
    'handleClearSmartNameSelection' | 'smartNameSelection'
  >
  composerNavigationActions: Pick<
    ComposerModel,
    | 'handleOpenAgentSettings'
    | 'applyWorktreeMeta'
    | 'folderCreateDisabled'
  >
}
