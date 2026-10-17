import { AgentsPane } from './AgentsPane'
import { OrchestrationPane } from './OrchestrationPane'
import { SettingsSection } from './SettingsSection'
import { translate } from '@/i18n/i18n'
import type { SettingsRenderContext } from './settings-render-context'

export function renderAgentsSettingsSection(context: SettingsRenderContext): React.JSX.Element {
  const { model, navigation, terminal, view } = context
  return (
    <SettingsSection
      id="agents"
      title={translate('auto.components.settings.Settings.8afa676615', 'Agents')}
      description={translate(
        'auto.components.settings.Settings.ec1ba547f7',
        'Manage AI agents, set a default, and customize commands.'
      )}
      searchEntries={navigation.getSectionSearchEntries('agents')}
    >
      {view.isSectionMounted('agents') ? (
        <AgentsPane
          settings={model.settings}
          updateSettings={model.updateSettings}
          wslSupportedPlatform={terminal.localWslSupportedPlatform}
          wslAvailable={terminal.localWindowsRuntimeCapabilities.wslAvailable}
          wslDistros={terminal.localWindowsRuntimeCapabilities.wslDistros}
          wslCapabilitiesLoading={terminal.localWindowsRuntimeCapabilities.isLoading}
        />
      ) : null}
    </SettingsSection>
  )
}

export function renderOrchestrationSettingsSection(
  context: SettingsRenderContext
): React.JSX.Element {
  const { model, navigation, view } = context
  return (
    <SettingsSection
      id="orchestration"
      title={translate('auto.components.settings.Settings.00c3a7950d', 'Orchestration')}
      description={translate(
        'auto.components.settings.Settings.475980f53d',
        'Coordinate multiple coding agents through Orca.'
      )}
      searchEntries={navigation.getSectionSearchEntries('orchestration')}
    >
      {view.isSectionMounted('orchestration') ? (
        <OrchestrationPane settings={model.settings} updateSettings={model.updateSettings} />
      ) : null}
    </SettingsSection>
  )
}
