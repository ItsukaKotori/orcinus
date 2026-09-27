import { describe, expect, it, vi } from 'vitest'
import { UnimplementedBridgeError } from '../unimplemented-fallback'
import { createHooksApi } from './agent-hook-api'

describe('Phase 0 hooks mock', () => {
  it('is probe-safe: the setup-script methods exist without fabricating rejections', () => {
    const hooks = createHooksApi()
    expect(typeof hooks.check).toBe('function')
    expect(typeof hooks.inspectSetupScriptImports).toBe('function')
  })

  it('answers the workspace setup-script probe benignly', async () => {
    await expect(createHooksApi().check({ repoId: 'repo-1' })).resolves.toEqual({
      status: 'ok',
      hasHooks: false,
      hooks: null,
      mayNeedUpdate: false
    })
  })

  it('reports no setup-script import candidates', async () => {
    await expect(createHooksApi().inspectSetupScriptImports({ repoId: 'repo-1' })).resolves.toEqual(
      []
    )
  })

  it('keeps unimplemented methods loudly rejecting', async () => {
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => {})
    await expect(
      createHooksApi().writeIssueCommand({ repoId: 'repo-1', content: 'echo hi' })
    ).rejects.toBeInstanceOf(UnimplementedBridgeError)
    expect(warn).toHaveBeenCalledWith(expect.stringContaining('hooks.writeIssueCommand'))
    warn.mockRestore()
  })
})
