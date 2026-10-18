import { describe, expect, it } from 'vitest'
import { MAX_LINE_LENGTH, runCommand } from './runner'

describe('runCommand', () => {
  it('captures lines, streams them, and reports the exit code', async () => {
    const lines: Array<{ line: string; stream: string }> = []
    const result = await runCommand(
      process.execPath,
      ['-e', "console.log('one'); console.error('two'); process.exit(3)"],
      { onLine: (line, stream) => lines.push({ line, stream }) }
    )
    expect(result.exitCode).toBe(3)
    expect(result.stdout).toContain('one')
    expect(result.stderr).toContain('two')
    expect(lines).toEqual(
      expect.arrayContaining([
        { line: 'one', stream: 'stdout' },
        { line: 'two', stream: 'stderr' }
      ])
    )
  })

  it('truncates over-long lines to MAX_LINE_LENGTH', async () => {
    const result = await runCommand(process.execPath, ['-e', "console.log('x'.repeat(9000))"])
    expect(result.stdout.trimEnd().length).toBe(MAX_LINE_LENGTH)
  })

  it('kills the child on abort with SIGTERM', async () => {
    const abort = new AbortController()
    const pending = runCommand(process.execPath, ['-e', 'setTimeout(() => {}, 60000)'], {
      signal: abort.signal,
      killGraceMs: 100
    })
    setTimeout(() => abort.abort(), 50)
    const result = await pending
    expect(result.signal).toBe('SIGTERM')
  })

  it('enforces the timeout', async () => {
    const result = await runCommand(process.execPath, ['-e', 'setTimeout(() => {}, 60000)'], {
      timeoutMs: 100,
      killGraceMs: 100
    })
    expect(result.signal).toBe('SIGTERM')
  })

  it('decodes multi-byte characters split across chunks', async () => {
    const script = [
      "const bytes = Buffer.from('中文测试\\n', 'utf8')",
      "process.stdout.write(bytes.subarray(0, 4))",
      "setTimeout(() => process.stdout.write(bytes.subarray(4)), 20)"
    ].join(';')
    const result = await runCommand(process.execPath, ['-e', script])
    expect(result.stdout).toBe('中文测试\n')
    expect(result.stdout).not.toContain('\uFFFD')
  })

  it('rejects when the binary cannot be spawned', async () => {
    await expect(runCommand('__ade_missing_binary__', [])).rejects.toThrowError()
  })
})
