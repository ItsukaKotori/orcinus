import { spawn } from 'node:child_process'

export type RunStream = 'stdout' | 'stderr'

export type RunResult = {
  exitCode: number | null
  signal: string | null
  stdout: string
  stderr: string
}

export type RunOptions = {
  onLine?: (line: string, stream: RunStream) => void
  signal?: AbortSignal
  timeoutMs?: number
  killGraceMs?: number
}

export const MAX_LINE_LENGTH = 4096
export const DEFAULT_TIMEOUT_MS = 120_000

/**
 * Runs one child process with line-buffered capture, a hard timeout, and
 * cancel semantics: SIGTERM first, SIGKILL after `killGraceMs`. The final
 * partial line (no trailing newline) is intentionally dropped.
 */
export function runCommand(cmd: string, args: string[], options: RunOptions = {}): Promise<RunResult> {
  const timeoutMs = options.timeoutMs ?? DEFAULT_TIMEOUT_MS
  const killGraceMs = options.killGraceMs ?? 2_000
  return new Promise<RunResult>((resolve, reject) => {
    const child = spawn(cmd, args, { env: process.env, stdio: ['ignore', 'pipe', 'pipe'] })
    let stdout = ''
    let stderr = ''
    let settled = false
    let killTimer: ReturnType<typeof setTimeout> | null = null

    const timeoutTimer = setTimeout(() => requestKill(), timeoutMs)

    function cleanup() {
      clearTimeout(timeoutTimer)
      if (killTimer !== null) {
        clearTimeout(killTimer)
      }
      options.signal?.removeEventListener('abort', onAbort)
    }

    function finish(run: () => void) {
      if (settled) {
        return
      }
      settled = true
      cleanup()
      run()
    }

    function requestKill() {
      if (child.exitCode !== null || child.signalCode !== null) {
        return
      }
      child.kill('SIGTERM')
      if (killTimer === null) {
        killTimer = setTimeout(() => child.kill('SIGKILL'), killGraceMs)
      }
    }

    function onAbort() {
      requestKill()
    }

    if (options.signal) {
      if (options.signal.aborted) {
        requestKill()
      } else {
        options.signal.addEventListener('abort', onAbort)
      }
    }

    const lineReader = (stream: RunStream, sink: (line: string) => void) => {
      let buffer = ''
      return (chunk: Buffer) => {
        buffer += chunk.toString('utf8')
        let index = buffer.indexOf('\n')
        while (index !== -1) {
          const raw = buffer.slice(0, index).replace(/\r$/, '')
          buffer = buffer.slice(index + 1)
          const line = raw.length > MAX_LINE_LENGTH ? raw.slice(0, MAX_LINE_LENGTH) : raw
          options.onLine?.(line, stream)
          sink(line + '\n')
          index = buffer.indexOf('\n')
        }
      }
    }

    child.stdout.on('data', lineReader('stdout', (line) => {
      stdout += line
    }))
    child.stderr.on('data', lineReader('stderr', (line) => {
      stderr += line
    }))
    child.on('error', (error) => finish(() => reject(error)))
    child.on('close', (exitCode, signal) =>
      finish(() => resolve({ exitCode, signal, stdout, stderr }))
    )
  })
}
