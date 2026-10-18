import { describe, expect, it, vi } from 'vitest'
import type { RunResult } from './runner'
import { createBrewAdapter, createNpmAdapter, type RunFn } from './managers'

function ok(stdout: string): RunResult {
  return { exitCode: 0, signal: null, stdout, stderr: '' }
}

function result(exitCode: number | null, stdout: string, stderr = ''): RunResult {
  return { exitCode, signal: null, stdout, stderr }
}

describe('npm adapter', () => {
  it('accepts exit 1 from npm outdated as "updates exist" and parses stdout', async () => {
    const run = vi.fn<RunFn>(async () =>
      result(1, JSON.stringify({ typescript: { current: '5.8.3', latest: '5.9.2' } }), '')
    )
    const adapter = createNpmAdapter(run)
    const outdated = await adapter.outdated()
    expect(outdated.entries).toEqual([
      { manager: 'npm', kind: 'global', name: 'typescript', current: '5.8.3', latest: '5.9.2' }
    ])
    expect(run).toHaveBeenCalledWith('npm', ['outdated', '-g', '--json'])
  })

  it('treats other non-zero exits as hard failures', async () => {
    const run = vi.fn<RunFn>(async () => result(2, '', 'npm ERR! network'))
    const adapter = createNpmAdapter(run)
    await expect(adapter.outdated()).rejects.toThrowError(/npm outdated failed \(exit 2\)/)
  })

  it('detects npm from --version and builds latest upgrades', async () => {
    const run = vi.fn<RunFn>(async () => ok('10.15.0\n'))
    const adapter = createNpmAdapter(run)
    await expect(adapter.detect()).resolves.toBe(true)
    expect(adapter.upgradeArgs({ manager: 'npm', kind: 'global', name: 'tsx', current: 'a', latest: 'b' })).toEqual([
      'install',
      '-g',
      'tsx@latest'
    ])
  })

  it('reports missing npm when spawn throws', async () => {
    const run: RunFn = async () => {
      throw new Error('ENOENT')
    }
    await expect(createNpmAdapter(run).detect()).resolves.toBe(false)
  })
})

describe('brew adapter', () => {
  it('parses formulae and casks and maps cask upgrades to --cask', async () => {
    const stdout = JSON.stringify({
      formulae: [{ name: 'node', installed_versions: ['22.13.0'], current_version: '22.21.0' }],
      casks: [{ name: 'docker-desktop', installed_versions: ['4.38.0'], current_version: '4.40.0' }]
    })
    const run = vi.fn<RunFn>(async () => ok(stdout))
    const adapter = createBrewAdapter(run)
    const outdated = await adapter.outdated()
    expect(outdated.entries).toHaveLength(2)
    expect(
      adapter.upgradeArgs({ manager: 'brew', kind: 'cask', name: 'docker-desktop', current: 'a', latest: 'b' })
    ).toEqual(['upgrade', '--cask', 'docker-desktop'])
    expect(
      adapter.upgradeArgs({ manager: 'brew', kind: 'formula', name: 'node', current: 'a', latest: 'b' })
    ).toEqual(['upgrade', 'node'])
  })

  it('fails hard on non-zero exits', async () => {
    const run = vi.fn<RunFn>(async () => result(1, '', 'Error: not a formula'))
    await expect(createBrewAdapter(run).outdated()).rejects.toThrowError(/brew outdated failed \(exit 1\)/)
  })
})
