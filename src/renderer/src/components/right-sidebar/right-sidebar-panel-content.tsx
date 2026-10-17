import { Suspense } from 'react'
import { lazyWithRetry as lazy } from '@/lib/lazy-with-retry'
import type { ActiveRightSidebarTab } from '@/store/slices/editor'
import { isPluginPanelTabKey } from '../../../../shared/plugins/plugin-manifest'

const FileExplorer = lazy(() => import('./FileExplorer'))
const SourceControl = lazy(() => import('./SourceControl'))
const AiVaultPanel = lazy(() => import('./AiVaultPanel'))
const FolderWorkspaceWorktreesPanel = lazy(() => import('./FolderWorkspaceWorktreesPanel'))
const PluginPanel = lazy(() => import('./PluginPanel'))

type RightSidebarPanelContentProps = {
  effectiveTab: ActiveRightSidebarTab
}

export function RightSidebarPanelContent({
  effectiveTab
}: RightSidebarPanelContentProps): React.JSX.Element {
  return (
    <div className="flex min-h-0 flex-1 flex-col overflow-hidden">
      <Suspense fallback={null}>
        {effectiveTab === 'explorer' && <FileExplorer />}
        {effectiveTab === 'source-control' && <SourceControl />}
        {effectiveTab === 'vault' && <AiVaultPanel />}
        {effectiveTab === 'workspaces' && <FolderWorkspaceWorktreesPanel />}
        {/* Plugin-contributed tabs route by key prefix; the panel itself
            handles plugins that have since been uninstalled or disabled.
            Why key: switching plugin tabs must remount the sandboxed iframe —
            a reused frame could keep posting messages while the bridge is
            rebound under the next plugin's identity. */}
        {isPluginPanelTabKey(effectiveTab) && (
          <PluginPanel key={effectiveTab} tabKey={effectiveTab} />
        )}
      </Suspense>
    </div>
  )
}
