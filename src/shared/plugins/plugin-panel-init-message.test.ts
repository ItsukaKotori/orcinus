import { describe, expect, it } from 'vitest'
import {
  PANEL_INIT_TYPE,
  createPanelInitMessage,
  parsePanelInitMessage
} from './plugin-panel-bridge'

describe('plugin panel init message', () => {
  it('round-trips opaque params through the guest parser', () => {
    const params = { filter: 'open', nested: [1, { deep: true }] }
    const parsed = parsePanelInitMessage(createPanelInitMessage(params))

    expect(parsed).toEqual({ ok: true, params })
  })

  it('round-trips undefined params as an explicit init frame', () => {
    const message = createPanelInitMessage(undefined)

    expect(message).toEqual({ type: PANEL_INIT_TYPE, params: undefined })
    expect(parsePanelInitMessage(message)).toEqual({ ok: true, params: undefined })
  })

  it('rejects frames that are not host init messages', () => {
    expect(parsePanelInitMessage({ type: 'orca-panel-pong', pingId: 1 })).toEqual({
      ok: false
    })
    expect(parsePanelInitMessage({ params: {} })).toEqual({ ok: false })
    expect(parsePanelInitMessage('orca-panel-init')).toEqual({ ok: false })
    expect(parsePanelInitMessage(null)).toEqual({ ok: false })
  })
})
