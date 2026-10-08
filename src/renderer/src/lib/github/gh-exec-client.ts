import { invokeCommand } from '../../../../bridge/real/invoke'
import type { GhAuthAccount, GhAuthDiagnostic } from '../../../../shared/github/auth-types'

export type GhExecResult = { stdout: string; stderr: string; code: number | null }
export type GhExecOptions = { cwd?: string; timeoutMs?: number; maxBuffer?: number }
export type GhExecutor = (args: string[], options?: GhExecOptions) => Promise<GhExecResult>

export class GhRunError extends Error {
  readonly stderr: string
  readonly stdout: string
  readonly code: number | null

  constructor(result: GhExecResult) {
    super(result.stderr.trim() || `gh exited with code ${result.code ?? 'unknown'}`)
    this.name = 'GhRunError'
    this.stderr = result.stderr
    this.stdout = result.stdout
    this.code = result.code
  }
}

const RETRY_DELAYS_MS = [250, 1000]
const TRANSIENT_PATTERNS = [
  /http[\s/]*5\d\d/i,
  /internal server error/i,
  /bad gateway/i,
  /service unavailable/i,
  /econnreset|etimedout|socket hang up/i
]

function isTransientFailure(result: GhExecResult): boolean {
  const text = `${result.stderr}\n${result.stdout}`
  if (/retry-after:/i.test(text)) {
    return false
  }
  return TRANSIENT_PATTERNS.some((pattern) => pattern.test(text))
}

export function createGhExecClient(executor: GhExecutor): {
  run: (args: string[], options?: GhExecOptions) => Promise<GhExecResult>
  runOrThrow: (args: string[], options?: GhExecOptions) => Promise<string>
} {
  const run = async (args: string[], options?: GhExecOptions): Promise<GhExecResult> => {
    let last: GhExecResult | null = null
    for (let attempt = 0; attempt <= RETRY_DELAYS_MS.length; attempt++) {
      try {
        const result = await executor(args, options)
        if (result.code === 0 && result.code !== null) {
          return result
        }
        last = result
        if (!isTransientFailure(result)) {
          return result
        }
      } catch (error) {
        last = {
          stdout: '',
          stderr: error instanceof Error ? error.message : String(error),
          code: null
        }
        if (!/timeout|timed out|econnreset|socket hang up/i.test(last.stderr)) {
          throw error
        }
      }
      const delay = RETRY_DELAYS_MS[attempt]
      if (delay !== undefined) {
        await new Promise<void>((resolve) => setTimeout(resolve, delay))
      }
    }
    return last ?? { stdout: '', stderr: 'gh exec failed', code: null }
  }
  const runOrThrow = async (args: string[], options?: GhExecOptions): Promise<string> => {
    const result = await run(args, options)
    if (result.code !== 0) {
      throw new GhRunError(result)
    }
    return result.stdout
  }
  return { run, runOrThrow }
}

export function defaultGhExecutor(): GhExecutor {
  return (args, options) =>
    invokeCommand<GhExecResult>('gh_exec', {
      args: {
        args,
        ...(options?.cwd !== undefined ? { cwd: options.cwd } : {}),
        ...(options?.timeoutMs !== undefined ? { timeoutMs: options.timeoutMs } : {}),
        ...(options?.maxBuffer !== undefined ? { maxBuffer: options.maxBuffer } : {})
      }
    })
}

export type { GhAuthAccount, GhAuthDiagnostic }
