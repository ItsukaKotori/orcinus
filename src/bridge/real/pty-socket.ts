import { invokeCommand } from './invoke'

/** WS 数据面端点（规格 §4.7）：`pty_data_endpoint` 的载荷。 */
export type PtyDataEndpoint = { port: number; token: string }

let cached: Promise<PtyDataEndpoint> | null = null

/**
 * WS 数据面端点（规格 §4.7）。端口与 token 在进程生命周期内不变，成功一次即缓存。
 * 失败不缓存：`.catch` 先清空缓存再重新抛出，端点瞬时失败只损失当次调用、下次
 * 重取（Task 12 审查 Minor-2——`cached ??=` 若留下 rejected promise 会缓存到进程
 * 余生，数据面静默死亡）。PtyHost 随装配即起服务，`pty_data_endpoint` 恒 `Ok`
 * （Task 10 起不再有「服务未起」分支；`BridgeError` 归一仍由 invokeCommand 兜底）。
 */
export function fetchPtyDataEndpoint(): Promise<PtyDataEndpoint> {
  cached ??= invokeCommand<PtyDataEndpoint>('pty_data_endpoint').catch((error: unknown) => {
    cached = null
    throw error
  })
  return cached
}

/** 每会话数据面连接的回调（`pty.ts` 的 emitter 接线）。 */
export type PtySocketHandlers = {
  /** 下行帧 → 契约 `onData` 载荷（utf8 解码，`rawLength` 为原始字节数）。 */
  onData: (payload: { id: string; data: string; rawLength: number }) => void
  /** 非 pty:exit / 替换触发的连接关闭 → 判会话死亡（规格 §3.2 已知语义）。 */
  onAbnormalClose: (id: string) => void
}

/** WS readyState OPEN（字面量而非 `WebSocket.OPEN` 静态量——stub 友好）。 */
const WS_OPEN = 1

/** 活跃会话 → WS。多会话并存；同 id 重连先让位旧连接（规格 §3.2 连接替换）。 */
const sockets = new Map<string, WebSocket>()

/** 停在端点 await 上的 open（in-flight 门控：墓碑只对真正在飞的 open 有意义）。 */
const opensInFlight = new Set<string>()

/**
 * 「退出早于 socket 落地」的墓碑：`pty:exit` 在 `openPtySocket` 停在端点 await 上时
 * 到达（规格 §3.2 的窗口），此刻表里无连接可摘；记下 id 让晚到的 open 直接丢弃
 * 本次连接——否则该 socket 会连上已死的会话，其异常关闭再广播一条假的本地死亡
 * （Task 12 审查 Minor-1 的 duplicate exit 竞态）。立碑由 `opensInFlight` 门控
 * （审查 fix round 1）：真实渲染层有多个 onExit 订阅者（pty-dispatcher、
 * use-resource-session-inventory，`subscribeToEvent` 不去重，Tauri 逐订阅者投递），
 * 普通 exit 时第 1 个订阅者摘表、第 2+ 个全命中 close-miss——此时并没有 open 在飞，
 * 无条件立碑会让墓碑无界增长，且 Phase 2 的 id 复用/reattach open 会被残留墓碑
 * 静默吞掉活连接（open 正常 resolve、无 onData、零报错）。残余窗口（需
 * incarnationId 穿透 socket 层才能根治，Phase 2）：旧代次的 exit 事件在同 id
 * 替换/重生 open 已落地后才送达 → 摘掉的是新代次的活 socket。
 */
const exitedDuringOpen = new Set<string>()

function utf8Decode(buffer: ArrayBuffer): string {
  return new TextDecoder().decode(buffer)
}

export function utf8Encode(data: string): Uint8Array<ArrayBuffer> {
  // SAFETY: TextEncoder.encode always allocates a plain (non-shared) ArrayBuffer.
  return new TextEncoder().encode(data) as Uint8Array<ArrayBuffer>
}

/**
 * 建一条会话数据面连接：`ws://127.0.0.1:<port>/pty/<id>?token=…`（规格 §3.2），
 * 端点经 `fetchPtyDataEndpoint`（缓存）。binary 帧即原始字节：下行喂
 * `onData`，上行（`sendPtySocketData`）直写 master writer。
 *
 * 关闭语义（规格 §3.2 的顺序保证）：`closePtySocket`（pty:exit / 同 id 替换
 * 触发）先把连接摘出表，其 onclose 不再回调；其余关闭按 close code 二分——
 * `1000` 是 server 退出/让位的干净收尾（真实退出码随后由 pty:exit 事件携带，
 * 不重复广播 code:-1），非 1000 即异常断线 → 会话死亡 → `onAbnormalClose`。
 */
export async function openPtySocket(id: string, handlers: PtySocketHandlers): Promise<void> {
  const previous = sockets.get(id)
  if (previous) {
    sockets.delete(id)
    previous.close()
  }
  opensInFlight.add(id)
  try {
    const { port, token } = await fetchPtyDataEndpoint()
    if (exitedDuringOpen.delete(id)) return // await 期间 exit 已到达：丢弃晚到的连接
    const ws = new WebSocket(`ws://127.0.0.1:${port}/pty/${id}?token=${token}`)
    ws.binaryType = 'arraybuffer'
    ws.onmessage = (event) => {
      const buffer = event.data as ArrayBuffer
      handlers.onData({ id, data: utf8Decode(buffer), rawLength: buffer.byteLength })
    }
    ws.onclose = (event) => {
      if (sockets.get(id) !== ws) return // pty:exit / 替换触发的关闭：已被摘除或接管
      sockets.delete(id)
      if (event.code === 1000) return // server 干净收尾，退出码由 pty:exit 携带
      handlers.onAbnormalClose(id)
    }
    sockets.set(id, ws)
  } finally {
    opensInFlight.delete(id) // 失败路径（端点拒绝）同样退出在飞集，不留假门控
  }
}

/**
 * 关闭并摘除会话连接（`pty:exit` 事件 / 本地清理路径）。摘除先行，连接的
 * onclose 因此被判为「有意关闭」，不再触发会话死亡广播。表里无连接可摘时，
 * 仅当该 id 有 open 停在端点 await 上才立墓碑（`opensInFlight` 门控，见
 * `exitedDuringOpen`）——多订阅者的重复 exit 在无 open 在飞时不得立碑。
 */
export function closePtySocket(id: string): void {
  const ws = sockets.get(id)
  if (!ws) {
    if (opensInFlight.has(id)) exitedDuringOpen.add(id)
    return
  }
  sockets.delete(id)
  try {
    ws.close()
  } catch {
    // 已死连接的 close 可能抛（stub/实现差异）；摘除即目的已达成。
  }
}

/**
 * 上行一帧（utf8 编码直写 master writer）。仅 WS 就绪（OPEN）时可写——
 * `write` 静默丢、`writeAccepted` 回 false 的语义由此实现；返回是否已发出。
 */
export function sendPtySocketData(id: string, data: string): boolean {
  const ws = sockets.get(id)
  if (!ws || ws.readyState !== WS_OPEN) return false
  ws.send(utf8Encode(data))
  return true
}

/**
 * 诊断探针（非产品路径）：连 `/pty/probe` 发一字节收一字节，验证 webview 到
 * WS 环回服务（规格 §9 风险 1）的连通性。控制台执行
 * `await __probeAdePtyWs()`，期望返回 `'ade pty ws: ok'`。
 * 注意：生产 server 无名为 probe 的会话（未知 id → close(1008)），回显需自备
 * 回显目标；真实会话路径由 `openPtySocket` 承载，本探针保留为常驻连通性诊断。
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
