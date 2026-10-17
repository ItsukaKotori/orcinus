import type { ComposerModel } from './composer-model'

export type ComposerCardSourceProps = Pick<
  ComposerModel,
  | 'eligibleRepos'
  | 'repoId'
  | 'projectOptions'
  | 'selectedProjectId'
  | 'selectedRepoIsGit'
  | 'projectHostSetupOptions'
  | 'selectedProjectHostSetupId'
  | 'ephemeralVmRecipes'
  | 'selectedEphemeralVmRecipeId'
  | 'ephemeralVmRecipeError'
  | 'name'
  | 'branchNameOverride'
  | 'parentWorktreeId'
  | 'selectedRepoExecutionHostId'
  | 'selectedRepoProjectId'
  | 'smartNameSelection'
  | 'reuseSelectedBranch'
  | 'createMultiple'
  | 'agentPrompt'
  | 'attachmentPaths'
  | 'tuiAgent'
  | 'detectedAgentIds'
  | 'advancedOpen'
  | 'projectError'
  | 'creating'
  | 'note'
  | 'baseBranch'
  | 'selectedRepoIsRemote'
  | 'selectedRepoConnectionId'
  | 'selectedRepoSshStatus'
  | 'selectedRepoRequiresConnection'
  | 'selectedRepoConnectInProgress'
  | 'onConnectSelectedRepo'
  | 'startFromResetHint'
  | 'forkPushWarning'
  | 'setupConfig'
  | 'requiresExplicitSetupChoice'
  | 'setupDecision'
  | 'setupAgentStartupPolicy'
  | 'shouldWaitForSetupCheck'
  | 'resolvedSetupDecision'
  | 'createError'
  | 'sparsePresets'
  | 'sparseSelectedPresetId'
>

export type ComposerCardActionProps = {
  onRepoChange: ComposerModel['handleRepoChange']
  onProjectChange: ComposerModel['handleProjectChange']
  onProjectHostSetupChange: ComposerModel['handleProjectHostSetupChange']
  onEphemeralVmRecipeChange: ComposerModel['setSelectedEphemeralVmRecipeId']
  onNameValueChange: ComposerModel['handleNameValueChange']
  onBranchNameOverrideChange: ComposerModel['handleBranchNameOverrideChange']
  onParentWorktreeIdChange: ComposerModel['setParentWorktreeId']
  onSmartBranchSelect: ComposerModel['handleSmartBranchSelect']
  onSmartNameModeChange?: ComposerModel['setSmartNameMode']
  onClearSmartNameSelection: ComposerModel['handleClearSmartNameSelection']
  canReuseSelectedBranch: boolean
  onReuseSelectedBranchChange: ComposerModel['handleReuseSelectedBranchChange']
  showCreateMultiple: boolean
  onCreateMultipleChange: ComposerModel['setCreateMultiple']
  onAgentPromptChange: ComposerModel['setAgentPrompt']
  getAttachmentLabel: (pathValue: string) => string
  onAddAttachment: () => void
  onRemoveAttachment: (pathValue: string) => void
  onTuiAgentChange: ComposerModel['setTuiAgent']
  onOpenAgentSettings: ComposerModel['handleOpenAgentSettings']
  onToggleAdvanced: () => void
  createDisabled: boolean
  onCreate: () => void
  onNoteChange: ComposerModel['setNote']
  onBaseBranchChange: ComposerModel['handleBaseBranchChange']
  branchesEnabled?: boolean
  setupControlsEnabled?: boolean
  onSetupDecisionChange: ComposerModel['setSetupDecision']
  onSetupAgentStartupPolicyChange: ComposerModel['handleSetupAgentStartupPolicyChange']
  canUseSparseCheckout: boolean
  selectedRepoPath: string | null
  onSparseSelectPreset: ComposerModel['handleSparseSelectPreset']
  sparseControlsEnabled?: boolean
}
