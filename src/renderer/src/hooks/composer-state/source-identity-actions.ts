import type { ComposerModel } from './composer-model'

type SourceIdentityActionsInput = Pick<
  ComposerModel,
  | 'applyLinkedWorkItem'
  | 'branchAutoNameRef'
  | 'branchNameOverride'
  | 'branchNameOverridePreservesNameEdits'
  | 'forkPushWarning'
  | 'lastAutoNameRef'
  | 'linkedWorkItem'
  | 'name'
  | 'pushTarget'
  | 'setBranchNameOverride'
  | 'setBranchNameOverridePreservesNameEdits'
  | 'setCreateError'
  | 'setForkPushWarning'
  | 'setLinkDebouncedQuery'
  | 'setLinkDirectItem'
  | 'setLinkPopoverOpen'
  | 'setLinkQuery'
  | 'setLinkedIssue'
  | 'setLinkedPR'
  | 'setLinkedTaskSourceContext'
  | 'setLinkedWorkItem'
  | 'setName'
  | 'setPushTarget'
  | 'setReuseEligibleBranch'
  | 'setReuseSelectedBranch'
  | 'smartGitHubPrStartPointSelectionRef'
>

import { useCallback } from 'react'
import type { GitHubWorkItem } from '../../../../shared/github/work-item-types'
import { resolveComposerManualBranchNameChange } from '../composer-branch-selection'

export function useSourceIdentityActions(input: SourceIdentityActionsInput) {
  const {
    applyLinkedWorkItem,
    branchAutoNameRef,
    branchNameOverride,
    branchNameOverridePreservesNameEdits,
    forkPushWarning,
    lastAutoNameRef,
    linkedWorkItem,
    name,
    pushTarget,
    setBranchNameOverride,
    setBranchNameOverridePreservesNameEdits,
    setCreateError,
    setForkPushWarning,
    setLinkDebouncedQuery,
    setLinkDirectItem,
    setLinkPopoverOpen,
    setLinkQuery,
    setLinkedIssue,
    setLinkedPR,
    setLinkedTaskSourceContext,
    setLinkedWorkItem,
    setName,
    setPushTarget,
    setReuseEligibleBranch,
    setReuseSelectedBranch,
    smartGitHubPrStartPointSelectionRef
  } = input

  const handleSelectLinkedItem = useCallback(
    (item: GitHubWorkItem): void => {
      smartGitHubPrStartPointSelectionRef.current = null
      applyLinkedWorkItem(item)
      setLinkPopoverOpen(false)
      setLinkQuery('')
      setLinkDebouncedQuery('')
      setLinkDirectItem(null)
    },
    [
      applyLinkedWorkItem,
      setLinkDebouncedQuery,
      setLinkDirectItem,
      setLinkPopoverOpen,
      setLinkQuery,
      smartGitHubPrStartPointSelectionRef
    ]
  )

  const handleLinkPopoverChange = useCallback(
    (open: boolean): void => {
      setLinkPopoverOpen(open)
      if (!open) {
        setLinkQuery('')
        setLinkDebouncedQuery('')
        setLinkDirectItem(null)
      }
    },
    [setLinkDebouncedQuery, setLinkDirectItem, setLinkPopoverOpen, setLinkQuery]
  )

  const handleRemoveLinkedWorkItem = useCallback((): void => {
    smartGitHubPrStartPointSelectionRef.current = null
    setLinkedWorkItem(null)
    setLinkedTaskSourceContext(null)
    setLinkedIssue('')
    setLinkedPR(null)
    setForkPushWarning(null)
    if (name === lastAutoNameRef.current) {
      lastAutoNameRef.current = ''
    }
  }, [
    linkedWorkItem,
    name,
    branchAutoNameRef,
    lastAutoNameRef,
    setBranchNameOverride,
    setBranchNameOverridePreservesNameEdits,
    setForkPushWarning,
    setLinkedIssue,
    setLinkedPR,
    setLinkedTaskSourceContext,
    setLinkedWorkItem,
    smartGitHubPrStartPointSelectionRef
  ])

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
    handleSelectLinkedItem,
    handleLinkPopoverChange,
    handleRemoveLinkedWorkItem,
    handleNameValueChange,
    handleBranchNameOverrideChange
  }
}
