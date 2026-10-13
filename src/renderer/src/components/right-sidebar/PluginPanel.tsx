import React, { useCallback } from 'react'
import { isPluginPanelTabKey } from '../../../../shared/plugins/plugin-manifest'
import { usePluginPanelEntry, type PluginPanelHealth } from './use-plugin-panel-entry'
import { usePluginPanels, usePluginPanelsStore } from '@/store/plugin-panels'
import { translate } from '@/i18n/i18n'

type PluginPanelProps = {
  tabKey: string
}

function PluginPanelMessage({ children }: { children: React.ReactNode }): React.JSX.Element {
  return (
    <div className="flex min-h-0 flex-1 items-center justify-center p-6 text-center text-sm text-muted-foreground">
      {children}
    </div>
  )
}

function PluginPanel({ tabKey }: PluginPanelProps): React.JSX.Element {
  const panels = usePluginPanels()
  const setPanelHealth = usePluginPanelsStore((state) => state.setPanelHealth)
  const panel = isPluginPanelTabKey(tabKey)
    ? (panels.find((entry) => entry.tabKey === tabKey) ?? null)
    : null
  const onHealth = useCallback(
    (health: PluginPanelHealth) => setPanelHealth(tabKey, health),
    [setPanelHealth, tabKey]
  )

  const {
    entryState,
    iframeRef,
    panelDocument,
    panelFrameName,
    panelFrameKey,
    handleFrameLoad
  } = usePluginPanelEntry({
    pluginKey: panel?.pluginKey ?? null,
    panelId: panel?.id ?? null,
    scope: panel?.scope ?? 'global',
    repoId: panel?.repoId,
    instanceKey: tabKey,
    onHealth
  })

  // Persisted plugin tabs can outlive their plugin (uninstalled/disabled);
  // render a graceful empty state instead of a broken frame.
  if (!panel) {
    return (
      <PluginPanelMessage>
        {translate(
          'auto.components.right.sidebar.PluginPanel.unavailable',
          'This plugin panel is no longer available.'
        )}
      </PluginPanelMessage>
    )
  }

  if (entryState.status === 'loading') {
    return (
      <PluginPanelMessage>
        {translate('auto.components.right.sidebar.PluginPanel.loading', 'Loading plugin panel...')}
      </PluginPanelMessage>
    )
  }

  if (entryState.status === 'unresponsive') {
    return (
      <PluginPanelMessage>
        {translate(
          'auto.components.right.sidebar.PluginPanel.unresponsive',
          'This plugin panel stopped responding and was suspended.'
        )}
      </PluginPanelMessage>
    )
  }

  if (entryState.status === 'error') {
    return (
      <PluginPanelMessage>
        {translate(
          'auto.components.right.sidebar.PluginPanel.loadFailed',
          'The plugin panel could not be loaded.'
        )}
      </PluginPanelMessage>
    )
  }

  return (
    <iframe
      key={panelFrameKey}
      ref={iframeRef}
      // SECURITY: never add allow-same-origin — the srcdoc frame must stay an
      // opaque origin so plugin UI cannot reach the app DOM, storage, or IPC.
      // The srcdoc itself is the host CSP shell wrapped around plugin HTML.
      sandbox="allow-scripts"
      name={panelFrameName}
      srcDoc={panelDocument ?? ''}
      onLoad={handleFrameLoad}
      title={panel.title}
      className="h-full w-full flex-1 border-0 bg-background"
    />
  )
}

export default PluginPanel
