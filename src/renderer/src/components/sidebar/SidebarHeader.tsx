import React from 'react'
import { useTranslation } from 'react-i18next'
import { useAppStore } from '@/store'
import { translate } from '@/i18n/i18n'
import { SidebarHeaderActions } from './sidebar-header-actions'

const SidebarHeader = React.memo(function SidebarHeader() {
  // Subscribe this memoized header to locale changes before using translate().
  useTranslation()
  const groupBy = useAppStore((s) => s.groupBy)
  const sidebarTitle =
    groupBy === 'repo'
      ? translate('dashboard.sidebar.projects', 'Projects')
      : translate('dashboard.sidebar.workspaces', 'Workspaces')

  return (
    <div className="mt-2 flex h-8 min-w-0 items-center justify-between gap-1.5 px-2">
      <div className="flex min-w-0 items-center gap-1">
        <span
          // Why truncate: the action cluster is shrink-0, so a long localized title
          // (es "Espacios de trabajo") otherwise wraps out of the h-8 row.
          className="min-w-0 truncate select-none pl-2 pr-0.5 text-xs font-semibold text-muted-foreground/80"
          data-sidebar-section-title={groupBy === 'repo' ? 'projects' : 'workspaces'}
        >
          {sidebarTitle}
        </span>
      </div>
      <div className="flex shrink-0 items-center gap-1">
        <SidebarHeaderActions />
      </div>
    </div>
  )
})

export default SidebarHeader
