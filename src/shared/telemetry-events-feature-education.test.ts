import { describe, expect, it } from 'vitest'
import { eventSchemas } from './telemetry-events'

describe('feature education telemetry event schemas', () => {
  it('accepts terminal pane split payloads with known sources', () => {
    expect(
      eventSchemas.terminal_pane_split.safeParse({
        source: 'keyboard',
        direction: 'horizontal'
      }).success
    ).toBe(true)
    expect(
      eventSchemas.terminal_pane_split.safeParse({
        source: 'context_menu',
        direction: 'vertical'
      }).success
    ).toBe(true)
  })

  it('rejects raw or unknown terminal pane split sources', () => {
    expect(
      eventSchemas.terminal_pane_split.safeParse({
        source: 'http://localhost:3000/private',
        direction: 'horizontal'
      }).success
    ).toBe(false)
    expect(
      eventSchemas.terminal_pane_split.safeParse({
        source: 'contextual_tour',
        direction: 'horizontal'
      }).success
    ).toBe(false)
  })
})
