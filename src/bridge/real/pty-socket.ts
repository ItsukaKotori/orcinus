import { invokeCommand } from './invoke'

/** WS 数据面端点（规格 §4.7）：`pty_data_endpoint` 的载荷。 */
export type PtyDataEndpoint = { port: number; token: string }

let cached: Promise<PtyDataEndpoint> | null = null

/**
 * WS 数据面端点（规格 §4.7）。端口与 token 在进程生命周期内不变，取一次即缓存；
 * 服务未起时 reject（`BridgeError` 已由 invokeCommand 归一为 `Error`）。
 */
export function fetchPtyDataEndpoint(): Promise<PtyDataEndpoint> {
  cached ??= invokeCommand<PtyDataEndpoint>('pty_data_endpoint')
  return cached
}

/**
 * 诊断探针（非产品路径）：连 `/pty/probe` 发一字节收一字节，验证 webview 到
 * WS 环回服务（规格 §9 风险 1）的连通性。控制台执行
 * `await __probeAdePtyWs()`，期望返回 `'ade pty ws: ok'`。
 * Task 7 会话路由落地后此探针改测真实会话路径，保留为常驻诊断。
 */
export async function __probeAdePtyWs(): Promise<string> {
  const { port, token } = await fetchPtyDataEndpoint()
  return await new Promise<string>((resolve, reject) => {
    const ws = new WebSocket(`ws://127.0.0.1:${port}/pty/probe?token=${token}`)
    const payload = new Uint8Array([0x61]) // 'a'
    const timer = setTimeout(() => {
      ws.close()
      reject(new Error('ade pty ws: timeout'))
    }, 5000)
    ws.binaryType = 'arraybuffer'
    ws.onopen = () => ws.send(payload)
    ws.onmessage = (event) => {
      clearTimeout(timer)
      const echo = new Uint8Array(event.data as ArrayBuffer)
      const ok = echo.length === 1 && echo[0] === payload[0]
      ws.close()
      if (ok) {
        resolve('ade pty ws: ok')
      } else {
        reject(new Error(`ade pty ws: echo mismatch (${echo.length} bytes)`))
      }
    }
    ws.onerror = () => {
      clearTimeout(timer)
      reject(new Error('ade pty ws: connection error'))
    }
  })
}
