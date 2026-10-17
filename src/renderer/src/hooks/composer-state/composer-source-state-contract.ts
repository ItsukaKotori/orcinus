import type { ComposerModel } from './composer-model'
export type ComposerSourceState = {
  sourceIdentityActions: Pick<
    ComposerModel,
    'handleNameValueChange' | 'handleBranchNameOverrideChange'
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
    | 'selectAddedProjectRepo'
  >
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
