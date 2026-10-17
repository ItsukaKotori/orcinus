import type { ComposerModel } from './composer-model'

type SourceIdentityActionsInput = Pick<
  ComposerModel,
  | 'branchAutoNameRef'
  | 'branchNameOverride'
  | 'branchNameOverridePreservesNameEdits'
  | 'forkPushWarning'
  | 'lastAutoNameRef'
  | 'name'
  | 'pushTarget'
  | 'setBranchNameOverride'
  | 'setBranchNameOverridePreservesNameEdits'
  | 'setCreateError'
  | 'setForkPushWarning'
  | 'setName'
  | 'setPushTarget'
  | 'setReuseEligibleBranch'
  | 'setReuseSelectedBranch'
>

import { useCallback } from 'react'
import { resolveComposerManualBranchNameChange } from '../composer-branch-selection'

export function useSourceIdentityActions(input: SourceIdentityActionsInput) {
  const {
    branchAutoNameRef,
    branchNameOverride,
    branchNameOverridePreservesNameEdits,
    forkPushWarning,
    lastAutoNameRef,
    name,
    pushTarget,
    setBranchNameOverride,
    setBranchNameOverridePreservesNameEdits,
    setCreateError,
    setForkPushWarning,
    setName,
    setPushTarget,
    setReuseEligibleBranch,
    setReuseSelectedBranch
  } = input

  const handleNameValueChange = useCallback(
    (nextName: string): void => {
      // Why: linked items keep refreshing the suggested name only while it's auto-managed; a manual edit stops later picks from clobbering it until cleared.
      if (!nextName.trim()) {
        lastAutoNameRef.current = ''
      } else if (name !== lastAutoNameRef.current) {
        lastAutoNameRef.current = ''
      }
      if (
        branchNameOverride &&
        !branchNameOverridePreservesNameEdits &&
        nextName !== branchAutoNameRef.current
      ) {
        setBranchNameOverride(undefined)
        branchAutoNameRef.current = ''
      }
      setName(nextName)
      setCreateError(null)
    },
    [
      branchNameOverride,
      branchNameOverridePreservesNameEdits,
      name,
      branchAutoNameRef,
      lastAutoNameRef,
      setBranchNameOverride,
      setCreateError,
      setName
    ]
  )

  const handleBranchNameOverrideChange = useCallback(
    (value: string | undefined): void => {
      const next = resolveComposerManualBranchNameChange({
        value,
        pushTarget,
        forkPushWarning
      })
      setBranchNameOverride(next.branchNameOverride)
      setBranchNameOverridePreservesNameEdits(Boolean(next.branchNameOverride))
      setPushTarget(next.pushTarget)
      setForkPushWarning(next.forkPushWarning)
      setReuseEligibleBranch(null)
      setReuseSelectedBranch(false)
      branchAutoNameRef.current = ''
    },
    [
      forkPushWarning,
      pushTarget,
      branchAutoNameRef,
      setBranchNameOverride,
      setBranchNameOverridePreservesNameEdits,
      setForkPushWarning,
      setPushTarget,
      setReuseEligibleBranch,
      setReuseSelectedBranch
    ]
  )

  return {
    handleNameValueChange,
    handleBranchNameOverrideChange
  }
}
