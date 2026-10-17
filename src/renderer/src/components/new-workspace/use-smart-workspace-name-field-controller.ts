import { useTranslation } from 'react-i18next'
import type {
  NormalizedSmartWorkspaceNameFieldProps,
  SmartWorkspaceNameFieldProps
} from './smart-workspace-name-field-model'
import { getSmartWorkspaceNameFieldCopy } from './smart-workspace-name-field-copy'
import { useSmartWorkspaceNameFieldActions } from './use-smart-workspace-name-field-actions'
import { useSmartWorkspaceNameFieldFoundation } from './use-smart-workspace-name-field-foundation'
import { useSmartWorkspaceNameFieldPresentation } from './use-smart-workspace-name-field-presentation'
import { useSmartWorkspaceSecondarySearches } from './use-smart-workspace-secondary-searches'

export function useSmartWorkspaceNameFieldController({
  disabled = false,
  textOnly = false,
  branchesEnabled = true,
  ...props
}: SmartWorkspaceNameFieldProps) {
  // Why: translate()-based options must refresh on language changes without remounting.
  useTranslation()
  const normalizedProps: NormalizedSmartWorkspaceNameFieldProps = {
    ...props,
    disabled,
    textOnly,
    branchesEnabled
  }
  const foundation = useSmartWorkspaceNameFieldFoundation(normalizedProps)

  useSmartWorkspaceSecondarySearches({
    foundation
  })
  const presentation = useSmartWorkspaceNameFieldPresentation(foundation)
  const actions = useSmartWorkspaceNameFieldActions(foundation, presentation)
  const copy = getSmartWorkspaceNameFieldCopy({
    branchesEnabled,
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
