import React from 'react'
import { Search } from 'lucide-react'
import { useTranslation } from 'react-i18next'
import { useAppStore } from '@/store'
import { useShortcutKeyComboDetails } from '@/hooks/useShortcutLabel'
import { ShortcutKeyCombo } from '@/components/ShortcutKeyCombo'
import { translate } from '@/i18n/i18n'

const SidebarNav = React.memo(function SidebarNav() {
  // Why: this memo boundary needs its own language subscription, while
  // translate() preserves Orca's pseudo-localization behavior.
  useTranslation()
  const worktreePaletteShortcutCombos = useShortcutKeyComboDetails('worktree.palette')
  const openModal = useAppStore((s) => s.openModal)

  return (
    <div
      className="flex flex-col gap-0.5 px-2 pt-2 pb-1"
    >
      <button
        type="button"
        onClick={() => openModal('worktree-palette')}
        aria-label={translate(
          'auto.components.sidebar.SidebarNav.0c3395fd32',
          'Search worktrees and browser tabs'
        )}
        className="group flex w-full items-center gap-2 rounded-md bg-worktree-sidebar-foreground/5 px-2 py-1.5 text-left text-[13px] font-medium tracking-tight text-worktree-sidebar-foreground/60 transition-colors hover:bg-worktree-sidebar-foreground/8"
      >
        <Search
          className="size-4 shrink-0 text-worktree-sidebar-foreground/30"
          strokeWidth={1.75}
        />
        <span className="flex-1">
          {translate('auto.components.sidebar.SidebarNav.80611a8b10', 'Search')}
        </span>
        <span className="pointer-events-none hidden shrink-0 items-center gap-1 group-hover:flex group-focus-within:flex">
          {worktreePaletteShortcutCombos.map((combo) => (
            <ShortcutKeyCombo
              key={combo.keys.join('-')}
              keys={combo.keys}
              doubleTap={combo.doubleTap}
              className="inline-flex gap-0.5"
              keyCapClassName="min-w-4 border-worktree-sidebar-border/80 bg-worktree-sidebar-foreground/8 px-1 py-px text-[9px] text-worktree-sidebar-foreground/55 shadow-none"
              separatorClassName="text-[9px] text-worktree-sidebar-foreground/45"
            />
          ))}
        </span>
      </button>
    </div>
  )
})

export default SidebarNav
