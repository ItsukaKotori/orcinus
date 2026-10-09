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

describe('git real adapter push', () => {
  const readGitArgs = (payload: unknown): string[] =>
    (payload as { args: { args: string[] } }).args.args

  /** Reads from a fake config table; missing keys answer `code: 1` (git's "not set"). */
  const makeGitReadMock = (options: {
    config?: Record<string, string>
    remoteVerbose?: string
  }): void => {
    const config = options.config ?? {}
    invokeMock.mockImplementation(async (command: string, payload?: unknown) => {
      if (command === 'git_read') {
        const args = readGitArgs(payload)
        if (args[0] === 'symbolic-ref') return { stdout: 'feature\n', stderr: '', code: 0 }
        if (args[0] === 'config' && args[1] === '--get') {
          const key = args[2]
          if (key === undefined) return { stdout: '', stderr: '', code: 1 }
          const value = config[key]
          return value === undefined
            ? { stdout: '', stderr: '', code: 1 }
            : { stdout: `${value}\n`, stderr: '', code: 0 }
        }
        if (args[0] === 'remote' && args[1] === '-v' && options.remoteVerbose !== undefined) {
          return { stdout: options.remoteVerbose, stderr: '', code: 0 }
        }
        throw new Error(`unexpected git_read ${args.join(' ')}`)
      }
      if (command === 'git_push') return null
      throw new Error(`unexpected ${command}`)
    })
  }

  it('falls back to origin HEAD when no push target is configured', async () => {
    invokeMock.mockImplementation(async (command: string, payload?: unknown) => {
      if (command === 'git_read') {
        const args = readGitArgs(payload)
        if (args[0] === 'symbolic-ref') return { stdout: 'feature\n', stderr: '', code: 0 }
        if (args[0] === 'config') return { stdout: '', stderr: '', code: 1 }
        throw new Error(`unexpected git_read ${args.join(' ')}`)
      }
      if (command === 'git_push') return null
      throw new Error(`unexpected ${command}`)
    })
    await createGitRealApi().push({ worktreePath: '/repo', publish: false } as never)
    expect(invokeMock).toHaveBeenCalledWith('git_push', {
      args: { worktreePath: '/repo', remote: 'origin', refspec: 'HEAD', forceWithLease: false }
    })
  })

  it('pushes to the configured branch remote and merge ref', async () => {
    invokeMock.mockImplementation(async (command: string, payload?: unknown) => {
      if (command === 'git_read') {
        const args = readGitArgs(payload)
        if (args[0] === 'symbolic-ref') return { stdout: 'feature\n', stderr: '', code: 0 }
        if (args[0] === 'config' && args[2] === 'branch.feature.remote')
          return { stdout: 'origin\n', stderr: '', code: 0 }
        if (args[0] === 'config' && args[2] === 'branch.feature.merge')
          return { stdout: 'refs/heads/feature\n', stderr: '', code: 0 }
        if (args[0] === 'config') return { stdout: '', stderr: '', code: 1 }
        throw new Error(`unexpected git_read ${args.join(' ')}`)
      }
      if (command === 'git_push') return null
      throw new Error(`unexpected ${command}`)
    })
    await createGitRealApi().push({ worktreePath: '/repo' })
    expect(invokeMock).toHaveBeenCalledWith('git_push', {
      args: { worktreePath: '/repo', remote: 'origin', refspec: 'HEAD:feature', forceWithLease: false }
    })
  })

  it('prefers branch.<branch>.pushRemote over remote.pushDefault', async () => {
    makeGitReadMock({
      config: {
        'branch.feature.pushRemote': 'fork',
        'remote.pushDefault': 'upstream',
        'branch.feature.merge': 'refs/heads/feature'
      }
    })
    await createGitRealApi().push({ worktreePath: '/repo' })
    expect(invokeMock).toHaveBeenCalledWith('git_push', {
      args: { worktreePath: '/repo', remote: 'fork', refspec: 'HEAD:feature', forceWithLease: false }
    })
  })

  it('falls back to remote.pushDefault when the branch has no pushRemote', async () => {
    makeGitReadMock({
      config: {
        'remote.pushDefault': 'upstream',
        'branch.feature.merge': 'refs/heads/feature'
      }
    })
    await createGitRealApi().push({ worktreePath: '/repo' })
    expect(invokeMock).toHaveBeenCalledWith('git_push', {
      args: {
        worktreePath: '/repo',
        remote: 'upstream',
        refspec: 'HEAD:feature',
        forceWithLease: false
      }
    })
  })

  it('falls back to origin HEAD when the push remote does not own the merge branch', async () => {
    makeGitReadMock({
      config: {
        'branch.feature.remote': 'origin',
        'branch.feature.pushRemote': 'fork',
        'branch.feature.merge': 'refs/heads/other'
      }
    })
    await createGitRealApi().push({ worktreePath: '/repo' })
    // Guard check `canPushConfiguredMergeBranch`: branchRef !== branch and the
    // pushRemote differs from the branch remote, so `fork`/`HEAD:other` must be
    // rejected and the plain `origin HEAD` first-publish fallback used instead.
    expect(invokeMock).toHaveBeenCalledWith('git_push', {
      args: { worktreePath: '/repo', remote: 'origin', refspec: 'HEAD', forceWithLease: false }
    })
  })

  it('falls back to origin HEAD when branch.<branch>.base targets the merge branch', async () => {
    makeGitReadMock({
      config: {
        'branch.feature.pushRemote': 'fork',
        'branch.feature.merge': 'refs/heads/feature',
        'branch.feature.base': 'refs/heads/feature'
      }
    })
    await createGitRealApi().push({ worktreePath: '/repo' })
    // Guard check `branchMergeTargetsConfiguredBase`: a base pointing at the
    // merge branch suppresses the configured `fork`/`HEAD:feature` push.
    expect(invokeMock).toHaveBeenCalledWith('git_push', {
      args: { worktreePath: '/repo', remote: 'origin', refspec: 'HEAD', forceWithLease: false }
    })
  })

  it('resolves a URL-valued configured remote to its named remote via remote -v', async () => {
    makeGitReadMock({
      config: {
        'branch.feature.remote': 'https://github.com/o/r.git',
        'branch.feature.merge': 'refs/heads/feature'
      },
      remoteVerbose: [
        'origin\thttps://github.com/o/r.git (fetch)',
        'origin\thttps://github.com/o/r.git (push)',
        ''
      ].join('\n')
    })
    await createGitRealApi().push({ worktreePath: '/repo' })
    expect(invokeMock).toHaveBeenCalledWith('git_read', {
      args: { worktreePath: '/repo', args: ['remote', '-v'] }
    })
    expect(invokeMock).toHaveBeenCalledWith('git_push', {
      args: { worktreePath: '/repo', remote: 'origin', refspec: 'HEAD:feature', forceWithLease: false }
    })
  })

  it('validates an explicit push target before pushing', async () => {
    invokeMock.mockImplementation(async (command: string) => {
      if (command === 'git_read') return { stdout: 'refs/heads/feature\n', stderr: '', code: 0 }
      if (command === 'git_push') return null
      throw new Error(`unexpected ${command}`)
    })
    const api = createGitRealApi()
    await api.push({
      worktreePath: '/repo',
      publish: false,
      pushTarget: { remoteName: 'fork', branchName: 'feature' }
    })
    expect(invokeMock).toHaveBeenCalledWith('git_push', {
      args: { worktreePath: '/repo', remote: 'fork', refspec: 'HEAD:feature', forceWithLease: false }
    })
    expect(invokeMock).toHaveBeenCalledWith('git_read', {
      args: {
        worktreePath: '/repo',
        args: ['check-ref-format', '--branch', 'feature']
      }
    })
  })

  it('passes forceWithLease through to git_push', async () => {
    invokeMock.mockImplementation(async (command: string) => {
      if (command === 'git_read') return { stdout: 'refs/heads/feature\n', stderr: '', code: 0 }
      if (command === 'git_push') return null
      throw new Error(`unexpected ${command}`)
    })
    await createGitRealApi().push({
      worktreePath: '/repo',
      forceWithLease: true,
      pushTarget: { remoteName: 'fork', branchName: 'feature' }
    })
    expect(invokeMock).toHaveBeenCalledWith('git_push', {
      args: { worktreePath: '/repo', remote: 'fork', refspec: 'HEAD:feature', forceWithLease: true }
    })
  })

  it('rejects a push target carrying a remote URL without invoking git', async () => {
    await expect(
      createGitRealApi().push({
        worktreePath: '/repo',
        pushTarget: {
          remoteName: 'fork',
          branchName: 'feature',
          remoteUrl: 'https://github.com/acme/widgets.git'
        }
      })
    ).rejects.toThrow('Push targets with a remote URL are not supported yet.')
    expect(invokeMock).not.toHaveBeenCalled()
  })

  it('rejects an unsafe push target shape without invoking git_push', async () => {
    await expect(
      createGitRealApi().push({
        worktreePath: '/repo',
        pushTarget: { remoteName: '../evil', branchName: 'feature' }
      })
    ).rejects.toThrow('Invalid git remote name: ../evil')
    expect(invokeMock).not.toHaveBeenCalled()
  })

  it('propagates a rejected check-ref-format and does not push', async () => {
    invokeMock.mockImplementation(async (command: string) => {
      if (command === 'git_read') return { stdout: '', stderr: 'fatal: bad ref', code: 1 }
      if (command === 'git_push') return null
      throw new Error(`unexpected ${command}`)
    })
    await expect(
      createGitRealApi().push({
        worktreePath: '/repo',
        pushTarget: { remoteName: 'fork', branchName: 'bad..name' }
      })
    ).rejects.toThrow('fatal: bad ref')
    expect(invokeMock.mock.calls.map(([command]) => command)).toEqual(['git_read'])
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
