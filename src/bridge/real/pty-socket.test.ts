import { invoke } from '@tauri-apps/api/core'
import { beforeEach, describe, expect, it, vi } from 'vitest'

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }))
vi.mock('@tauri-apps/api/event', () => ({ listen: vi.fn() }))

const invokeMock = vi.mocked(invoke)

beforeEach(() => {
  invokeMock.mockReset()
  vi.resetModules()
})

async function importFresh() {
  return await import('./pty-socket')
}

describe('fetchPtyDataEndpoint', () => {
  it('invokes pty_data_endpoint and returns the payload', async () => {
    invokeMock.mockResolvedValueOnce({ port: 51234, token: 'abcd' })
    const { fetchPtyDataEndpoint } = await importFresh()
    await expect(fetchPtyDataEndpoint()).resolves.toEqual({ port: 51234, token: 'abcd' })
    expect(invokeMock).toHaveBeenCalledTimes(1)
    expect(invokeMock).toHaveBeenCalledWith('pty_data_endpoint')
  })

  it('caches the endpoint within the process lifetime', async () => {
    invokeMock.mockResolvedValue({ port: 51234, token: 'abcd' })
    const { fetchPtyDataEndpoint } = await importFresh()
    await fetchPtyDataEndpoint()
    await fetchPtyDataEndpoint()
    expect(invokeMock).toHaveBeenCalledTimes(1)
  })

  it('retries after a failed fetch instead of caching the rejection', async () => {
    invokeMock.mockRejectedValueOnce(new Error('endpoint momentarily down'))
    invokeMock.mockResolvedValue({ port: 51235, token: 'efgh' })
    const { fetchPtyDataEndpoint } = await importFresh()
    // 端点瞬时失败：拒绝当次调用，但不缓存 rejected promise（Task 12 Minor-2）。
    await expect(fetchPtyDataEndpoint()).rejects.toThrow('endpoint momentarily down')
    await expect(fetchPtyDataEndpoint()).resolves.toEqual({ port: 51235, token: 'efgh' })
    // 自愈后缓存照常工作：第三次不再发起 IPC。
    await expect(fetchPtyDataEndpoint()).resolves.toEqual({ port: 51235, token: 'efgh' })
    expect(invokeMock).toHaveBeenCalledTimes(2)
  })
})

describe('__probeAdePtyWs', () => {
  it('connects to /pty/probe with the token and reports an echo match', async () => {
    invokeMock.mockResolvedValueOnce({ port: 51234, token: 'abcd' })
    const { __probeAdePtyWs } = await importFresh()

    const sent: unknown[] = []
    class FakeWebSocket {
      static instances: FakeWebSocket[] = []
      binaryType = 'blob'
      onopen: (() => void) | null = null
      onmessage: ((event: { data: ArrayBuffer }) => void) | null = null
      onerror: (() => void) | null = null
      constructor(public url: string) {
        FakeWebSocket.instances.push(this)
        // 模拟真实事件序：open 在构造之后、处理器赋值完成后的微任务里触发。
        queueMicrotask(() => this.onopen?.())
      }
      send(data: unknown): void {
        sent.push(data)
        const echo = data instanceof Uint8Array ? data.slice() : new Uint8Array([0x61])
        queueMicrotask(() => this.onmessage?.({ data: echo.buffer }))
      }
      close(): void {}
    }
    vi.stubGlobal('WebSocket', FakeWebSocket)

    const result = await __probeAdePtyWs()
    expect(result).toBe('ade pty ws: ok')
    expect(FakeWebSocket.instances[0]?.url).toBe('ws://127.0.0.1:51234/pty/probe?token=abcd')
    expect(sent.length).toBe(1)
    vi.unstubAllGlobals()
  })

  it('rejects when the socket errors', async () => {
    invokeMock.mockResolvedValueOnce({ port: 51234, token: 'abcd' })
    const { __probeAdePtyWs } = await importFresh()

    class FailingWebSocket {
      onopen: (() => void) | null = null
      onmessage: ((event: { data: ArrayBuffer }) => void) | null = null
      onerror: (() => void) | null = null
      constructor() {
        queueMicrotask(() => this.onerror?.())
      }
      send(): void {}
      close(): void {}
    }
    vi.stubGlobal('WebSocket', FailingWebSocket)

    await expect(__probeAdePtyWs()).rejects.toThrow('ade pty ws: connection error')
    vi.unstubAllGlobals()
  })
})
