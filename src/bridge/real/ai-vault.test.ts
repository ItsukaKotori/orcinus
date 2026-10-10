import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { UnimplementedBridgeError } from '../unimplemented-fallback'
import { createAiVaultRealApi } from './ai-vault'

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }))
vi.mock('@tauri-apps/api/event', () => ({ listen: vi.fn() }))

const invokeMock = vi.mocked(invoke)
const listenMock = vi.mocked(listen)

beforeEach(() => {
  invokeMock.mockReset()
  listenMock.mockReset()
})

describe('aiVault real adapter', () => {
  it('routes listSessions with only the fields the command declares, stripping the host scope', async () => {
    invokeMock.mockResolvedValueOnce({ sessions: [], issues: [], scannedAt: 'now' })
    const result = await createAiVaultRealApi().listSessions({
      limit: 250,
      force: true,
      scopePaths: ['/repo'],
      unlimited: false,
      requestToken: 'token-1',
      executionHostScope: 'local'
    })
    expect(result).toEqual({ sessions: [], issues: [], scannedAt: 'now' })
    expect(invokeMock).toHaveBeenCalledWith('ai_vault_list_sessions', {
      args: {
        limit: 250,
        force: true,
        scopePaths: ['/repo'],
        unlimited: false,
        requestToken: 'token-1'
      }
    })
    // Why: `executionHostScope` is renderer-side routing only; the command has no
    // such field and the wire stays self-describing without it.
    expect(invokeMock.mock.calls[0]?.[1]).not.toHaveProperty('args.executionHostScope')
  })

  it('omits fields the caller did not provide instead of materializing nulls', async () => {
    invokeMock.mockResolvedValueOnce({ sessions: [], issues: [], scannedAt: 'now' })
    await createAiVaultRealApi().listSessions({
      limit: undefined,
      force: false,
      scopePaths: undefined,
      unlimited: undefined,
      requestToken: undefined,
      executionHostScope: 'local'
    })
    expect(invokeMock).toHaveBeenCalledWith('ai_vault_list_sessions', { args: { force: false } })
  })

  it('passes no args through when listSessions is called without arguments', async () => {
    invokeMock.mockResolvedValueOnce({ sessions: [], issues: [], scannedAt: 'now' })
    await createAiVaultRealApi().listSessions()
    expect(invokeMock).toHaveBeenCalledWith('ai_vault_list_sessions', { args: undefined })
  })

  it('routes resolveSessionTitles with the declared scope hint and requests', async () => {
    invokeMock.mockResolvedValueOnce({ titles: [] })
    const args = {
      executionHostScope: 'local' as const,
      requests: [{ agent: 'claude' as const, sessionId: 's1', transcriptPath: '/t/s1.jsonl' }]
    }
    await expect(createAiVaultRealApi().resolveSessionTitles(args)).resolves.toEqual({ titles: [] })
    expect(invokeMock).toHaveBeenCalledWith('ai_vault_resolve_session_titles', { args })
  })

  it('routes cancelListSessions with exactly the request token', async () => {
    invokeMock.mockResolvedValueOnce(null)
    await expect(
      createAiVaultRealApi().cancelListSessions({ requestToken: 'token-1' })
    ).resolves.toBeNull()
    expect(invokeMock).toHaveBeenCalledWith('ai_vault_cancel_list_sessions', {
      args: { requestToken: 'token-1' }
    })
  })

  it('routes prepareSessionResume with the resume payload', async () => {
    invokeMock.mockResolvedValueOnce({ useRealCodexHome: true })
    const args = {
      agent: 'codex' as const,
      filePath: '/h/session.jsonl',
      codexHome: '/h/.codex',
      sessionId: 's2',
      executionHostId: 'local' as const
    }
    await expect(createAiVaultRealApi().prepareSessionResume(args)).resolves.toEqual({
      useRealCodexHome: true
    })
    expect(invokeMock).toHaveBeenCalledWith('ai_vault_prepare_session_resume', { args })
  })

  it('routes deleteSession with the delete payload', async () => {
    invokeMock.mockResolvedValueOnce({ outcome: 'deleted' })
    const args = {
      agent: 'claude' as const,
      sessionId: 's3',
      filePath: '/p/s3.jsonl',
      executionHostId: 'local' as const
    }
    await expect(createAiVaultRealApi().deleteSession(args)).resolves.toEqual({
      outcome: 'deleted'
    })
    expect(invokeMock).toHaveBeenCalledWith('ai_vault_delete_session', { args })
  })

  it('subscribes onWindowFocused to tauri://focus, forwards the callback, and unsubscribes', async () => {
    const unlisten = vi.fn()
    listenMock.mockResolvedValueOnce(unlisten)
    const callback = vi.fn()
    const stop = createAiVaultRealApi().onWindowFocused(callback)
    expect(listenMock).toHaveBeenCalledWith('tauri://focus', expect.any(Function))
    const listener = listenMock.mock.calls[0]?.[1] as unknown as (event: unknown) => void
    listener({ event: 'tauri://focus', id: 0, payload: null })
    expect(callback).toHaveBeenCalledTimes(1)
    stop()
    await Promise.resolve()
    expect(unlisten).toHaveBeenCalledTimes(1)
    stop()
    expect(unlisten).toHaveBeenCalledTimes(1)
  })

  it('drops a pending focus registration unsubscribed before listen resolves', async () => {
    const unlisten = vi.fn()
    listenMock.mockResolvedValueOnce(unlisten)
    const stop = createAiVaultRealApi().onWindowFocused(() => {})
    stop()
    await Promise.resolve()
    await Promise.resolve()
    expect(unlisten).toHaveBeenCalledTimes(1)
  })

  it.each(['searchSessions', 'searchStatus', 'listSubagentSessions', 'getFirstUserPrompt'] as const)(
    'keeps %s on the rejecting fallback',
    async (method) => {
      const warn = vi.spyOn(console, 'warn').mockImplementation(() => {})
      const api = createAiVaultRealApi() as unknown as Record<
        string,
        (...args: unknown[]) => unknown
      >
      await expect(api[method]?.({})).rejects.toBeInstanceOf(UnimplementedBridgeError)
      expect(invokeMock).not.toHaveBeenCalled()
      warn.mockRestore()
    }
  )
})
