import type { PluginHostListEntry, PluginRef } from '../../../../preload/api-types'

/** One plugin key can be installed globally and in several projects, so any
 *  consent/toggle dialog target must name the exact scope instance. */
export function toPluginRef(plugin: PluginHostListEntry): PluginRef {
  return plugin.repoId
    ? { pluginKey: plugin.pluginKey, scope: plugin.scope, repoId: plugin.repoId }
    : { pluginKey: plugin.pluginKey, scope: plugin.scope }
}

export function pluginRefsEqual(a: PluginRef, b: PluginRef): boolean {
  return (
    a.pluginKey === b.pluginKey &&
    a.scope === b.scope &&
    (a.repoId ?? '') === (b.repoId ?? '')
  )
}

export function matchesPluginRef(plugin: PluginHostListEntry, ref: PluginRef | null): boolean {
  return ref ? pluginRefsEqual(toPluginRef(plugin), ref) : false
}

/** Stable React/identity key for one scope instance. */
export function pluginRefKey(ref: PluginRef): string {
  return `${ref.scope}:${ref.repoId ?? ''}:${ref.pluginKey}`
}
