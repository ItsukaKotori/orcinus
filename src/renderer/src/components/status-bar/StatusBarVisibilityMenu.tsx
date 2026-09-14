import { Activity, Plug } from 'lucide-react'
import React from 'react'
import {
  DropdownMenu,
  DropdownMenuCheckboxItem,
  DropdownMenuContent,
  DropdownMenuTrigger
} from '@/components/ui/dropdown-menu'
import { translate } from '@/i18n/i18n'
import type { StatusBarController } from './use-status-bar-controller'

export function StatusBarVisibilityMenu({
  controller
}: {
  controller: StatusBarController
}): React.JSX.Element {
  const {
    menuOpen,
    menuPoint,
    recordFeatureInteraction,
    setMenuOpen,
    statusBarItems,
    toggleStatusBarItem
  } = controller

  return (
    <DropdownMenu open={menuOpen} onOpenChange={setMenuOpen} modal={false}>
      <DropdownMenuTrigger asChild>
        <button
          aria-hidden
          tabIndex={-1}
          className="pointer-events-none absolute size-px opacity-0"
          style={{ left: menuPoint.x, top: menuPoint.y }}
        />
      </DropdownMenuTrigger>
      <DropdownMenuContent className="min-w-0 w-fit" sideOffset={0} align="start">
        <DropdownMenuCheckboxItem
          checked={statusBarItems.includes('resource-usage')}
          onCheckedChange={() => {
            recordFeatureInteraction('resource-manager')
            toggleStatusBarItem('resource-usage')
          }}
        >
          <Activity className="size-3.5" />
          {translate('auto.components.status.bar.StatusBar.d1e1a7a6bf', 'Resource Manager')}
        </DropdownMenuCheckboxItem>
        <DropdownMenuCheckboxItem
          checked={statusBarItems.includes('ports')}
          onCheckedChange={() => {
            recordFeatureInteraction('ports')
            toggleStatusBarItem('ports')
          }}
        >
          <Plug className="size-3.5" />
          {translate('auto.components.status.bar.StatusBar.9659e38343', 'Ports')}
        </DropdownMenuCheckboxItem>
      </DropdownMenuContent>
    </DropdownMenu>
  )
}
