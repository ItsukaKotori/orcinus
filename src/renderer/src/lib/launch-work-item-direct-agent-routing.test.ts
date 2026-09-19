import { beforeEach, describe, expect, it, vi } from 'vitest'

const mocks = vi.hoisted(() => ({
  preflightAgentTrust: vi.fn()
}))

vi.mock('@/lib/agent-trust-preflight', () => ({
  preflightAgentTrust: mocks.preflightAgentTrust
}))

import { markDirectWorkItemAgentTrusted } from './launch-work-item-direct-agent-routing'

describe('markDirectWorkItemAgentTrusted', () => {
  beforeEach(() => vi.clearAllMocks())

  it('marks trust before a terminal launch', async () => {
    await markDirectWorkItemAgentTrusted({
      agent: 'codex',
      workspacePath: '/repo/worktree',
      connectionId: 'ssh-1'
    })

    expect(mocks.preflightAgentTrust).toHaveBeenCalledWith({
      agent: 'codex',
      workspacePath: '/repo/worktree',
      connectionId: 'ssh-1'
    })
  })
})
