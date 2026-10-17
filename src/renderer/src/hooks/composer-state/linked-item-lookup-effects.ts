import type { ComposerModel } from './composer-model'

type LinkedItemLookupEffectsInput = Pick<
  ComposerModel,
  | 'baseBranch'
  | 'prefetchWorktreeCreateBase'
  | 'repoId'
  | 'selectedRepoConnectionId'
  | 'selectedRepoIsGit'
  | 'selectedRepoSshStatus'
  | 'setSetupDecision'
  | 'setupConfig'
  | 'setupPolicy'
  | 'shouldWaitForSetupCheck'
  | 'sshConnectedGeneration'
>

import { canUseRepoBackedComposerSources } from '@/lib/new-workspace-ssh-gate'
import { useEffect } from 'react'

export function useLinkedItemLookupEffects(input: LinkedItemLookupEffectsInput) {
  const {
    baseBranch,
    prefetchWorktreeCreateBase,
    repoId,
    selectedRepoConnectionId,
    selectedRepoIsGit,
    selectedRepoSshStatus,
    setSetupDecision,
    setupConfig,
    setupPolicy,
    shouldWaitForSetupCheck,
    sshConnectedGeneration
  } = input

  const canPrefetchSelectedRepoWorkItems = canUseRepoBackedComposerSources({
    connectionId: selectedRepoConnectionId,
    status: selectedRepoSshStatus
  })

  const prefetchSshConnectedGeneration =
    selectedRepoConnectionId && selectedRepoSshStatus === 'connected' ? sshConnectedGeneration : 0

  useEffect(() => {
    if (!repoId || !selectedRepoIsGit || !canPrefetchSelectedRepoWorkItems) {
      return
    }
    void prefetchWorktreeCreateBase(repoId, baseBranch)
  }, [
    baseBranch,
    canPrefetchSelectedRepoWorkItems,
    prefetchSshConnectedGeneration,
    prefetchWorktreeCreateBase,
    repoId,
    selectedRepoIsGit
  ])

  // Reset setup decision when config / policy changes.
  useEffect(() => {
    if (shouldWaitForSetupCheck) {
      setSetupDecision(null)
      return
    }
    if (!setupConfig) {
      setSetupDecision(null)
      return
    }
    if (setupPolicy === 'ask') {
      setSetupDecision(null)
      return
    }
    setSetupDecision(setupPolicy === 'run-by-default' ? 'run' : 'skip')
  }, [setupConfig, setupPolicy, shouldWaitForSetupCheck, setSetupDecision])

  return {
    canPrefetchSelectedRepoWorkItems,
    prefetchSshConnectedGeneration
  }
}
