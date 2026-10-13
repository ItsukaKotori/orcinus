import { useEffect } from 'react'
import { useAppStore } from '@/store'

/**
 * Bridges `plugins:open-tab` events (emitted by the Rust `workspace.openTab`
 * host method) into the unified tab store. Renders nothing; mounted once with
 * the other app background services. The backend already deduped by
 * `(pluginKey, tabId, params)` and re-emits the existing instance id, so the
 * store action focuses the existing tab instead of creating a duplicate.
 */
export function PluginOpenTabGate(): null {
  useEffect(() => {
    const dispose = window.api.plugins.onOpenTab((request) => {
      useAppStore.getState().openPluginTab({
        pluginKey: request.pluginKey,
        tabId: request.tabId,
        scope: request.scope,
        ...(request.repoId ? { repoId: request.repoId } : {}),
        instanceId: request.instanceId,
        ...(request.title ? { title: request.title } : {}),
        ...(request.params !== undefined ? { params: request.params } : {}),
        worktreeId: request.worktreeId
      })
    })
    return () => {
      // Why: React StrictMode tears the effect down between its two dev mounts;
      // the idempotent unsubscribe keeps the remount from double-subscribing.
      dispose()
    }
  }, [])

  return null
}
