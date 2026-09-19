import { describe, expect, it } from 'vitest'

import { AGENT_KIND_VALUES, agentKindToTuiAgent, tuiAgentToAgentKind } from './agent-kind'
import { TUI_AGENT_CONFIG } from './tui-agent-config'
import type { TuiAgent } from './tui-agent'

describe('tuiAgentToAgentKind', () => {
  it('maps every shipped TuiAgent to a concrete kind', () => {
    const agents = Object.keys(TUI_AGENT_CONFIG) as TuiAgent[]

    for (const agent of agents) {
      const kind = tuiAgentToAgentKind(agent)

      expect(kind).not.toBe('other')
      expect(AGENT_KIND_VALUES.includes(kind)).toBe(true)
    }
  })

  it('keeps concrete kinds in exact sync with shipped TuiAgents', () => {
    const agents = Object.keys(TUI_AGENT_CONFIG) as TuiAgent[]
    const mappedKinds = agents.map((agent) => tuiAgentToAgentKind(agent)).sort()
    const concreteSchemaKinds = AGENT_KIND_VALUES.filter((kind) => kind !== 'other').sort()

    expect(mappedKinds).toEqual(concreteSchemaKinds)
  })

  it('uses the product id for Claude and the TuiAgent id for Pi', () => {
    expect(tuiAgentToAgentKind('claude')).toBe('claude-code')
    expect(tuiAgentToAgentKind('pi')).toBe('pi')
  })
})

describe('agentKindToTuiAgent', () => {
  it('round-trips every shipped TuiAgent through its kind', () => {
    const agents = Object.keys(TUI_AGENT_CONFIG) as TuiAgent[]
    for (const agent of agents) {
      expect(agentKindToTuiAgent(tuiAgentToAgentKind(agent))).toBe(agent)
    }
  })

  it("reverses Claude's product id back to the TuiAgent id", () => {
    expect(agentKindToTuiAgent('claude-code')).toBe('claude')
  })

  it('returns null for the catch-all and missing kinds', () => {
    expect(agentKindToTuiAgent('other')).toBeNull()
    expect(agentKindToTuiAgent(null)).toBeNull()
    expect(agentKindToTuiAgent(undefined)).toBeNull()
  })
})
