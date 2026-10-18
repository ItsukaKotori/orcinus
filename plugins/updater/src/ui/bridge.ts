import {
  PANEL_ACTION_REQUEST_TYPE,
  PANEL_ACTION_RESULT_TYPE,
  PANEL_INIT_TYPE,
  PANEL_PONG_TYPE,
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
  let connected = false
  let resolveReady: () => void = () => {}
  const ready = new Promise<void>((resolve) => {
    resolveReady = resolve
  })

  window.addEventListener('message', (event) => {
    const data = event.data as Record<string, unknown> | null
    if (!data || typeof data !== 'object') {
      return
    }
    if (data.type === PANEL_PONG_TYPE && !connected) {
      connected = true
      resolveReady()
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
