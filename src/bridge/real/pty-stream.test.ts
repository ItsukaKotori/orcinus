import { invoke } from '@tauri-apps/api/core'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { PreloadApi } from '../../shared/preload-api/api-types'

const { FakeChannel } = vi.hoisted(() => {
  /** Programmable Channel stub: tests deliver ArrayBuffer / number[] frames by hand. */
  class FakeChannel {
    static instances: FakeChannel[] = []
    onmessage: ((chunk: ArrayBuffer | number[]) => void) | null = null
    constructor() {
      FakeChannel.instances.push(this)
    }
    deliver(chunk: ArrayBuffer | number[]): void {
      this.onmessage?.(chunk)
    }
  }
  return { FakeChannel }
})

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn(), Channel: FakeChannel }))
vi.mock('@tauri-apps/api/event', () => ({ listen: vi.fn() }))

const invokeMock = vi.mocked(invoke)

beforeEach(() => {
  invokeMock.mockReset()
  FakeChannel.instances = []
  vi.resetModules()
})

afterEach(() => {
  vi.unstubAllGlobals()
})

async function importFresh() {
  return await import('./pty-stream')
}

type Handlers = Parameters<(typeof import('./pty-stream'))['openPtyStream']>[1]

function utf8Bytes(text: string): number[] {
  return Array.from(new TextEncoder().encode(text))
}

function utf8Buffer(text: string): ArrayBuffer {
  return new TextEncoder().encode(text).slice().buffer as ArrayBuffer
}

function capturingHandlers(): { handlers: Handlers; received: Array<Record<string, unknown>> } {
  const received: Array<Record<string, unknown>> = []
  return {
    received,
    handlers: {
      onData: (payload) => received.push(payload),
      onAbnormalClose: (id) => received.push({ abnormalClose: id })
    }
  }
}

describe('openPtyStream', () => {
  it('invokes pty_attach with the args envelope and the channel', async () => {
    invokeMock.mockResolvedValue(null)
    const { openPtyStream } = await importFresh()
    const { handlers } = capturingHandlers()
    await openPtyStream('pty-1', handlers)
    expect(invokeMock).toHaveBeenCalledTimes(1)
    const [command, request] = invokeMock.mock.calls[0] as [string, Record<string, unknown>]
    expect(command).toBe('pty_attach')
    expect(request.args).toEqual({ id: 'pty-1' })
    expect(request.channel).toBe(FakeChannel.instances[0])
  })

  it('decodes an ArrayBuffer chunk (≥1KiB Raw form) with the raw byte length', async () => {
    invokeMock.mockResolvedValue(null)
    const { openPtyStream } = await importFresh()
    const { handlers, received } = capturingHandlers()
    await openPtyStream('pty-1', handlers)
    FakeChannel.instances[0]?.deliver(utf8Buffer('héllo')) // 多字节 utf-8
    expect(received).toEqual([{ id: 'pty-1', data: 'héllo', rawLength: 6 }])
  })

  it('decodes a large ArrayBuffer chunk (≥1KiB) verbatim', async () => {
    invokeMock.mockResolvedValue(null)
    const { openPtyStream } = await importFresh()
    const { handlers, received } = capturingHandlers()
    await openPtyStream('pty-1', handlers)
    const text = 'a'.repeat(2048)
    FakeChannel.instances[0]?.deliver(utf8Buffer(text))
    expect(received[0]).toEqual({ id: 'pty-1', data: text, rawLength: 2048 })
  })

  it('decodes a number[] chunk (<1KiB JSON form) with the raw byte length', async () => {
    invokeMock.mockResolvedValue(null)
    const { openPtyStream } = await importFresh()
    const { handlers, received } = capturingHandlers()
    await openPtyStream('pty-1', handlers)
    FakeChannel.instances[0]?.deliver(utf8Bytes('héllo'))
    expect(received).toEqual([{ id: 'pty-1', data: 'héllo', rawLength: 6 }])
  })

  it('delivers frames that arrive while the attach invoke is still pending', async () => {
    let releaseAttach: () => void = () => {}
    invokeMock.mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          releaseAttach = () => resolve(null)
        })
    )
    const { openPtyStream } = await importFresh()
    const { handlers, received } = capturingHandlers()
    const opened = openPtyStream('pty-1', handlers)
    FakeChannel.instances[0]?.deliver(utf8Bytes('early'))
    releaseAttach()
    await opened
    expect(received).toEqual([{ id: 'pty-1', data: 'early', rawLength: 5 }])
  })

  it('attach reject calls onAbnormalClose, rethrows and detaches the channel', async () => {
    invokeMock.mockRejectedValue(new Error('unknown session'))
    const { openPtyStream } = await importFresh()
    const { handlers, received } = capturingHandlers()
    await expect(openPtyStream('ghost', handlers)).rejects.toThrow('unknown session')
    expect(received).toEqual([{ abnormalClose: 'ghost' }])
    FakeChannel.instances[0]?.deliver(utf8Bytes('zombie'))
    expect(received).toEqual([{ abnormalClose: 'ghost' }]) // 通道已哑化
  })

  it('replaces the previous channel on a same-id reopen (host attach takeover)', async () => {
    invokeMock.mockResolvedValue(null)
    const { openPtyStream } = await importFresh()
    const { handlers, received } = capturingHandlers()
    await openPtyStream('pty-1', handlers)
    await openPtyStream('pty-1', handlers)
    expect(FakeChannel.instances).toHaveLength(2)
    FakeChannel.instances[0]?.deliver(utf8Bytes('old'))
    expect(received).toEqual([]) // 旧通道已摘除
    FakeChannel.instances[1]?.deliver(utf8Bytes('new'))
    expect(received).toEqual([{ id: 'pty-1', data: 'new', rawLength: 3 }])
  })
})

describe('closePtyStream', () => {
  it('detaches onmessage so later frames stay inert', async () => {
    invokeMock.mockResolvedValue(null)
    const { openPtyStream, closePtyStream } = await importFresh()
    const { handlers, received } = capturingHandlers()
    await openPtyStream('pty-1', handlers)
    closePtyStream('pty-1')
    FakeChannel.instances[0]?.deliver(utf8Bytes('late'))
    expect(received).toEqual([])
    // 重复 close（多 exit 订阅者）安全。
    expect(() => closePtyStream('pty-1')).not.toThrow()
  })

  it('exit during the attach await drops the late channel (no zombie onData)', async () => {
    let releaseAttach: () => void = () => {}
    invokeMock.mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          releaseAttach = () => resolve(null)
        })
    )
    const { openPtyStream, closePtyStream } = await importFresh()
    const { handlers, received } = capturingHandlers()
    const opened = openPtyStream('pty-1', handlers)
    // pty:exit 在 attach 在飞时到达：表里无通道可摘 + attach 在飞 → 立墓碑。
    closePtyStream('pty-1')
    releaseAttach()
    await opened
    FakeChannel.instances[0]?.deliver(utf8Bytes('zombie'))
    expect(received).toEqual([]) // 晚到的通道被墓碑丢弃
    // 同 id 重生：墓碑已被消费，新 attach 正常落地收数。
    invokeMock.mockResolvedValue(null)
    await openPtyStream('pty-1', handlers)
    FakeChannel.instances[1]?.deliver(utf8Bytes('fresh'))
    expect(received).toEqual([{ id: 'pty-1', data: 'fresh', rawLength: 5 }])
  })

  it('duplicate closes without an in-flight attach record no tombstone (respawn stays live)', async () => {
    invokeMock.mockResolvedValue(null)
    const { openPtyStream, closePtyStream } = await importFresh()
    const { handlers, received } = capturingHandlers()
    await openPtyStream('pty-1', handlers)
    closePtyStream('pty-1') // 第 1 个订阅者摘表
    closePtyStream('pty-1') // 第 2+ 个命中 close-miss，无 attach 在飞 → 不得立碑
    closePtyStream('pty-1')
    await openPtyStream('pty-1', handlers)
    FakeChannel.instances[1]?.deliver(utf8Bytes('live'))
    expect(received).toEqual([{ id: 'pty-1', data: 'live', rawLength: 4 }])
  })

  it('attach reject after an exit tombstone skips the duplicate abnormal close', async () => {
    let rejectAttach: (error: Error) => void = () => {}
    invokeMock.mockImplementationOnce(
      () =>
        new Promise((_, reject) => {
          rejectAttach = reject
        })
    )
    const { openPtyStream, closePtyStream } = await importFresh()
    const { handlers, received } = capturingHandlers()
    const opened = openPtyStream('pty-1', handlers)
    closePtyStream('pty-1') // pty:exit 先行：真实死亡已广播，墓碑立起
    rejectAttach(new Error('session gone'))
    await expect(opened).rejects.toThrow('session gone')
    expect(received).toEqual([]) // 不再补一条本地死亡（pty:exit 为权威）
  })
})

describe('__probeAdePtyStream', () => {
  function stubGlobalPty(onWrite?: (id: string, data: string) => void): {
    spawned: string[]
    written: Array<[string, string]>
    killed: string[]
  } {
    const calls = {
      spawned: [] as string[],
      written: [] as Array<[string, string]>,
      killed: [] as string[]
    }
    vi.stubGlobal('api', {
      pty: {
        spawn: async (opts: { cols: number; rows: number; command?: string }) => {
          calls.spawned.push(opts.command ?? '')
          return { id: 'probe-1' }
        },
        write: (id: string, data: string) => {
          calls.written.push([id, data])
          onWrite?.(id, data)
        },
        kill: async (id: string) => {
          calls.killed.push(id)
        }
      } satisfies Pick<PreloadApi['pty'], 'spawn' | 'write' | 'kill'>
    })
    return calls
  }

  it('spawns via window.api, writes through and reports ok on echo', async () => {
    // 宿主 tty 回显：write 的 'a' 以数字数组形态（<1KiB JSON 帧）下行。
    const calls = stubGlobalPty((_id, data) => {
      FakeChannel.instances[0]?.deliver(utf8Bytes(data))
    })
    invokeMock.mockResolvedValue(null)
    const { __probeAdePtyStream } = await importFresh()
    await expect(__probeAdePtyStream()).resolves.toBe('ade pty stream: ok')
    expect(calls.spawned).toEqual(['cat'])
    expect(calls.written).toEqual([['probe-1', 'a']])
    expect(calls.killed).toEqual(['probe-1'])
  })

  it('rejects with a timeout when no echo comes back and still cleans up', async () => {
    vi.useFakeTimers()
    try {
      const calls = stubGlobalPty()
      invokeMock.mockResolvedValue(null) // attach 成功但永无回显
      const { __probeAdePtyStream } = await importFresh()
      // 同步挂上 settlement 收集器：超时拒绝在 fake timer tick 里发生，
      // 不能等 expect(...).rejects 才挂 handler（会被记为 unhandled rejection）。
      const probing = __probeAdePtyStream()
      const settled = probing.then(
        () => 'ok' as const,
        (error: unknown) => (error as Error).message
      )
      await vi.advanceTimersByTimeAsync(0) // 走到 write
      expect(calls.written).toEqual([['probe-1', 'a']])
      await vi.advanceTimersByTimeAsync(5000)
      expect(await settled).toBe('ade pty stream: timeout')
      expect(calls.killed).toEqual(['probe-1'])
    } finally {
      vi.useRealTimers()
    }
  })

  it('rejects when window.api is absent', async () => {
    const { __probeAdePtyStream } = await importFresh()
    await expect(__probeAdePtyStream()).rejects.toThrow('window.api.pty unavailable')
    expect(invokeMock).not.toHaveBeenCalled()
  })
})
