// @vitest-environment happy-dom
import { afterEach, expect, it, vi } from 'vitest'
import { callHost } from '../ui/host-bridge'
import { HostRepository } from './host-repository'
import { PAGE_SIZE } from './query'

const originalParent = window.parent
const requests: { action: string; params: unknown }[] = []

function stubParentReply(reply: Record<string, unknown>): void {
  Object.defineProperty(window, 'parent', {
    configurable: true,
    value: {
      postMessage: (msg: { requestId: string; action: string; params: unknown }): void => {
        requests.push({ action: msg.action, params: msg.params })
        setTimeout(() => {
          window.dispatchEvent(
            new MessageEvent('message', {
              data: { type: 'orca-panel-action-result', requestId: msg.requestId, ...reply },
            })
          )
        }, 0)
      },
    },
  })
}

const flush = (): Promise<void> => new Promise((resolve) => setTimeout(resolve, 0))

afterEach(() => {
  Object.defineProperty(window, 'parent', { configurable: true, value: originalParent })
  requests.length = 0
  vi.restoreAllMocks()
})

it('callHost resolves value on ok', async () => {
  stubParentReply({ ok: true, value: { limit: 200, items: [] } })
  const promise = callHost('git.log', { limit: 200 })
  await flush()
  await expect(promise).resolves.toEqual({ limit: 200, items: [] })
})

it('callHost rejects on not-ok', async () => {
  stubParentReply({ ok: false, errorCode: 'no-active-worktree', error: 'none' })
  const promise = callHost('git.log')
  const assertion = expect(promise).rejects.toThrow(/no-active-worktree/)
  await flush()
  await assertion
})

it('HostRepository.loadCommitFiles posts git.commitFiles and returns entries', async () => {
  stubParentReply({ ok: true, value: [{ path: 'a.ts', status: 'modified' }] })
  const promise = new HostRepository().loadCommitFiles('a'.repeat(40))
  await flush()
  const files = await promise
  expect(files[0]?.path).toBe('a.ts')
})

it('HostRepository.loadRefs posts git.refs and returns { refs, truncated }', async () => {
  stubParentReply({ ok: true, value: { refs: [{ id: 'refs/heads/main', name: 'main', category: 'branches' }], truncated: false } })
  const promise = new HostRepository().loadRefs()
  await flush()
  const result = await promise
  expect(result.refs[0]?.name).toBe('main')
  expect(result.truncated).toBe(false)
  // Discriminating: the outgoing action must be git.refs (not git.log).
  expect(requests[0]?.action).toBe('git.refs')
})

it('HostRepository.loadSnapshot forwards filter/offset and validates the result', async () => {
  stubParentReply({
    ok: true,
    value: { items: [], hasIncomingChanges: false, hasOutgoingChanges: false, hasMore: false, limit: PAGE_SIZE },
  })
  const promise = new HostRepository().loadSnapshot({ refs: 'all', author: 'Lin' }, 100)
  await flush()
  await expect(promise).resolves.toMatchObject({ items: [], hasMore: false })
  // Discriminating: filter + offset actually reach the host call.
  expect(requests[0]?.action).toBe('git.log')
  expect(requests[0]?.params).toEqual({ refs: 'all', author: 'Lin', skip: 100, limit: PAGE_SIZE })
})

it('HostRepository.loadSnapshot rejects a malformed host result', async () => {
  stubParentReply({ ok: true, value: { items: [] } }) // missing required booleans/limit
  const promise = new HostRepository().loadSnapshot()
  await flush()
  await expect(promise).rejects.toThrow()
})
