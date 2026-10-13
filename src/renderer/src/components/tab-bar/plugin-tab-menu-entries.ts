import type { PluginHostListEntry, PluginScope } from '../../../../preload/api-types'

/** One menu row: a `contributes.tabs` entry of an enabled plugin available to
 *  the current worktree. */
export type PluginTabMenuEntry = {
  key: string
  pluginKey: string
  pluginName: string
  tabId: string
  title: string
  icon?: string
  scope: PluginScope
  repoId?: string
}

/** Enabled means the same set that may mount panels: consented and not
 *  disabled/errored-invalid. Pending (awaiting consent) stays hidden. */
function isEnabledPlugin(plugin: PluginHostListEntry): boolean {
  return (
    plugin.status === 'running' || plugin.status === 'restarting' || plugin.status === 'idle'
  )
}

/**
 * Flattens the tabs an enabled plugin may open for the active worktree:
 * global entries always, plus project entries installed into the active
 * worktree's repo. Excludes other repos' project entries so they cannot leak
 * across projects (spec §3.6).
 */
export function collectPluginTabMenuEntries(
  plugins: readonly PluginHostListEntry[],
  activeRepoId: string | null
): PluginTabMenuEntry[] {
  return plugins
    .filter(isEnabledPlugin)
    .filter(
      (plugin) =>
        plugin.scope === 'global' ||
        (activeRepoId !== null && plugin.repoId === activeRepoId)
    )
    .flatMap((plugin) =>
      plugin.tabs.map((tab) => ({
        key: `${plugin.pluginKey}\u001f${plugin.scope}\u001f${plugin.repoId ?? ''}\u001f${tab.id}`,
        pluginKey: plugin.pluginKey,
        pluginName: plugin.name,
        tabId: tab.id,
        title: tab.title.trim() || tab.id,
        ...(tab.icon ? { icon: tab.icon } : {}),
        scope: plugin.scope,
        ...(plugin.repoId ? { repoId: plugin.repoId } : {})
      }))
    )
}
