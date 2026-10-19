// @vitest-environment happy-dom
import { afterEach, expect, it, vi } from 'vitest'
import { callHost } from '../ui/host-bridge'
import { HostRepository } from './host-repository'

const originalParent = window.parent

function stubParentReply(reply: Record<string, unknown>): void {
  Object.defineProperty(window, 'parent', {
    configurable: true,
    value: {
      postMessage: (msg: { requestId: string }): void => {
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

it('HostRepository.loadRefs posts git.refs and returns refs', async () => {
  stubParentReply({ ok: true, value: [{ id: 'refs/heads/main', name: 'main', category: 'branches' }] })
  const promise = new HostRepository().loadRefs()
  await flush()
  const refs = await promise
  expect(refs[0]?.name).toBe('main')
})

it('HostRepository.loadSnapshot validates the parsed result and forwards filter/offset', async () => {
  stubParentReply({
    ok: true,
    value: { items: [], hasIncomingChanges: false, hasOutgoingChanges: false, hasMore: false, limit: 100 },
  })
  const promise = new HostRepository().loadSnapshot({ refs: 'all', author: 'Lin' }, 100)
  await flush()
  await expect(promise).resolves.toMatchObject({ items: [], hasMore: false })
})
