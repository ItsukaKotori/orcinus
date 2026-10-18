import type { ManagerId, OutdatedEntry, PackageKind } from '../shared/types'
import { parseBrewOutdated, parseNpmOutdated } from './parse'
import { runCommand, type RunResult } from './runner'

export type RunFn = (cmd: string, args: string[], signal?: AbortSignal) => Promise<RunResult>

export type OutdatedResult = { entries: OutdatedEntry[]; raw: RunResult }

/** One package manager behind the registry: detection, outdated parsing, and
 *  per-package upgrade commands. Adding a manager = adding one adapter. */
export type ManagerAdapter = {
  id: ManagerId
  label: string
  kinds: PackageKind[]
  detect: () => Promise<boolean>
  outdated: () => Promise<OutdatedResult>
  upgradeArgs: (entry: OutdatedEntry) => string[]
  upgrade: (entry: OutdatedEntry, signal?: AbortSignal) => Promise<RunResult>
}

const defaultRun: RunFn = (cmd, args, signal) => runCommand(cmd, args, { signal })

export function createNpmAdapter(run: RunFn = defaultRun): ManagerAdapter {
  return {
    id: 'npm',
    label: 'npm',
    kinds: ['global'],
    detect: async () => {
      try {
        const result = await run('npm', ['--version'])
        return result.exitCode === 0
      } catch {
        return false
      }
    },
    outdated: async () => {
      const raw = await run('npm', ['outdated', '-g', '--json'])
      // npm outdated exits 1 when outdated packages exist; anything else is a failure.
      if (raw.exitCode !== 0 && raw.exitCode !== 1) {
        throw new Error(`npm outdated failed (exit ${raw.exitCode}): ${raw.stderr.trim()}`)
      }
      return { entries: parseNpmOutdated(raw.stdout), raw }
    },
    upgradeArgs: (entry) => ['install', '-g', `${entry.name}@latest`],
    upgrade: (entry, signal) => run('npm', ['install', '-g', `${entry.name}@latest`], signal)
  }
}

export function createBrewAdapter(run: RunFn = defaultRun): ManagerAdapter {
  return {
    id: 'brew',
    label: 'Homebrew',
    kinds: ['formula', 'cask'],
    detect: async () => {
      try {
        const result = await run('brew', ['--version'])
        return result.exitCode === 0
      } catch {
        return false
      }
    },
    outdated: async () => {
      const raw = await run('brew', ['outdated', '--json=v2'])
      if (raw.exitCode !== 0) {
        throw new Error(`brew outdated failed (exit ${raw.exitCode}): ${raw.stderr.trim()}`)
      }
      return { entries: parseBrewOutdated(raw.stdout), raw }
    },
    upgradeArgs: (entry) =>
      entry.kind === 'cask' ? ['upgrade', '--cask', entry.name] : ['upgrade', entry.name],
    upgrade: (entry, signal) =>
      run(
        'brew',
        entry.kind === 'cask' ? ['upgrade', '--cask', entry.name] : ['upgrade', entry.name],
        signal
      )
  }
}

export function defaultAdapters(): ManagerAdapter[] {
  return [createNpmAdapter(), createBrewAdapter()]
}
