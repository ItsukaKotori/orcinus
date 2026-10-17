import { translate } from '@/i18n/i18n'
import type { SmartNameMode } from './smart-workspace-source-results'

export function getSmartWorkspaceNameFieldCopy({
  branchesEnabled,
  disabled,
  disabledPlaceholder,
  mode
}: {
  branchesEnabled: boolean
  disabled: boolean
  disabledPlaceholder?: string
  mode: SmartNameMode
}) {
  const smartPlaceholder = branchesEnabled
    ? translate(
        'auto.components.new.workspace.SmartWorkspaceNameField.placeholderSmartWithBranch',
        'Type a name or branch'
      )
    : translate(
        'auto.components.new.workspace.SmartWorkspaceNameField.placeholderWorkspaceName',
        'Type a workspace name'
      )
  const placeholder = disabled
    ? (disabledPlaceholder ??
      translate('auto.components.new.workspace.SmartWorkspaceNameField.unavailable', 'Unavailable'))
    : mode === 'smart'
      ? smartPlaceholder
      : mode === 'branches'
        ? translate(
            'auto.components.new.workspace.SmartWorkspaceNameField.searchBranches',
            'Search branches'
          )
        : translate(
            'auto.components.new.workspace.SmartWorkspaceNameField.workspaceName',
            'Workspace name'
          )

  return { placeholder }
}
