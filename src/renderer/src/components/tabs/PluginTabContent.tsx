import React from 'react'
import type { Tab } from '../../../../shared/tab-types'
import { usePluginPanelEntry } from '../right-sidebar/use-plugin-panel-entry'
import { translate } from '@/i18n/i18n'

type PluginTabContentProps = {
  /** Active unified tab whose `contentType` is `'plugin'`. */
  tab: Tab
  /** Closes this unified tab from a state that renders no live frame. */
  onClose: () => void
}

function PluginTabMessage({
  children,
  onClose
}: {
  children: React.ReactNode
  onClose: () => void
}): React.JSX.Element {
  return (
    <div className="flex h-full min-h-0 w-full flex-1 flex-col items-center justify-center gap-3 p-6 text-center text-sm text-muted-foreground">
      <div>{children}</div>
      <button
        type="button"
        onClick={onClose}
        className="rounded-md border border-border px-2.5 py-1 text-xs font-medium text-foreground hover:bg-accent"
      >
        {translate('auto.components.tabs.PluginTabContent.close', '关闭标签页')}
      </button>
    </div>
  )
}

/**
 * Main-area renderer for a `contributes.tabs` entry (spec §3.6): loads the
 * CSP-wrapped shell + session through `readTabEntry`, mounts it in the same
 * sandboxed iframe/bridge session as right-sidebar panels, and posts the
 * persisted `workspace.openTab` params as `orca-panel-init` after the shell's
 * first pong. Entry/session plumbing is shared with PluginPanel via
 * `usePluginPanelEntry`.
 */
export default function PluginTabContent({
  tab,
  onClose
}: PluginTabContentProps): React.JSX.Element {
  const binding = tab.pluginTab
  const { entryState, iframeRef, panelDocument, panelFrameName, panelFrameKey, handleFrameLoad } =
    usePluginPanelEntry({
      pluginKey: binding?.pluginKey ?? null,
      panelId: null,
      tabId: binding?.tabId ?? null,
      instanceId: binding?.instanceId,
      worktreeId: tab.worktreeId,
      scope: binding?.scope ?? 'global',
      ...(binding?.repoId ? { repoId: binding.repoId } : {}),
      instanceKey: tab.id,
      ...(binding?.params !== undefined ? { initParams: binding.params } : {})
    })

  // A persisted plugin tab can outlive its binding (older/hand-edited state);
  // render the closeable unavailable state instead of a broken frame.
  if (!binding) {
    return (
      <PluginTabMessage onClose={onClose}>
        {translate('auto.components.tabs.PluginTabContent.unavailable', '该插件 Tab 已不可用。')}
      </PluginTabMessage>
    )
  }

  if (entryState.status === 'loading') {
    return (
      <PluginTabMessage onClose={onClose}>
        {translate('auto.components.tabs.PluginTabContent.loading', '正在加载插件 Tab…')}
      </PluginTabMessage>
    )
  }

  if (entryState.status === 'unresponsive') {
    return (
      <PluginTabMessage onClose={onClose}>
        {translate(
          'auto.components.tabs.PluginTabContent.unresponsive',
          '该插件 Tab 已停止响应。'
        )}
      </PluginTabMessage>
    )
  }

  if (entryState.status === 'error') {
    return (
      <PluginTabMessage onClose={onClose}>
        {translate(
          'auto.components.tabs.PluginTabContent.loadFailed',
          '该插件 Tab 不可用（插件可能已禁用或卸载）。'
        )}
      </PluginTabMessage>
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
      title={binding.titleHint ?? binding.tabId}
      className="h-full w-full flex-1 border-0 bg-background"
    />
  )
}
