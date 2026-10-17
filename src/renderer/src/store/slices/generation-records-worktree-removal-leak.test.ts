/**
 * Memory-leak regression: commit-message-generation records must be evicted
 * when their worktree is removed.
 *
 * The record map is keyed by worktree and retains generated message text. The
 * slice ships a tested `pruneCommitMessageGenerationRecords` action, but
 * nothing in production called it on worktree removal, so the records
 * accumulated one entry per worktree for the renderer session. `removeWorktree`
 * now prunes the map to the surviving worktree set.
 */
import { describe, it, expect, vi, beforeEach } from 'vitest'
import type * as AgentStatusModule from '@/lib/agent-status'

vi.mock('sonner', () => ({
  toast: { info: vi.fn(), success: vi.fn(), error: vi.fn(), warning: vi.fn() }
}))

vi.mock('@/components/terminal-pane/pty-dispatcher', () => ({
  restorePtyDataHandlersAfterFailedShutdown: vi.fn(),
  unregisterPtyDataHandlers: vi.fn()
}))

vi.mock('@/lib/agent-status', async (importOriginal) => {
  const actual = await importOriginal<typeof AgentStatusModule>()
  return { ...actual, detectAgentStatusFromTitle: vi.fn().mockReturnValue(null) }
})

const mockApi = {
  worktrees: {
    list: vi.fn().mockResolvedValue([]),
    remove: vi.fn().mockResolvedValue(undefined),
    forceDeletePreservedBranch: vi.fn().mockResolvedValue({ deleted: true }),
    updateMeta: vi.fn().mockResolvedValue({})
  },
  pty: { kill: vi.fn().mockResolvedValue(undefined) },
  runtimeEnvironments: { call: vi.fn() }
}

// @ts-expect-error -- minimal window.api stub for the store under test
globalThis.window = { api: mockApi }

import { createTestStore, seedStore, makeWorktree } from './store-test-helpers'
import {
  getCommitMessageGenerationRecordKey,
  type CommitMessageGenerationRecord
} from './commit-message-generation'

const REPO = 'repo1'
const WT = 'repo1::/path/wt1'
const WT_PATH = '/path/wt1'
const OTHER = 'repo1::/path/wt2'
const OTHER_PATH = '/path/wt2'

function commitRecord(worktreeId: string, worktreePath: string): CommitMessageGenerationRecord {
  return {
    context: { worktreeId, worktreePath, requestId: 1 },
    status: 'succeeded',
    message: 'a generated commit message',
    error: null,
    hydrated: true
  }
}

function commitKey(worktreeId: string, worktreePath: string): string {
  const key = getCommitMessageGenerationRecordKey(worktreeId, worktreePath)
  if (!key) {
    throw new Error('expected a commit generation key')
  }
  return key
}

describe('worktree removal evicts generation records (leak regression)', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    mockApi.worktrees.remove.mockResolvedValue(undefined)
  })

  it('removes commit generation records for the removed worktree and keeps others', async () => {
    const store = createTestStore()
    seedStore(store, {
      worktreesByRepo: {
        repo1: [
          makeWorktree({ id: WT, repoId: REPO, path: WT_PATH }),
          makeWorktree({ id: OTHER, repoId: REPO, path: OTHER_PATH })
        ]
      }
    })
    store
      .getState()
      .setCommitMessageGenerationRecord(commitKey(WT, WT_PATH), commitRecord(WT, WT_PATH))
    store
      .getState()
      .setCommitMessageGenerationRecord(
        commitKey(OTHER, OTHER_PATH),
        commitRecord(OTHER, OTHER_PATH)
      )

    const result = await store.getState().removeWorktree({ id: WT, executionHostId: null })
    expect(result).toEqual({ ok: true })

    const s = store.getState()
    // Removed worktree's records are gone (the leak).
    expect(s.commitMessageGenerationRecords[commitKey(WT, WT_PATH)]).toBeUndefined()
    // Surviving worktree's records are preserved (no over-pruning).
    expect(s.commitMessageGenerationRecords[commitKey(OTHER, OTHER_PATH)]).toBeDefined()
  })
})
