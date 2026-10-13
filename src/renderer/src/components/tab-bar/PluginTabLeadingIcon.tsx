import type { PluginTabBinding } from '../../../../shared/tab-types'
import { usePluginPanelsStore } from '@/store/plugin-panels'
import { resolvePluginPanelIcon } from '../right-sidebar/plugin-panel-activity-items'

/**
 * Leading strip icon for a main-area plugin tab: the contribution's declared
 * icon through the same curated map the right-sidebar uses, falling back to
 * the generic plugin glyph when the plugin/tab is gone or declares no icon.
 * The strip data only carries the binding, so the icon is looked up from the
 * authoritative plugin list rather than persisted on the tab.
 */
export function PluginTabLeadingIcon({
  binding
}: {
  binding: PluginTabBinding | undefined
}): React.JSX.Element {
  const iconName = usePluginPanelsStore((state) => {
    if (!binding) {
      return undefined
    }
    const plugin = state.plugins.find(
      (candidate) =>
        candidate.pluginKey === binding.pluginKey &&
        candidate.scope === binding.scope &&
        (candidate.repoId ?? undefined) === binding.repoId
    )
    return plugin?.tabs.find((tab) => tab.id === binding.tabId)?.icon
  })
  const Icon = resolvePluginPanelIcon(iconName)
  return <Icon className="mr-1 size-3 shrink-0 text-muted-foreground" aria-hidden />
}
