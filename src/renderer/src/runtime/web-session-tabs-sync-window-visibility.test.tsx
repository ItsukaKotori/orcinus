// @vitest-environment happy-dom

import { act, cleanup, renderHook } from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { tagRuntimeSubscriptionReplayResponse } from '../../../shared/runtime-subscription-replay'
import type { RuntimeMobileSessionTabsResult } from '../../../shared/runtime-types'
import { resetStaleDocumentVisibilityForTesting } from '@/components/terminal-pane/stale-document-visibility'
import type * as WorktreeRuntimeOwnerModule from '@/lib/worktree-runtime-owner'
import type * as WebRuntimeSessionModule from './web-runtime-session'

const mocks = vi.hoisted(() => ({
  createTerminal: vi.fn(),
  getExplicitRuntimeEnvironmentIdForWorktree: vi.fn(),
  recoverSnapshot: vi.fn(),
  runtimeSessionMirrorEnvironmentKey: vi.fn()
}))

vi.mock('./use-runtime-session-mirror-environment-key', () => ({
  useRuntimeSessionMirrorEnvironmentKey: mocks.runtimeSessionMirrorEnvironmentKey
}))

vi.mock('@/lib/worktree-runtime-owner', async (importOriginal) => {
  const actual = await importOriginal<typeof WorktreeRuntimeOwnerModule>()
  return {
    ...actual,
    getExplicitRuntimeEnvironmentIdForWorktree: mocks.getExplicitRuntimeEnvironmentIdForWorktree
  }
})

vi.mock('./web-session-terminal-orphan-recovery', () => ({
  recoverWebSessionTerminalOrphansBeforeApply: mocks.recoverSnapshot
}))

vi.mock('./web-runtime-session', async (importOriginal) => {
  const actual = await importOriginal<typeof WebRuntimeSessionModule>()
  return { ...actual, createWebRuntimeSessionTerminal: mocks.createTerminal }
})

import { useAppStore } from '@/store'
import type { PublicKnownRuntimeEnvironment } from '../../../shared/runtime-environments'
import type { AppState } from '@/store/types'
import { replaceRuntimeEnvironmentRevisions } from './runtime-environment-revision'
import { clearHostLiveTerminalProbesForTests } from './host-live-terminal-probe'
import {
  _getWebSessionTabsRecoveryTrackingCountsForTest,
  _getWebSessionTabsTrackingCountsForTest,
  resetWebSessionTabsSnapshotFreshnessForTests,
  useWebSessionTabsSync,
  WEB_SESSION_TABS_VISIBILITY_RESUME_STAGGER_MS
} from './web-session-tabs-sync'
import { clearRuntimeEnvironmentConnectionGenerationsForTests } from '@/store/slices/runtime-status'
import { WINDOW_VISIBILITY_SUBSCRIPTION_PARK_DELAY_MS } from './window-visibility-subscription-parking'

const ENV_A = 'env-a'
const ENV_B = 'env-b'
const WORKTREE = 'repo-a::worktree-a'
const REVISION_A = 101
const REVISION_B = 201
const MIRROR_KEY = `${ENV_A}\u0001runtime-a\u00010\u0001${REVISION_A}\u0000${ENV_B}\u0001runtime-b\u00010\u0001${REVISION_B}`
const initialState = useAppStore.getInitialState()

type RuntimeSubscribe = typeof window.api.runtimeEnvironments.subscribe
type RuntimeSubscription = {
  request: Parameters<RuntimeSubscribe>[0]
  callbacks: Parameters<RuntimeSubscribe>[1]
  unsubscribe: ReturnType<typeof vi.fn>
}

type Deferred<T> = {
  promise: Promise<T>
  resolve: (value: T) => void
}

const subscriptions: RuntimeSubscription[] = []
const runtimeCall = vi.fn(async (_args: { method: string }) => ({
  id: 'list-all',
  ok: true as const,
  result: { snapshots: [] },
  _meta: { runtimeId: 'runtime-test' }
}))

/** Why: an inventory that publishes nothing now buys one `terminal.list`
 *  readiness probe before it may settle the mirror, so a bare call count no
 *  longer says which questions the client asked (STA-5377). */
function runtimeCallMethods(): string[] {
  return runtimeCall.mock.calls.map(([args]) => args.method).sort()
}

/** Both seeded environments answer an empty inventory, so each owes a probe. */
const EMPTY_INVENTORY_CALLS = [
  'session.tabs.listAll',
  'session.tabs.listAll',
  'terminal.list',
  'terminal.list'
]
const runtimeSubscribe = vi.fn<RuntimeSubscribe>(async (request, callbacks) => {
  const unsubscribe = vi.fn()
  subscriptions.push({ request, callbacks, unsubscribe })
  return { unsubscribe, sendBinary: vi.fn() }
})

function createDeferred<T>(): Deferred<T> {
  let resolve = (_value: T): void => {}
  const promise = new Promise<T>((done) => {
    resolve = done
  })
  return { promise, resolve }
}

function makeEmptySnapshot(snapshotVersion = 1): RuntimeMobileSessionTabsResult {
  return {
    worktree: WORKTREE,
    publicationEpoch: 'epoch-1',
    snapshotVersion,
    activeGroupId: 'host-group-1',
    activeTabId: null,
    activeTabType: null,
    tabs: []
  }
}


function setDocumentVisibility(state: 'visible' | 'hidden'): void {
  Object.defineProperty(document, 'visibilityState', {
    configurable: true,
    get: () => state
  })
  document.dispatchEvent(new Event('visibilitychange'))
}

async function settle(): Promise<void> {
  await Promise.resolve()
  await Promise.resolve()
  await Promise.resolve()
}

function findSubscription(
  method: 'session.tabs.subscribeAll' | 'session.tabs.subscribe',
  selector: string,
  occurrence = 0
): RuntimeSubscription {
  const matches = subscriptions.filter(
    ({ request }) => request.method === method && request.selector === selector
  )
  const subscription = matches[occurrence]
  if (!subscription) {
    throw new Error(`Missing ${method} subscription ${occurrence} for ${selector}`)
  }
  return subscription
}

async function publish(
  subscription: RuntimeSubscription,
  result: unknown,
  replayed = false
): Promise<void> {
  await act(async () => {
    const response = {
      id: 'subscription-event',
      ok: true as const,
      result,
      _meta: { runtimeId: 'runtime-test' }
    }
    subscription.callbacks.onResponse(
      replayed ? tagRuntimeSubscriptionReplayResponse(response) : response
    )
    await settle()
  })
}

function seedRemoteMirrorState(): void {
  const runtimeEnvironments = [
    { id: ENV_A, createdAt: 100, pairingRevision: REVISION_A },
    { id: ENV_B, createdAt: 200, pairingRevision: REVISION_B }
  ] as PublicKnownRuntimeEnvironment[]
  replaceRuntimeEnvironmentRevisions(runtimeEnvironments)
  useAppStore.setState(
    {
      ...initialState,
      activeWorktreeId: WORKTREE,
      workspaceSessionReady: true,
      runtimeEnvironments,
      runtimeStatusByEnvironmentId: new Map([
        [ENV_A, { status: { runtimeId: 'runtime-a' }, connectionGeneration: 1 }],
        [ENV_B, { status: { runtimeId: 'runtime-b' }, connectionGeneration: 2 }]
      ]) as AppState['runtimeStatusByEnvironmentId']
    },
    true
  )
}


describe('useWebSessionTabsSync window visibility', () => {
  beforeEach(() => {
    vi.useFakeTimers()
    subscriptions.length = 0
    runtimeCall.mockClear()
    runtimeSubscribe.mockClear()
    mocks.createTerminal.mockReset().mockResolvedValue(undefined)
    mocks.recoverSnapshot.mockReset().mockImplementation(async (_state, snapshot) => snapshot)
    mocks.getExplicitRuntimeEnvironmentIdForWorktree.mockReset().mockReturnValue(ENV_A)
    mocks.runtimeSessionMirrorEnvironmentKey.mockReset().mockReturnValue(MIRROR_KEY)
    Object.defineProperty(window, 'api', {
      configurable: true,
      value: { runtimeEnvironments: { call: runtimeCall, subscribe: runtimeSubscribe } }
    })
    setDocumentVisibility('visible')
    resetWebSessionTabsSnapshotFreshnessForTests()
    clearHostLiveTerminalProbesForTests()
    seedRemoteMirrorState()
  })

  afterEach(() => {
    cleanup()
    useAppStore.setState(initialState, true)
    replaceRuntimeEnvironmentRevisions([])
    resetWebSessionTabsSnapshotFreshnessForTests()
    clearRuntimeEnvironmentConnectionGenerationsForTests()
    resetStaleDocumentVisibilityForTesting()
    setDocumentVisibility('visible')
    vi.useRealTimers()
  })


  it('parks every live mirror without repeating one-shot hydration', async () => {
    const hook = renderHook(() => useWebSessionTabsSync())
    await act(settle)

    expect(runtimeCallMethods()).toEqual(EMPTY_INVENTORY_CALLS)
    expect(runtimeSubscribe).toHaveBeenCalledTimes(3)
    expect(subscriptions.map(({ request }) => request.method).sort()).toEqual([
      'session.tabs.subscribe',
      'session.tabs.subscribeAll',
      'session.tabs.subscribeAll'
    ])

    act(() => {
      setDocumentVisibility('hidden')
      vi.advanceTimersByTime(WINDOW_VISIBILITY_SUBSCRIPTION_PARK_DELAY_MS - 1)
    })
    expect(subscriptions.every(({ unsubscribe }) => unsubscribe.mock.calls.length === 0)).toBe(true)

    act(() => vi.advanceTimersByTime(1))
    expect(subscriptions.every(({ unsubscribe }) => unsubscribe.mock.calls.length === 1)).toBe(true)

    act(() => setDocumentVisibility('visible'))
    act(() => vi.advanceTimersByTime(WEB_SESSION_TABS_VISIBILITY_RESUME_STAGGER_MS))
    await act(settle)
    expect(runtimeSubscribe).toHaveBeenCalledTimes(6)
    expect(runtimeCallMethods()).toEqual(EMPTY_INVENTORY_CALLS)

    act(() => {
      setDocumentVisibility('hidden')
      vi.advanceTimersByTime(WINDOW_VISIBILITY_SUBSCRIPTION_PARK_DELAY_MS - 1)
      setDocumentVisibility('visible')
      vi.advanceTimersByTime(1)
    })
    expect(runtimeSubscribe).toHaveBeenCalledTimes(6)

    hook.unmount()
    expect(
      subscriptions.slice(3).every(({ unsubscribe }) => unsubscribe.mock.calls.length === 1)
    ).toBe(true)
  })

  it('starts hidden without remote IO and hydrates once on reveal', async () => {
    setDocumentVisibility('hidden')
    const hook = renderHook(() => useWebSessionTabsSync())
    await act(settle)

    expect(runtimeCall).not.toHaveBeenCalled()
    expect(runtimeSubscribe).not.toHaveBeenCalled()

    act(() => setDocumentVisibility('visible'))
    act(() => vi.advanceTimersByTime(WEB_SESSION_TABS_VISIBILITY_RESUME_STAGGER_MS))
    await act(settle)
    expect(runtimeCallMethods()).toEqual(EMPTY_INVENTORY_CALLS)
    expect(runtimeSubscribe).toHaveBeenCalledTimes(3)
    hook.unmount()
  })

  it('does not bootstrap twice when global snapshots race active resume', async () => {
    const pendingCreate = createDeferred<unknown>()
    mocks.createTerminal.mockReturnValue(pendingCreate.promise)
    const hook = renderHook(() => useWebSessionTabsSync())
    await act(settle)
    const snapshot = makeEmptySnapshot()

    await publish(findSubscription('session.tabs.subscribeAll', ENV_A), {
      type: 'snapshots',
      snapshots: [snapshot]
    })
    await publish(findSubscription('session.tabs.subscribe', ENV_A), {
      type: 'snapshot',
      ...snapshot
    })
    expect(mocks.createTerminal).toHaveBeenCalledTimes(1)

    act(() => {
      setDocumentVisibility('hidden')
      vi.advanceTimersByTime(WINDOW_VISIBILITY_SUBSCRIPTION_PARK_DELAY_MS)
      setDocumentVisibility('visible')
      vi.advanceTimersByTime(WEB_SESSION_TABS_VISIBILITY_RESUME_STAGGER_MS)
    })
    await act(settle)

    await publish(findSubscription('session.tabs.subscribeAll', ENV_A, 1), {
      type: 'snapshots',
      snapshots: [snapshot]
    })
    await publish(findSubscription('session.tabs.subscribe', ENV_A, 1), {
      type: 'snapshot',
      ...snapshot
    })
    expect(mocks.createTerminal).toHaveBeenCalledTimes(1)

    pendingCreate.resolve(undefined)
    await act(settle)
    hook.unmount()
  })

  it('fences recovery that finishes after the stream is parked', async () => {
    const deferredRecovery = createDeferred<RuntimeMobileSessionTabsResult>()
    mocks.recoverSnapshot.mockImplementationOnce(() => deferredRecovery.promise)
    const hook = renderHook(() => useWebSessionTabsSync())
    await act(settle)
    const snapshot = makeEmptySnapshot()

    await publish(findSubscription('session.tabs.subscribe', ENV_A), {
      type: 'snapshot',
      ...snapshot
    })
    act(() => {
      setDocumentVisibility('hidden')
      vi.advanceTimersByTime(WINDOW_VISIBILITY_SUBSCRIPTION_PARK_DELAY_MS)
    })
    deferredRecovery.resolve(snapshot)
    await act(settle)
    expect(mocks.createTerminal).not.toHaveBeenCalled()

    act(() => setDocumentVisibility('visible'))
    await act(settle)
    await publish(findSubscription('session.tabs.subscribe', ENV_A, 1), {
      type: 'snapshot',
      ...snapshot
    })
    expect(mocks.createTerminal).toHaveBeenCalledTimes(1)
    hook.unmount()
  })
})
