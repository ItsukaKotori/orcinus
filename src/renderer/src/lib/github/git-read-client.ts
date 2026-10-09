import { invokeCommand } from '../../../../bridge/real/invoke'

export type GitReadResult = { stdout: string; stderr: string; code: number | null }
export type GitReadExecutor = (args: string[]) => Promise<GitReadResult>

export class GitReadError extends Error {
  readonly code: number | null
  readonly stderr: string

  constructor(result: GitReadResult) {
    super(result.stderr.trim() || `git read exited with code ${result.code ?? 'unknown'}`)
    this.name = 'GitReadError'
    this.code = result.code
    this.stderr = result.stderr
  }
}

export function createRunGit(
  executor: GitReadExecutor
): (args: string[]) => Promise<{ stdout: string }> {
  return async (args) => {
    const result = await executor(args)
    if (result.code !== 0) {
      throw new GitReadError(result)
    }
    return { stdout: result.stdout }
  }
}

export function defaultGitReadExecutor(worktreePath: string): GitReadExecutor {
  return (args) => invokeCommand<GitReadResult>('git_read', { args: { worktreePath, args } })
}
