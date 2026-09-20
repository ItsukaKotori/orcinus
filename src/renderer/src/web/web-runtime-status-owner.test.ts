import { afterEach, beforeEach, expect, it, vi } from 'vitest'
import { WebRuntimeClient } from './web-runtime-client'
import {
  decrypt,
  deriveSharedKey,
  encrypt,
  generateKeyPair,
  publicKeyFromBase64,
  publicKeyToBase64
} from './web-e2ee'
import type { WebPairingOffer } from './web-pairing'

const serverKeys = generateKeyPair()

let pairing: WebPairingOffer
let runtimeId = 'before'
const fakeSockets: FakeWebSocket[] = []
const clients: WebRuntimeClient[] = []

class FakeWebSocket {
  static readonly CONNECTING = 0
  static readonly OPEN = 1
  readyState = FakeWebSocket.CONNECTING
  binaryType = 'arraybuffer'
  onopen: (() => void) | null = null
  onmessage: ((event: { data: unknown }) => void) | null = null
  onclose: (() => void) | null = null
  onerror: (() => void) | null = null
  close = vi.fn()
  send = vi.fn((raw: unknown) => this.respond(raw))
  private serverSharedKey: Uint8Array | null = null
  private serverAuthenticated = false

  constructor(readonly _url: string) {
    fakeSockets.push(this)
    // Why: real sockets open asynchronously; the transport attaches its handlers
    // synchronously right after construction, so defer the open by a microtask.
    queueMicrotask(() => {
      if (this.readyState === FakeWebSocket.CONNECTING) {
        this.readyState = FakeWebSocket.OPEN
        this.onopen?.()
      }
    })
  }

  respond(raw: unknown): void {
    if (!this.serverSharedKey) {
      const hello = JSON.parse(String(raw)) as { publicKeyB64: string }
      this.serverSharedKey = deriveSharedKey(
        serverKeys.secretKey,
        publicKeyFromBase64(hello.publicKeyB64)
      )
      this.onmessage?.({ data: JSON.stringify({ type: 'e2ee_ready' }) })
      return
    }
    const plaintext = decrypt(String(raw), this.serverSharedKey)
    if (!plaintext) {
      return
    }
    const message = JSON.parse(plaintext) as { id?: string; type?: string }
    if (message.type === 'e2ee_auth') {
      this.serverAuthenticated = true
      this.onmessage?.({
        data: encrypt(JSON.stringify({ type: 'e2ee_authenticated' }), this.serverSharedKey)
      })
      return
    }
    if (!this.serverAuthenticated || !message.id) {
      return
    }
    this.onmessage?.({
      data: encrypt(
        JSON.stringify({
          id: message.id,
          ok: true,
          result: { runtimeId, capabilities: [] },
          _meta: { runtimeId: 'runtime-test' }
        }),
        this.serverSharedKey
      )
    })
  }
}

beforeEach(() => {
  runtimeId = 'before'
  fakeSockets.length = 0
  vi.stubGlobal('WebSocket', FakeWebSocket)
  vi.stubGlobal('window', {
    setTimeout,
    clearTimeout,
    setInterval,
    clearInterval,
    atob: (value: string) => Buffer.from(value, 'base64').toString('binary'),
    btoa: (value: string) => Buffer.from(value, 'binary').toString('base64')
  })
  pairing = {
    v: 2,
    endpoint: 'ws://127.0.0.1:6768',
    deviceToken: 'device-token',
    publicKeyB64: publicKeyToBase64(serverKeys.publicKey)
  }
})

afterEach(() => {
  clients.splice(0).forEach((client) => client.close())
  vi.unstubAllGlobals()
})

it('primary browser status follows the authenticated socket and closing it retires the owner', async () => {
  const publish = vi.fn()
  const client = new WebRuntimeClient(pairing, {
    status: { environmentId: 'browser', pairingRevision: 1, publish, verified: vi.fn() }
  })
  clients.push(client)
  await expect
    .poll(() => client.statusOwner?.read().verification, { timeout: 3_000 })
    .toBe('verified')
  expect(client.statusOwner?.read().status?.runtimeId).toBe('before')

  runtimeId = 'after'
  for (const socket of fakeSockets) {
    socket.readyState = 3
    socket.onclose?.()
  }
  await expect
    .poll(() => client.statusOwner?.read().status?.runtimeId, { timeout: 3_000 })
    .toBe('after')
  expect(client.statusOwner?.read().transport).toBe('ready')
  client.close()
  expect(publish.mock.lastCall?.[0]).toMatchObject({ retired: true, verification: 'blocked' })
})
