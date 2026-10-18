import {
  PANEL_ACTION_REQUEST_TYPE,
  PANEL_ACTION_RESULT_TYPE,
  PANEL_INIT_TYPE,
  PANEL_PUSH_TYPE,
  parsePanelInitMessage,
  parsePanelPushMessage,
  type PluginPanelPushMessage
} from '../../../../src/shared/plugins/plugin-panel-bridge'

export type PanelActionResult =
  | { ok: true; value: unknown }
  | { ok: false; errorCode?: string; error?: string }

export type HostBridge = {
  ready: Promise<void>
  invokeCommand: (commandId: string, args?: unknown) => Promise<PanelActionResult>
  onPush: (listener: (message: PluginPanelPushMessage) => void) => void
  onInit: (listener: (params: unknown) => void) => void
}

export function createHostBridge(): HostBridge {
  const pushListeners: Array<(message: PluginPanelPushMessage) => void> = []
  const initListeners: Array<(params: unknown) => void> = []
  const pending = new Map<string, (result: PanelActionResult) => void>()
  let sequence = 0
  // Why: the host mounts its message listener before the iframe document runs,
  // so by the time this bridge (and its own listener) exists, a command posted
  // to the parent is already handled. The guest only receives orca-panel-init /
  // -push / -action-result; pongs travel guest→host, so there is no round-trip
  // to await before issuing commands.
  const ready: Promise<void> = Promise.resolve()

  window.addEventListener('message', (event) => {
    const data = event.data as Record<string, unknown> | null
    if (!data || typeof data !== 'object') {
      return
    }
    if (data.type === PANEL_ACTION_RESULT_TYPE) {
      if (typeof data.requestId !== 'string') {
        return
      }
      const resolve = pending.get(data.requestId)
      if (!resolve) {
        return
      }
      pending.delete(data.requestId)
      resolve(
        data.ok === true
          ? { ok: true, value: data.value }
          : {
              ok: false,
              errorCode: typeof data.errorCode === 'string' ? data.errorCode : undefined,
              error: typeof data.error === 'string' ? data.error : undefined
            }
      )
      return
    }
    if (data.type === PANEL_PUSH_TYPE) {
      const parsed = parsePanelPushMessage(data)
      if (parsed.ok) {
        for (const listener of pushListeners) {
          listener(data as PluginPanelPushMessage)
        }
      }
      return
    }
    if (data.type === PANEL_INIT_TYPE) {
      const parsed = parsePanelInitMessage(data)
      if (parsed.ok) {
        for (const listener of initListeners) {
          listener(parsed.params)
        }
      }
    }
  })

  const call = (action: string, params?: unknown): Promise<PanelActionResult> =>
    new Promise((resolve) => {
      const requestId = `req-${++sequence}`
      pending.set(requestId, resolve)
      window.parent.postMessage(
        { type: PANEL_ACTION_REQUEST_TYPE, requestId, action, params },
        '*'
      )
    })

  return {
    ready,
    invokeCommand: (commandId, args) => call('worker.invokeCommand', { commandId, args }),
    onPush: (listener) => {
      pushListeners.push(listener)
    },
    onInit: (listener) => {
      initListeners.push(listener)
    }
  }
}
