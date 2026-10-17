import { readFileSync } from 'node:fs'
import { join } from 'node:path'
import { describe, expect, it } from 'vitest'

const WORKTREE_CONTEXT_SOURCE = readFileSync(
  join(__dirname, 'source-control/listing/use-worktree-context.ts'),
  'utf8'
)
const BASE_REF_DEFAULT_SOURCE = readFileSync(
  join(__dirname, 'source-control/sync/use-base-ref-default.ts'),
  'utf8'
)

function sourceBetween(source: string, startPattern: string, endPattern: string): string {
  const start = source.indexOf(startPattern)
  expect(start).toBeGreaterThanOrEqual(0)
  const end = source.indexOf(endPattern, start + startPattern.length)
  expect(end).toBeGreaterThan(start)
  return source.slice(start, end)
}

describe('SourceControl host-context boundaries', () => {
  it('keeps eligibility base refreshes scoped to repo execution ownership', () => {
    const ownerSettingsSection = sourceBetween(
      WORKTREE_CONTEXT_SOURCE,
      'const activeRepoSettings = useMemo(',
      'const activeRepoRuntimeEnvironmentId'
    )
    expect(ownerSettingsSection).not.toContain('activeRepo ?? null')
    expect(ownerSettingsSection).toContain(
      '[activeRepoConnectionId, activeRepoExecutionHostId, activeRepoId, settings]'
    )

    const baseRefSection = sourceBetween(
      BASE_REF_DEFAULT_SOURCE,
      '// Why: reset to null so that effectiveBaseRef becomes falsy until the IPC',
      '  return defaultBaseRef'
    )
    expect(baseRefSection).toContain(
      'getRuntimeRepoBaseRefDefault(\n      { activeRuntimeEnvironmentId: activeRepoRuntimeEnvironmentId },\n      activeRepoId'
    )
    const dependencyBlock = sourceBetween(baseRefSection, '  }, [', '  ])')
    const dependencyEntries = dependencyBlock
      .split('\n')
      .slice(1)
      .map((line) => line.trim().replace(/,$/, ''))
      .filter((line) => line.length > 0 && !line.startsWith('//'))
    expect(dependencyEntries).toEqual([
      'activeRepoConnectionId',
      'activeRepoExecutionHostId',
      'activeRepoId',
      'activeRepoRuntimeEnvironmentId',
      'isBranchVisible',
      'isFolder'
    ])
  })
})
