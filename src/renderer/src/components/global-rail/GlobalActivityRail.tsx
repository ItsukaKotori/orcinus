import { useEffect } from 'react'
import type { LucideIcon } from 'lucide-react'
import { Plug } from 'lucide-react'
import { cn } from '@/lib/utils'
import { useAppStore } from '@/store'
import { GLOBAL_RAIL_BOTTOM_ITEMS, GLOBAL_RAIL_ITEMS } from './global-rail-items'

function RailButton({
  label,
  icon: Icon,
  active,
  onClick
}: {
  label: string
  icon: LucideIcon
  active: boolean
  onClick: () => void
}): React.JSX.Element {
  return (
    <button
      type="button"
      className={cn(
        'relative flex h-10 w-10 shrink-0 items-center justify-center transition-colors',
        active ? 'text-foreground' : 'text-muted-foreground/60 hover:text-muted-foreground'
      )}
      aria-label={label}
      title={label}
      onClick={onClick}
    >
      <Icon size={18} />
      {active ? (
        <div className="absolute right-0 top-[25%] bottom-[25%] w-[2px] bg-foreground rounded-l" />
      ) : null}
    </button>
  )
}

/**
 * The leftmost global rail: project switch, plugin center, one entry per
 * installed global plugin, and a bottom-anchored settings/help group. Lives
 * outside the worktree sidebar so it stays put on non-project views.
 */
export function GlobalActivityRail(): React.JSX.Element {
  const activeView = useAppStore((s) => s.activeView)
  const pluginCenterEntries = useAppStore((s) => s.pluginCenterEntries)
  const globalPlugins = pluginCenterEntries.filter((entry) => entry.scope === 'global')

  useEffect(() => {
    void useAppStore.getState().loadPluginCenterEntries()
  }, [])

  const openHelp = (): void => {
    // Why: the sidebar help menu is a sidebar-sized Settings+Help button pair and cannot be
    // reused as a rail button; until a dedicated help surface exists, route to the app's
    // existing guided-help pane (the same setup-guide surface the help menu's Milestones opens).
    useAppStore.getState().openSettingsTarget({ pane: 'setup-guide', repoId: null })
    useAppStore.getState().openSettingsPage()
  }

  return (
    <nav
      aria-label="全局导航"
      className="flex h-full shrink-0 flex-col items-center w-10 min-w-[40px] bg-sidebar border-r border-border"
    >
      {GLOBAL_RAIL_ITEMS.map((item) => (
        <RailButton
          key={item.kind}
          label={item.label}
          icon={item.icon}
          active={activeView === item.view}
          onClick={() => useAppStore.getState().setActiveView(item.view)}
        />
      ))}
      {globalPlugins.map((entry) => (
        <RailButton
          key={entry.pluginKey}
          label={entry.name}
          icon={Plug}
          active={activeView === `plugin:${entry.pluginKey}`}
          onClick={() => useAppStore.getState().setActiveView(`plugin:${entry.pluginKey}`)}
        />
      ))}
      <div className="mt-auto flex w-full flex-col items-center border-t border-border py-1">
        {GLOBAL_RAIL_BOTTOM_ITEMS.map((item) => (
          <RailButton
            key={item.kind}
            label={item.label}
            icon={item.icon}
            // Why: only Settings is a top-level view; Help is a target inside it.
            active={item.kind === 'settings' && activeView === 'settings'}
            onClick={() => {
              if (item.kind === 'settings') {
                useAppStore.getState().openSettingsPage()
                return
              }
              openHelp()
            }}
          />
        ))}
      </div>
    </nav>
  )
}
