import { invoke } from '@tauri-apps/api/core'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { UnimplementedBridgeError } from '../unimplemented-fallback'
import { createGitRealApi } from './git'

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn(async () => ({})) }))

const invokeMock = vi.mocked(invoke)

type GitMethod = keyof ReturnType<typeof createGitRealApi>

beforeEach(() => {
  invokeMock.mockReset()
  invokeMock.mockImplementation(async () => ({}))
})

describe('git real adapter commands', () => {
  it('maps git.status to git_status with the { args } envelope', async () => {
    await createGitRealApi().status({ worktreePath: '/tmp/repo' })
    expect(invokeMock).toHaveBeenCalledWith('git_status', { args: { worktreePath: '/tmp/repo' } })
  })

  const commandCases = [
    ['stage', 'git_stage'],
    ['bulkStage', 'git_bulk_stage'],
    ['unstage', 'git_unstage'],
    ['bulkUnstage', 'git_bulk_unstage'],
    ['discard', 'git_discard'],
    ['bulkDiscard', 'git_bulk_discard'],
    ['commit', 'git_commit'],
    ['diff', 'git_diff'],
    ['cancelStatus', 'git_cancel_status'],
    ['upstreamStatus', 'git_upstream_status'],
    ['conflictOperation', 'git_conflict_operation'],
    ['branchCompare', 'git_branch_compare'],
    ['commitCompare', 'git_commit_compare'],
    ['branchDiff', 'git_branch_diff'],
    ['commitDiff', 'git_commit_diff'],
    ['history', 'git_history']
  ] satisfies Array<[GitMethod, string]>

  it.each(commandCases)('maps git.%s to %s with the { args } envelope', async (method, command) => {
    const api = createGitRealApi() as unknown as Record<
      string,
      (args: unknown) => Promise<unknown>
    >
    await api[method]({ worktreePath: '/tmp/repo' })
    expect(invokeMock).toHaveBeenCalledWith(command, { args: { worktreePath: '/tmp/repo' } })
  })
})

describe('git real adapter payloads and errors', () => {
  it('propagates rejected command errors as Error(message)', async () => {
    invokeMock.mockRejectedValueOnce({ message: 'boom' })
    await expect(createGitRealApi().status({ worktreePath: '/tmp/x' })).rejects.toThrow('boom')
  })

  it('resolves the status payload unchanged', async () => {
    const status = { entries: [], conflictOperation: 'merge', head: 'abc' } as const
    invokeMock.mockResolvedValueOnce(status)
    await expect(createGitRealApi().status({ worktreePath: '/repo' })).resolves.toBe(status)
  })

  it.each([
    [
      'text',
      {
        kind: 'text',
        originalContent: 'before\n',
        modifiedContent: 'after\n',
        originalIsBinary: false,
        modifiedIsBinary: false
      }
    ],
    [
      'binary',
      {
        kind: 'binary',
        originalContent: '',
        modifiedContent: '',
        originalIsBinary: true,
        modifiedIsBinary: false
      }
    ]
  ])('passes a %s diff through without runtime conversion', async (_kind, diff) => {
    invokeMock.mockResolvedValueOnce(diff)
    await expect(
      createGitRealApi().diff({ worktreePath: '/repo', filePath: 'a.txt', staged: false })
    ).resolves.toBe(diff)
  })
})

describe('git real adapter unimplemented surface', () => {
  it.each(['generateCommitMessage', 'setStatusUpstreamRefWatch'] satisfies GitMethod[])(
    'keeps git.%s loud by rejecting UnimplementedBridgeError without invoking',
    async (method) => {
      const warn = vi.spyOn(console, 'warn').mockImplementation(() => {})
      const api = createGitRealApi() as unknown as Record<
        string,
        (args: unknown) => Promise<unknown>
      >
      await expect(api[method]({ worktreePath: '/x' })).rejects.toBeInstanceOf(
        UnimplementedBridgeError
      )
      await expect(api[method]({ worktreePath: '/x' })).rejects.toThrow(`git.${method}`)
      expect(invokeMock).not.toHaveBeenCalled()
      warn.mockRestore()
    }
  )
})
