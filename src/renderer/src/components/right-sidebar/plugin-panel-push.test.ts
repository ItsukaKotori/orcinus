import { describe, expect, it, vi } from 'vitest'
import { forwardPanelPush } from './plugin-panel-push'

function fakeWindow() {
  return { postMessage: vi.fn() } as unknown as Window
}

describe('forwardPanelPush', () => {
  it('posts a camelCase push into the matching session frame', () => {
    const target = fakeWindow()
    const forwarded = forwardPanelPush(() => target, 'token-a', {
      sessionToken: 'token-a',
      topic: 'state',
      payload: { n: 1 }
    })
    expect(forwarded).toBe(true)
    expect(target.postMessage).toHaveBeenCalledWith(
      { type: 'orca-panel-push', topic: 'state', payload: { n: 1 } },
      '*'
    )
  })

  it('drops pushes for other sessions and missing frames', () => {
    const target = fakeWindow()
    expect(
      forwardPanelPush(() => target, 'token-a', { sessionToken: 'token-b', payload: 1 })
    ).toBe(false)
    expect(
      forwardPanelPush(() => null, 'token-a', { sessionToken: 'token-a', payload: 1 })
    ).toBe(false)
    expect(
      forwardPanelPush(() => target, null, { sessionToken: 'token-a', payload: 1 })
    ).toBe(false)
  })
})
