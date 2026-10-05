import { describe, expect, it, vi } from 'vitest'
import {
  withMethodFallback,
  withUnimplementedFallback,
  UnimplementedBridgeError
} from './unimplemented-fallback'
import { createCacheApi } from './mock/cache-api'
import { createCliApi } from './mock/cli-api'
import { createOnboardingApi } from './mock/onboarding-api'
import { createReposApi } from './mock/repos-api'
import { createRuntimeEnvironmentsApi } from './mock/runtime-environments-api'
import { createRemoteWorkspaceApi } from './mock/workspace-session-api'
import { createPtyApi } from './mock/pty-api'

describe('withUnimplementedFallback', () => {
  it('passes through implemented namespaces', async () => {
    const api = withUnimplementedFallback({ app: { ping: async () => 'pong' } })
    await expect(api.app.ping()).resolves.toBe('pong')
  })

  it('rejects unknown namespace methods with a tagged error', async () => {
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => {})
    const api = withUnimplementedFallback({})
    await expect(api.files.readFile({})).rejects.toBeInstanceOf(UnimplementedBridgeError)
    expect(warn).toHaveBeenCalledWith(expect.stringContaining('files.readFile'))
  })

  it('leaves then and symbol properties unfabricated so the namespace is not thenable', async () => {
    type Namespace = { files: { readFile: (target: unknown) => Promise<string> } }
    const api = withUnimplementedFallback<Namespace>({})
    await expect(Promise.resolve(api.files)).resolves.toBe(api.files)
    expect(Reflect.get(api.files, Symbol.iterator)).toBeUndefined()
  })

  it('answers subscription-shaped methods with a no-op unsubscriber', () => {
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => {})
    const api = withUnimplementedFallback<{
      files: { onChanged: (cb: () => void) => () => void }
    }>({})
    const unsubscribe = api.files.onChanged(() => {})
    expect(typeof unsubscribe).toBe('function')
    expect(() => unsubscribe()).not.toThrow()
    expect(warn).toHaveBeenCalledWith(expect.stringContaining('files.onChanged'))
    warn.mockRestore()
  })

  it('keeps the action-shaped onClientPageRendererRequest rejecting', async () => {
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => {})
    const api = withUnimplementedFallback<{
      browser: { onClientPageRendererRequest: () => Promise<unknown> }
    }>({})
    await expect(api.browser.onClientPageRendererRequest()).rejects.toBeInstanceOf(
      UnimplementedBridgeError
    )
    warn.mockRestore()
  })
})

describe('withMethodFallback', () => {
  it('passes through implemented methods', () => {
    const api = withMethodFallback('browser', { onRequest: () => () => {} })
    expect(typeof api.onRequest()).toBe('function')
  })

  it('rejects unknown methods with the namespaced path', async () => {
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => {})
    const api = withMethodFallback('browser', { onRequest: () => () => {} })
    await expect(api.setViewportOverride({})).rejects.toBeInstanceOf(UnimplementedBridgeError)
    expect(warn).toHaveBeenCalledWith(
      expect.stringContaining('browser.setViewportOverride')
    )
  })

  it('leaves then and symbol properties unfabricated so it is not thenable', async () => {
    const api = withMethodFallback('browser', { onRequest: () => () => {} })
    await expect(Promise.resolve(api)).resolves.toBe(api)
    expect(Reflect.get(api, Symbol.iterator)).toBeUndefined()
  })

  it('answers missing subscription-shaped methods with a no-op unsubscriber', () => {
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => {})
    const api = withMethodFallback<{ onThing?: (cb: () => void) => () => void }>('pty', {})
    const unsubscribe = api.onThing?.(() => {})
    expect(typeof unsubscribe).toBe('function')
    expect(() => unsubscribe?.()).not.toThrow()
    warn.mockRestore()
  })
})

describe('pty mock subscriptions keep effect cleanup safe', () => {
  it('returns unsubscribers instead of promises for every mounted listener', () => {
    const api = createPtyApi()
    for (const method of [
      'onData',
      'onReplay',
      'onWriteUnavailable',
      'onClearBufferRequest',
      'onSerializeBufferRequest',
      'onExit',
      'onSpawned'
    ] as const) {
      const unsubscribe = (api[method] as (cb: () => void) => unknown)(() => {})
      expect(typeof unsubscribe, method).toBe('function')
    }
    expect(() => api.resize('pty-1', 80, 24)).not.toThrow()
  })
})

describe('mock namespaces reject missing methods through the method-level fallback', () => {
  it('tags each rejection with the namespace path instead of a sync TypeError', async () => {
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => {})
    // onboarding implements its whole type, so the missing-method probe is not on the declared type.
    const onboarding = createOnboardingApi() as unknown as {
      forceComplete: () => Promise<unknown>
    }
    const cases: Array<{ path: string; call: () => Promise<unknown> }> = [
      { path: 'onboarding.forceComplete', call: () => onboarding.forceComplete() },
      { path: 'cli.install', call: () => createCliApi().install() },
      { path: 'repos.add', call: () => createReposApi().add({ path: '/tmp/repo' }) },
      {
        path: 'runtimeEnvironments.resolve',
        call: () => createRuntimeEnvironmentsApi().resolve({ selector: 'env-1' })
      },
      // Why not session.flush: the Phase 2A mock session sub-surface implements
      // all six contract methods, so the probe moved to the cache domain.
      {
        path: 'cache.setGitHub',
        call: () => createCacheApi().setGitHub({ cache: { pr: {}, issue: {} } })
      },
      {
        path: 'remoteWorkspace.get',
        call: () => createRemoteWorkspaceApi().get({ targetId: 'target-1' })
      }
    ]

    for (const { path, call } of cases) {
      await expect(call()).rejects.toBeInstanceOf(UnimplementedBridgeError)
      expect(warn).toHaveBeenCalledWith(expect.stringContaining(path))
    }
  })
})
