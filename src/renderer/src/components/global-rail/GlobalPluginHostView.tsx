import { useAppStore } from '@/store'
import { translate } from '@/i18n/i18n'
import { usePluginPanelEntry } from '../right-sidebar/use-plugin-panel-entry'

function GlobalPluginHostMessage({ children }: { children: React.ReactNode }): React.JSX.Element {
  return (
    <div className="flex h-full min-h-0 flex-1 items-center justify-center p-6 text-center text-sm text-muted-foreground">
      {children}
    </div>
  )
}

/** Host surface for a global plugin: the plugin's first panel contribution
 *  mounted in the shared sandboxed shell/bridge session. */
export function GlobalPluginHostView({
  pluginKey
}: {
  pluginKey: string
}): React.JSX.Element {
  const entry = useAppStore((s) =>
    s.pluginCenterEntries.find(
      (candidate) => candidate.pluginKey === pluginKey && candidate.scope === 'global'
    )
  )
  // Multi-panel global plugins get a rail-side selector later; today the first
  // contributed panel is the surface.
  const panel = entry?.panels[0] ?? null

  const { entryState, iframeRef, panelDocument, panelFrameName, panelFrameKey, handleFrameLoad } =
    usePluginPanelEntry({
      pluginKey: panel ? pluginKey : null,
      panelId: panel?.id ?? null,
      scope: 'global',
      instanceKey: `plugin:${pluginKey}`
    })

  if (!entry || !panel) {
    return (
      <GlobalPluginHostMessage>
        {translate(
          'auto.components.globalRail.GlobalPluginHostView.unavailable',
          '此全局插件没有可用的面板。'
        )}
      </GlobalPluginHostMessage>
    )
  }

  if (entryState.status === 'loading') {
    return (
      <GlobalPluginHostMessage>
        {translate(
          'auto.components.globalRail.GlobalPluginHostView.loading',
          '正在加载全局插件面板…'
        )}
      </GlobalPluginHostMessage>
    )
  }

  if (entryState.status === 'unresponsive') {
    return (
      <GlobalPluginHostMessage>
        {translate(
          'auto.components.globalRail.GlobalPluginHostView.unresponsive',
          '全局插件面板已停止响应。'
        )}
      </GlobalPluginHostMessage>
    )
  }

  if (entryState.status === 'error') {
    return (
      <GlobalPluginHostMessage>
        {translate(
          'auto.components.globalRail.GlobalPluginHostView.loadFailed',
          '全局插件面板加载失败。'
        )}
      </GlobalPluginHostMessage>
    )
  }

  return (
    <iframe
      key={panelFrameKey}
      ref={iframeRef}
      // SECURITY: mirrors PluginPanel — scripts-only sandbox keeps plugin UI on
      // an opaque origin, and srcdoc carries the host CSP shell.
      sandbox="allow-scripts"
      name={panelFrameName}
      srcDoc={panelDocument ?? ''}
      onLoad={handleFrameLoad}
      title={panel.title}
      className="h-full w-full flex-1 border-0 bg-background"
    />
  )
}
