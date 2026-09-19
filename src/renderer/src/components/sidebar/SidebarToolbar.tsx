import React from 'react'
import { useTranslation } from 'react-i18next'
import { ScrollToCurrentWorkspaceToolbarButton } from './ScrollToCurrentWorkspaceToolbarButton'
import { SidebarSettingsHelpMenu } from './SidebarSettingsHelpMenu'

const SidebarToolbar = React.memo(function SidebarToolbar() {
  // Why: this memo boundary needs its own language subscription, while
  // translate() preserves Orca's pseudo-localization behavior. Without it the
  // toolbar (and the ScrollToCurrentWorkspaceToolbarButton it renders) keeps
  // whatever language was active at boot — English, since the persisted locale
  // is applied asynchronously after the lazy catalog loads.
  useTranslation()

  return (
    <div className="mt-auto shrink-0">
      <div className="flex items-center justify-between border-t border-worktree-sidebar-border px-2 py-1.5">
        <div className="flex min-w-0 items-center gap-1">
          <SidebarSettingsHelpMenu />
        </div>
        <div className="flex items-center gap-1">
          <ScrollToCurrentWorkspaceToolbarButton />
        </div>
      </div>
    </div>
  )
})

export default SidebarToolbar
