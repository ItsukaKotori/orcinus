import { useMemo } from 'react'
import { useTranslation } from 'react-i18next'
import { isSmartWorkspaceSourceQueryWithinLimit } from './smart-workspace-source-results'
import {
  EMPTY_REPO_SEARCH_REPOS,
  type NormalizedSmartWorkspaceNameFieldProps,
  type SmartWorkspaceNameFieldProps
} from './smart-workspace-name-field-model'
import { getSmartWorkspaceNameFieldCopy } from './smart-workspace-name-field-copy'
import { useSmartWorkspaceNameFieldActions } from './use-smart-workspace-name-field-actions'
import { useSmartWorkspaceNameFieldFoundation } from './use-smart-workspace-name-field-foundation'
import { useSmartWorkspaceGithubSearch } from './use-smart-workspace-github-search'
import { useSmartWorkspaceNameFieldPresentation } from './use-smart-workspace-name-field-presentation'
import { useSmartWorkspaceSecondarySearches } from './use-smart-workspace-secondary-searches'

export function useSmartWorkspaceNameFieldController({
  disabled = false,
  textOnly = false,
  branchesEnabled = true,
  repoBackedSourcesDisabled = false,
  repoBackedSearchRepos = EMPTY_REPO_SEARCH_REPOS,
  allowCrossRepoProjectAdd = true,
  crossRepoSwitchTarget = 'project',
  ...props
}: SmartWorkspaceNameFieldProps) {
  // Why: translate()-based options must refresh on language changes without remounting.
  useTranslation()
  const normalizedProps: NormalizedSmartWorkspaceNameFieldProps = {
    ...props,
    disabled,
    textOnly,
    branchesEnabled,
    repoBackedSourcesDisabled,
    repoBackedSearchRepos,
    allowCrossRepoProjectAdd,
    crossRepoSwitchTarget
  }
  const foundation = useSmartWorkspaceNameFieldFoundation(normalizedProps)
  const sourceQueryWithinLimit = useMemo(
    () => isSmartWorkspaceSourceQueryWithinLimit(foundation.debouncedQuery),
    [foundation.debouncedQuery]
  )
  const shouldQueryGithub =
    sourceQueryWithinLimit &&
    !repoBackedSourcesDisabled &&
    !textOnly &&
    foundation.repoBackedSearchTargets.length > 0 &&
    (foundation.mode === 'smart' || foundation.mode === 'github')

  useSmartWorkspaceGithubSearch({
    foundation,
    sourceQueryWithinLimit,
    shouldQueryGithub
  })
  useSmartWorkspaceSecondarySearches({
    foundation
  })
  const presentation = useSmartWorkspaceNameFieldPresentation(foundation)
  const actions = useSmartWorkspaceNameFieldActions(foundation, presentation)
  const copy = getSmartWorkspaceNameFieldCopy({
    repoBackedSourcesDisabled,
    branchesEnabled,
    crossRepoSwitchTarget,
    disabled,
    disabledPlaceholder: props.disabledPlaceholder,
    mode: foundation.mode
  })

  return {
    ...foundation,
    ...presentation,
    ...actions,
    ...copy
  }
}

export type SmartWorkspaceNameFieldController = ReturnType<
  typeof useSmartWorkspaceNameFieldController
>
