import { Channel } from '@tauri-apps/api/core'
import type { PreloadApi } from '../../shared/preload-api/api-types'
import { invokeCommand } from './invoke'

/**
 * 单帧两形态（task-15-report「Channel 选型」）：≥1KiB 帧走 fetch 队列的
 * ArrayBuffer（`InvokeResponseBody::Raw` 二进制），<1KiB 帧走 eval 的 JSON
 * 数字数组——解码两形态都要兜。
 */
type PtyChunk = ArrayBuffer | number[]

/** 每会话数据面通道的回调（`pty.ts` 的 emitter 接线）。 */
export type PtyStreamHandlers = {
  /** 下行块 → 契约 `onData` 载荷（utf8 解码，`rawLength` 为原始字节数）。 */
  onData: (payload: { id: string; data: string; rawLength: number }) => void
  /** attach 被宿主拒绝（未知 id 等）→ 判会话死亡（规格 §3.2 已知语义）。 */
  onAbnormalClose: (id: string) => void
}

/** 活跃会话 → Channel。多会话并存；同 id 重复 attach 由宿主接管替换（规格 §3.2）。 */
const streams = new Map<string, Channel<PtyChunk>>()

/** 停在 attach invoke 上的 open（in-flight 门控：墓碑只对真正在飞的 attach 有意义）。 */
const attachesInFlight = new Set<string>()

/**
 * 「退出早于通道落地」的墓碑：`pty:exit` 在 `openPtyStream` 停在 attach await
 * 上时到达（规格 §3.2 的窗口），此刻表里无通道可摘；记下 id 让晚到的 attach
 * 落地后直接丢弃该通道——否则死会话的 backlog 字节会以 onData 形态污染渲染层。
 * 原 WS 版的 in-flight/墓碑门控在此坍缩：通道静默结束**不再**本地广播 exit(-1)
 * （渲染层死亡判定严格以 `pty:exit` 事件为权威，规格 §3.2 修订二），墓碑只拦
 * 「僵尸 onData」，不再拦「假死亡」。立碑仍由 `attachesInFlight` 门控：普通
 * exit 时第 1 个订阅者摘表、第 2+ 个全命中 close-miss，无条件立碑会让墓碑无界
 * 增长、Phase 2 的 id 复用/reattach 会被残留墓碑静默吞掉活通道。
 */
const exitedDuringAttach = new Set<string>()

/** 摘除后的通道回调置空为 no-op（Channel 无公开 close，摘除即哑化）。 */
function noopOnMessage(): void {}

function utf8Decode(bytes: ArrayBuffer | Uint8Array): string {
  return new TextDecoder().decode(bytes)
}

/** 单帧两形态归一为字节视图：ArrayBuffer 直包一层；数字数组逐字节收集。 */
function toDoubleBytes(chunk: PtyChunk): Uint8Array<ArrayBuffer> {
  if (chunk instanceof ArrayBuffer) {
    return new Uint8Array(chunk) as Uint8Array<ArrayBuffer>
  }
  // SAFETY: Uint8Array.from always allocates a plain (non-shared) ArrayBuffer.
  return Uint8Array.from(chunk) as Uint8Array<ArrayBuffer>
}

function chunkByteLength(chunk: PtyChunk): number {
  return Array.isArray(chunk) ? chunk.length : chunk.byteLength
}

/**
 * 建一条会话数据面通道：`invoke('pty_attach', { args: { id }, channel })`（规格
 * §3.2 修订二）。`pty_attach` 不在 specta bindings 里（Channel 走手工宏注册），
 * 直接用命令名字符串。宿主先发 backlog 块、再持续转发 outbound 字节块；onmessage
 * 在 invoke 之前挂好，attach 期间先到的 backlog 块不丢。
 *
 * 死亡语义（规格 §3.2 修订二）：渲染层**严格以 `pty:exit` 事件为权威**——宿主
 * 转发结束（通道静默）不广播任何本地死亡；唯一的异常路径是 attach invoke 被拒
 * （spawn 成功后会话即刻消失的竞态等）→ `onAbnormalClose`。
 */
export async function openPtyStream(id: string, handlers: PtyStreamHandlers): Promise<void> {
  const previous = streams.get(id)
  if (previous) {
    streams.delete(id)
    previous.onmessage = noopOnMessage // 本地摘除；宿主侧旧转发已由 attach 接管取消
  }
  const channel = new Channel<PtyChunk>()
  channel.onmessage = (chunk) => {
    handlers.onData({
      id,
      data: utf8Decode(toDoubleBytes(chunk)),
      rawLength: chunkByteLength(chunk)
    })
  }
  attachesInFlight.add(id)
  try {
    await invokeCommand('pty_attach', { args: { id }, channel })
    // await 期间 exit 已到达：丢弃晚到的通道（僵尸 onData 拦截，见墓碑注释）。
    if (exitedDuringAttach.delete(id)) {
      channel.onmessage = noopOnMessage
      return
    }
    streams.set(id, channel)
  } catch (error) {
    // 墓碑在场 = pty:exit 已先行广播过真实死亡，跳过 onAbnormalClose（不重复）；
    // 同时消费掉墓碑，open 已终结、不留跨代次残留。
    const alreadyExited = exitedDuringAttach.delete(id)
    channel.onmessage = noopOnMessage
    if (!alreadyExited) handlers.onAbnormalClose(id)
    throw error
  } finally {
    attachesInFlight.delete(id) // 失败路径同样退出在飞集，不留假门控
  }
}

/**
 * 关闭并摘除会话通道（`pty:exit` 事件 / 本地清理路径）。摘除先行，通道回调哑化；
 * 表里无通道可摘时，仅当该 id 有 attach 停在 await 上才立墓碑（in-flight 门控，
 * 见 `exitedDuringAttach`）——多订阅者的重复 exit 在无 attach 在飞时不得立碑。
 */
export function closePtyStream(id: string): void {
  const channel = streams.get(id)
  if (!channel) {
    if (attachesInFlight.has(id)) exitedDuringAttach.add(id)
    return
  }
  streams.delete(id)
  channel.onmessage = noopOnMessage
}

/**
 * 诊断探针（非产品路径）：真实 spawn 一个 `cat` 会话 → attach 通道 → 写一字节 →
 * 验证 tty 回显经 Channel 下行，验证 webview 到宿主（规格 §3.2 修订二）的数据
 * 面通路。控制台执行 `await __probeAdePtyStream()`，期望返回
 * `'ade pty stream: ok'`。会话用完即 kill；常驻诊断，可重复执行。
 */
export async function __probeAdePtyStream(): Promise<string> {
  const pty = (globalThis as { api?: PreloadApi }).api?.pty
  if (!pty) throw new Error('ade pty stream: window.api.pty unavailable')
  const reply = await pty.spawn({ cols: 80, rows: 24, command: 'cat' })
  const id = reply.id
  let timeoutHandle: ReturnType<typeof setTimeout> | null = null
  try {
    // 只认 write 之后的数据：'cat' 命令行本身的回显也含 'a'，不作数。
    let armed = false
    let signalEcho: (() => void) | null = null
    const echo = new Promise<void>((resolve) => {
      signalEcho = resolve
    })
    await openPtyStream(id, {
      onData: ({ data }) => {
        if (armed && data.includes('a')) signalEcho?.()
      },
      onAbnormalClose: () => {}
    })
    armed = true
    pty.write(id, 'a') // write 语义：fire-and-forget；tty 行律即刻回显
    await new Promise<void>((resolve, reject) => {
      timeoutHandle = setTimeout(() => reject(new Error('ade pty stream: timeout')), 5000)
      echo.then(resolve)
    })
    return 'ade pty stream: ok'
  } finally {
    if (timeoutHandle) clearTimeout(timeoutHandle)
    closePtyStream(id)
    void pty.kill(id).catch(() => {})
  }
}
