import type { ComposerModel } from './composer-model'

type BranchStartPointActionsInput = Pick<
  ComposerModel,
  | 'branchAutoNameRef'
  | 'handleRepoChange'
  | 'initialProjectGroupAppliedRef'
  | 'setBaseBranch'
  | 'setBranchNameOverride'
  | 'setBranchNameOverridePreservesNameEdits'
  | 'setCompareBaseRef'
  | 'setForkPushWarning'
  | 'setProjectError'
  | 'setSelectedProjectGroupId'
  | 'setPushTarget'
  | 'setReuseEligibleBranch'
  | 'setReuseSelectedBranch'
  | 'setSparseDirectories'
  | 'setSparseEnabled'
  | 'setSparseSelectedPresetId'
  | 'setStartFromResetHint'
>

import { useCallback } from 'react'
import type { SparsePreset } from '../../../../shared/worktree/create-types'

export function useBranchStartPointActions(input: BranchStartPointActionsInput) {
  const {
    branchAutoNameRef,
    handleRepoChange,
    initialProjectGroupAppliedRef,
    setBaseBranch,
    setBranchNameOverride,
    setBranchNameOverridePreservesNameEdits,
    setCompareBaseRef,
    setForkPushWarning,
    setProjectError,
    setSelectedProjectGroupId,
    setPushTarget,
    setReuseEligibleBranch,
    setReuseSelectedBranch,
    setSparseDirectories,
    setSparseEnabled,
    setSparseSelectedPresetId,
    setStartFromResetHint
  } = input

  const showProjectRequiredError = useCallback((): void => {
    setProjectError('Choose or add a project before creating a workspace.')
    requestAnimationFrame(() => {
      document
        .querySelector<HTMLElement>('[data-project-combobox-root="true"][role="combobox"]')
        ?.focus()
    })
  }, [setProjectError])

  const handleSparseSelectPreset = useCallback(
    (preset: SparsePreset | null): void => {
      if (preset) {
        setSparseEnabled(true)
        setSparseDirectories(preset.directories.join('\n'))
        setSparseSelectedPresetId(preset.id)
      } else {
        setSparseEnabled(false)
        setSparseDirectories('')
        setSparseSelectedPresetId(null)
      }
    },
    [setSparseDirectories, setSparseEnabled, setSparseSelectedPresetId]
  )

  const handleBaseBranchChange = useCallback(
    (next: string | undefined): void => {
      setBaseBranch(next)
      setCompareBaseRef(undefined)
      setPushTarget(undefined)
      setBranchNameOverride(undefined)
      // Why (#5181): Start-from means "new branch from this base", so it never reuses — clear reuse state from a prior smart-field branch pick.
      setBranchNameOverridePreservesNameEdits(false)
      setReuseEligibleBranch(null)
      setReuseSelectedBranch(false)
      setForkPushWarning(null)
      branchAutoNameRef.current = ''
      setStartFromResetHint(null)
    },
    [
      branchAutoNameRef,
      setBaseBranch,
      setBranchNameOverride,
      setBranchNameOverridePreservesNameEdits,
      setCompareBaseRef,
      setForkPushWarning,
      setPushTarget,
      setReuseEligibleBranch,
      setReuseSelectedBranch,
      setStartFromResetHint
    ]
  )

  const selectAddedProjectRepo = useCallback(
    (nextRepoId: string): void => {
      initialProjectGroupAppliedRef.current = true
      setSelectedProjectGroupId(null)
      setProjectError(null)
      handleRepoChange(nextRepoId)
    },
    [handleRepoChange, initialProjectGroupAppliedRef, setProjectError, setSelectedProjectGroupId]
  )

  return {
    showProjectRequiredError,
    handleSparseSelectPreset,
    handleBaseBranchChange,
    selectAddedProjectRepo
  }
}
