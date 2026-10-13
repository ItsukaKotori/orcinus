import React, { useEffect, useMemo } from 'react'
import {
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuSeparator
} from '@/components/ui/dropdown-menu'
import { translate } from '@/i18n/i18n'
import { createBrowserUuid } from '@/lib/browser-uuid'
import { useAppStore } from '@/store'
import { ensurePluginPanelsLoaded, usePluginPanelsStore } from '@/store/plugin-panels'
import { resolvePluginPanelIcon } from '../right-sidebar/plugin-panel-activity-items'
import { collectPluginTabMenuEntries } from './plugin-tab-menu-entries'

/**
 * 「+」menu「插件 Tab」group: enabled plugins' declared tabs available to this
 * worktree (global + the active repo's project entries). Selecting a row opens
 * a fresh tab instance through the shared store action; hidden when empty.
 */
export function TabBarPluginTabMenuItems({
  worktreeId
}: {
  worktreeId: string
}): React.JSX.Element | null {
  const plugins = usePluginPanelsStore((state) => state.plugins)
  const openPluginTab = useAppStore((state) => state.openPluginTab)
  const activeRepoId = useAppStore(
    (state) => state.getKnownWorktreeById?.(worktreeId)?.repoId ?? null
  )

  useEffect(() => {
    ensurePluginPanelsLoaded()
  }, [])

  const entries = useMemo(
    () => collectPluginTabMenuEntries(plugins, activeRepoId),
    [activeRepoId, plugins]
  )

  if (entries.length === 0) {
    return null
  }

  return (
    <>
      <DropdownMenuSeparator />
      <DropdownMenuLabel className="px-2 py-1 text-[11px] font-medium text-muted-foreground">
        {translate('auto.components.tab.bar.TabBar.pluginTabs', '插件 Tab')}
      </DropdownMenuLabel>
      {entries.map((entry) => {
        const Icon = resolvePluginPanelIcon(entry.icon)
        return (
          <DropdownMenuItem
            key={entry.key}
            onSelect={() => {
              openPluginTab({
                pluginKey: entry.pluginKey,
                tabId: entry.tabId,
                scope: entry.scope,
                ...(entry.repoId ? { repoId: entry.repoId } : {}),
                instanceId: createBrowserUuid(),
                title: entry.title,
                worktreeId
              })
            }}
            className="gap-2 rounded-[7px] px-2 py-1.5 text-[12px] leading-5 font-medium"
          >
            <Icon className="size-4 text-muted-foreground" />
            <span className="min-w-0 flex-1 truncate">{entry.title}</span>
            <span className="shrink-0 text-[10px] font-normal text-muted-foreground">
              {entry.pluginName}
            </span>
          </DropdownMenuItem>
        )
      })}
    </>
  )
}
