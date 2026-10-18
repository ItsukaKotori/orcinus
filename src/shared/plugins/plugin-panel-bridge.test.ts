import { describe, expect, it } from 'vitest'
import {
  PANEL_PUSH_TYPE,
  createPanelPushMessage,
  parsePanelPushMessage
} from './plugin-panel-bridge'

describe('orca-panel-push', () => {
  it('round-trips topic and opaque payload', () => {
    const message = createPanelPushMessage('log', { line: 'hi' })
    expect(message).toEqual({ type: PANEL_PUSH_TYPE, topic: 'log', payload: { line: 'hi' } })
    expect(parsePanelPushMessage(message)).toEqual({ ok: true, topic: 'log', payload: { line: 'hi' } })
  })

  it('rejects malformed pushes and oversize topics', () => {
    expect(parsePanelPushMessage({ type: 'other' }).ok).toBe(false)
    expect(parsePanelPushMessage({ type: PANEL_PUSH_TYPE, topic: 'x'.repeat(65), payload: 1 }).ok).toBe(false)
    expect(parsePanelPushMessage({ type: PANEL_PUSH_TYPE }).ok).toBe(false)
  })

  it('omits an absent topic', () => {
    expect(createPanelPushMessage(undefined, 1)).toEqual({ type: PANEL_PUSH_TYPE, payload: 1 })
  })
})
