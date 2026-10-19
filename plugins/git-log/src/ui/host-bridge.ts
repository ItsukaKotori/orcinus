let seq = 0

/** Error carrying the host's stable error code, so the UI can branch on it
 *  (`unknown-oid`, `not-a-git-repo`, …) instead of string-matching the message. */
export class HostCallError extends Error {
  readonly code: string
  constructor(code: string, message: string) {
    super(message)
    this.name = 'HostCallError'
    this.code = code
  }
}

/** 在沙箱 panel/tab 内调宿主方法：postMessage `orca-panel-action` → 等匹配的
 *  `orca-panel-action-result`。`ok` 真则 resolve(value)，否则 reject。opaque
 *  origin，`'*'` 是唯一可用 targetOrigin（宿主按发送 window 校验，非 origin）。 */
const HOST_CALL_TIMEOUT_MS = 5000

export function callHost(action: string, params?: unknown): Promise<unknown> {
  return new Promise((resolve, reject) => {
    const requestId = `gl-${action}-${Date.now()}-${++seq}`
    let timer = 0
    const onResult = (event: MessageEvent): void => {
      const data = event.data as
        | { type?: string; requestId?: string; ok?: boolean; value?: unknown; error?: string; errorCode?: string }
        | undefined
      if (!data || data.type !== 'orca-panel-action-result' || data.requestId !== requestId) return
      clearTimeout(timer)
      window.removeEventListener('message', onResult)
      if (data.ok) resolve(data.value)
      else {
        const code = data.errorCode ?? 'host-call-failed'
        reject(new HostCallError(code, `${code}${data.error ? `: ${data.error}` : ''}`))
      }
    }
    window.addEventListener('message', onResult)
    timer = window.setTimeout(() => {
      window.removeEventListener('message', onResult)
      reject(new Error('host-call-timeout'))
    }, HOST_CALL_TIMEOUT_MS)
    window.parent.postMessage({ type: 'orca-panel-action', requestId, action, params }, '*')
  })
}
