import type { PluginPanelPushRequest } from '../../../../shared/preload-api/api/plugin-host-api'
import { createPanelPushMessage } from '../../../../shared/plugins/plugin-panel-bridge'

/** Forwards one `plugins:panel-push` payload into the plugin iframe whose
 *  session matches. Returns false when the push is not for this frame. */
export function forwardPanelPush(
  getPanelWindow: () => Window | null,
  sessionToken: string | null,
  push: PluginPanelPushRequest
): boolean {
  if (!sessionToken || push.sessionToken !== sessionToken) {
    return false
  }
  const target = getPanelWindow()
  if (!target) {
    return false
  }
  target.postMessage(createPanelPushMessage(push.topic, push.payload), '*')
  return true
}
