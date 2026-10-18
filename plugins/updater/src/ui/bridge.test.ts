// @vitest-environment happy-dom
import { afterEach, describe, expect, it, vi } from 'vitest'
import { PANEL_ACTION_REQUEST_TYPE } from '../../../../src/shared/plugins/plugin-panel-bridge'
import { createHostBridge } from './bridge'

afterEach(() => {
  vi.restoreAllMocks()
})

describe('createHostBridge', () => {
  it('resolves ready without any host round-trip', async () => {
    const bridge = createHostBridge()
    await expect(bridge.ready).resolves.toBeUndefined()
  })

  it('posts worker.invokeCommand to the parent and resolves on the result frame', async () => {
    const postMessage = vi.spyOn(window, 'postMessage').mockImplementation(() => {})
    const bridge = createHostBridge()
    const pending = bridge.invokeCommand('check')

    expect(postMessage).toHaveBeenCalledTimes(1)
    const frame = postMessage.mock.calls[0]?.[0] as {
      type: string
      requestId: string
      action: string
      params: { commandId: string }
    }
    expect(frame.type).toBe(PANEL_ACTION_REQUEST_TYPE)
    expect(frame.action).toBe('worker.invokeCommand')
    expect(frame.params.commandId).toBe('check')
    expect(typeof frame.requestId).toBe('string')

    window.dispatchEvent(
      new MessageEvent('message', {
        data: { type: 'orca-panel-action-result', requestId: frame.requestId, ok: true, value: { started: true } }
      })
    )
    await expect(pending).resolves.toEqual({ ok: true, value: { started: true } })
  })

  it('delivers pushes to subscribers', () => {
    const bridge = createHostBridge()
    const received: unknown[] = []
    bridge.onPush((message) => received.push(message))
    window.dispatchEvent(
      new MessageEvent('message', {
        data: { type: 'orca-panel-push', topic: 'log', payload: { lines: ['x'] } }
      })
    )
    expect(received).toEqual([{ type: 'orca-panel-push', topic: 'log', payload: { lines: ['x'] } }])
  })
})
