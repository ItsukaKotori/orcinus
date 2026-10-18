import { describe, expect, it } from 'vitest'
import { INITIAL_UI_STATE, reducePush } from './state'

describe('reducePush', () => {
  it('replaces the report on state pushes', () => {
    const next = reducePush(INITIAL_UI_STATE, {
      type: 'orca-panel-push',
      topic: 'state',
      payload: { phase: 'done', checkedAt: 1, managers: [], rows: [] }
    })
    expect(next.report).toEqual({ phase: 'done', checkedAt: 1, managers: [], rows: [] })
  })

  it('appends log lines and caps the buffer', () => {
    let state = INITIAL_UI_STATE
    for (let index = 0; index < 3; index += 1) {
      state = reducePush(state, {
        type: 'orca-panel-push',
        topic: 'log',
        payload: { lines: [`line-${index}`] }
      })
    }
    expect(state.logs).toEqual(['line-0', 'line-1', 'line-2'])
    expect(reducePush(state, { type: 'orca-panel-push', payload: 1 })).toBe(state)
  })
})
