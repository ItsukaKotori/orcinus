import type { PluginTabBinding } from '../../../../../shared/tab-types'
import type { TabsSlice, TabsSliceGet, TabsSliceSet } from './tabs-slice-contract'
import { buildActiveSurfacePatch } from './tabs-surface'

/** Spec §3.6: cap the label so a hostile contribution title cannot blow up
 *  the tab strip. */
const MAX_PLUGIN_TAB_LABEL_LENGTH = 128

export function openPluginTabBindingId(input: {
  pluginKey: string
  tabId: string
  instanceId: string
}): string {
  return `${input.pluginKey}/${input.tabId}/${input.instanceId}`
}

export function openPluginTabLabel(input: {
  title?: string
  titleHint?: string
  tabId: string
}): string {
  const label = input.title?.trim() || input.titleHint?.trim() || input.tabId
  return label.slice(0, MAX_PLUGIN_TAB_LABEL_LENGTH)
}

export function createTabsPluginActions(
  set: TabsSliceSet,
  get: TabsSliceGet
): Pick<TabsSlice, 'openPluginTab'> {
  return {
    openPluginTab: (input) => {
      const { worktreeId, instanceId } = input
      const existing = (get().unifiedTabsByWorktree[worktreeId] ?? []).find(
        (tab) => tab.contentType === 'plugin' && tab.pluginTab?.instanceId === instanceId
      )
      if (existing) {
        // Why: only the active worktree may take focus; a background
        // `workspace.openTab` must not yank the user away from what they see.
        if (get().activeWorktreeId === worktreeId) {
          get().activateTab(existing.id, { worktreeId })
          // Why: activateTab moves the group selection but not the derived
          // active surface; patch it in the same animation frame the users sees.
          set((state) => buildActiveSurfacePatch(state, worktreeId, existing.groupId))
        }
        return existing
      }

      const binding: PluginTabBinding = {
        pluginKey: input.pluginKey,
        tabId: input.tabId,
        scope: input.scope,
        ...(input.repoId ? { repoId: input.repoId } : {}),
        instanceId,
        ...(input.title ? { titleHint: input.title } : {}),
        ...(input.params !== undefined ? { params: input.params } : {})
      }
      const id = openPluginTabBindingId(input)
      const shouldActivate = get().activeWorktreeId === worktreeId
      const created = get().createUnifiedTab(worktreeId, 'plugin', {
        id,
        entityId: id,
        label: openPluginTabLabel({ ...input, titleHint: binding.titleHint }),
        pluginTab: binding,
        // Why: mirrors every other opener — activate only when the target
        // worktree is the one on screen; background worktrees stay untouched.
        activate: shouldActivate
      })
      if (shouldActivate) {
        // Why: browser/editor openers write `activeTabTypeByWorktree` as part of
        // activation; createUnifiedTab alone leaves the derived surface stale.
        set((state) => buildActiveSurfacePatch(state, worktreeId, created.groupId))
      }
      return created
    }
  }
}
