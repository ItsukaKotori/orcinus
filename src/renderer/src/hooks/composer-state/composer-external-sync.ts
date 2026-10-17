import type { ComposerTargetState } from './composer-target-state-contract'
import type { ComposerExternalSyncState } from './composer-external-sync-contract'
import { useHostRuntimeEffects } from './host-runtime-effects'
import { useLinkedItemLookupEffects } from './linked-item-lookup-effects'

export function useComposerExternalSync(target: ComposerTargetState): ComposerExternalSyncState {
  const hostRuntimeEffects = useHostRuntimeEffects({
    commitHookCheckIfCurrent: target.providerRuntimeSync.commitHookCheckIfCurrent,
    connectionId: target.workspaceIdentityState.connectionId,
    createGateMode: target.composerTargetStore.createGateMode,
    disabledTuiAgents: target.workspaceIdentityState.disabledTuiAgents,
    enableIssueAutomation: target.composerTargetStore.enableIssueAutomation,
    ensureDetectedAgents: target.workspaceIdentityState.ensureDetectedAgents,
    ensureRemoteDetectedAgents: target.workspaceIdentityState.ensureRemoteDetectedAgents,
    ensureRuntimeDetectedAgents: target.workspaceIdentityState.ensureRuntimeDetectedAgents,
    fallbackDefaultAgent: target.workspaceIdentityState.fallbackDefaultAgent,
    folderTargetConnectionId: target.runtimeTargetSelection.folderTargetConnectionId,
    isRemote: target.workspaceIdentityState.isRemote,
    loadHookCheckForRepo: target.providerRuntimeSync.loadHookCheckForRepo,
    newWorkspaceDraft: target.composerTargetStore.newWorkspaceDraft,
    repoId: target.initialTargetState.repoId,
    repoIdRef: target.runtimeTargetSelection.repoIdRef,
    runtimeEnvironmentId: target.workspaceIdentityState.runtimeEnvironmentId,
    selectedRepoConnectionIdRef: target.asyncComposerState.selectedRepoConnectionIdRef,
    selectedRepoExecutionHostId: target.runtimeTargetSelection.selectedRepoExecutionHostId,
    selectedRepoHookContextKey: target.runtimeTargetSelection.selectedRepoHookContextKey,
    selectedRepoIsGit: target.runtimeTargetSelection.selectedRepoIsGit,
    selectedRepoSettingsRef: target.asyncComposerState.selectedRepoSettingsRef,
    selectedRepoSshStatus: target.runtimeTargetSelection.selectedRepoSshStatus,
    setLoadedIssueCommand: target.asyncComposerState.setLoadedIssueCommand,
    setTuiAgent: target.workspaceIdentityState.setTuiAgent,
    settings: target.composerTargetStore.settings,
    tuiAgent: target.workspaceIdentityState.tuiAgent
  })
  const linkedItemLookupEffects = useLinkedItemLookupEffects({
    baseBranch: target.workspaceIdentityState.baseBranch,
    prefetchWorktreeCreateBase: target.composerTargetStore.prefetchWorktreeCreateBase,
    repoId: target.initialTargetState.repoId,
    selectedRepoConnectionId: target.runtimeTargetSelection.selectedRepoConnectionId,
    selectedRepoIsGit: target.runtimeTargetSelection.selectedRepoIsGit,
    selectedRepoSshStatus: target.runtimeTargetSelection.selectedRepoSshStatus,
    setSetupDecision: target.asyncComposerState.setSetupDecision,
    setupConfig: target.derivedComposerState.setupConfig,
    setupPolicy: target.derivedComposerState.setupPolicy,
    shouldWaitForSetupCheck: target.derivedComposerState.shouldWaitForSetupCheck,
    sshConnectedGeneration: target.composerTargetStore.sshConnectedGeneration
  })
  return {
    hostRuntimeEffects,
    linkedItemLookupEffects
  }
}
