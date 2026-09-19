import { DeveloperPermissionsPane } from './DeveloperPermissionsPane'
import { SettingsSection } from './SettingsSection'
import { translate } from '@/i18n/i18n'
import type { SettingsRenderContext } from './settings-render-context'

export function renderDeveloperPermissionsSettingsSection(
  context: SettingsRenderContext
): React.JSX.Element | null {
  const { model, navigation, view } = context
  return model.showDesktopOnlySettings && model.isMac ? (
    <SettingsSection
      id="developer-permissions"
      title={translate('auto.components.settings.Settings.65660d4548', 'macOS Permissions')}
      description={translate(
        'auto.components.settings.Settings.9b83cc62c2',
        'macOS privacy access for terminal-launched developer tools.'
      )}
      searchEntries={navigation.getSectionSearchEntries('developer-permissions')}
    >
      {view.isSectionMounted('developer-permissions') ? (
        <DeveloperPermissionsPane highlightedSettingId={model.highlightedSettingsTargetId} />
      ) : null}
    </SettingsSection>
  ) : null
}
