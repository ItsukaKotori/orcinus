import { describe, expect, it } from 'vitest'
import { readTaskPageSource } from './task-page-source-family.test-support'

const CONTENT_SOURCE = readTaskPageSource('task-page/Content.tsx')
const SOURCE_BAR_SOURCE = readTaskPageSource('task-page/SourceBar.tsx')
const SOURCE_CONTEXT_SOURCE = readTaskPageSource('task-page-source-context.tsx')
const RUNTIME_HOSTS_SOURCE = readTaskPageSource('use-task-page-runtime-hosts.ts')

function sourceBetween(source: string, startPattern: string, endPattern: string): string {
  const start = source.indexOf(startPattern)
  expect(start).toBeGreaterThanOrEqual(0)
  const end = source.indexOf(endPattern, start + startPattern.length)
  expect(end).toBeGreaterThan(start)
  return source.slice(start, end)
}

describe('TaskPage source switching host boundary', () => {
  it('renders GitHub item details from the task-detail page owner only', () => {
    const detailSection = sourceBetween(
      CONTENT_SOURCE,
      '<PullRequestPage',
      ") : taskSource === 'github' && githubMode === 'project' ?"
    )

    expect(CONTENT_SOURCE).toContain('<ProjectViewWrapper selectedRepoIds={repoSelection} />')
    expect(detailSection).toContain('workItem={dialogWorkItem}')
    expect(detailSection).toContain('<PullRequestPage')
    expect(detailSection).toContain('sourceContext={dialogSourceContext}')
    expect(detailSection).toContain('<GitHubItemDialog')
    expect(detailSection).toContain('sourceContext={dialogSourceContext}')
    expect(CONTENT_SOURCE.match(/<GitHubItemDialog/g)).toHaveLength(1)
  })

  it('switches task source without mutating the focused run host', () => {
    const section = sourceBetween(
      SOURCE_BAR_SOURCE,
      '{visibleSourceOptions.map((source) => {',
      'className="hidden min-w-0 max-w-[min(420px,40vw)]'
    )

    expect(section).toContain('openTaskPage(')
    expect(section).toContain('taskSource: source.id')
    expect(section).toContain('defaultTaskSource: source.id')
    expect(section).not.toContain('activeRuntimeEnvironmentId')
    expect(section).not.toContain('projectHostSetupId')
    expect(section).not.toContain('workspaceRunContext')
  })

  it('treats missing remote task-source capability as source unavailable', () => {
    const section = sourceBetween(
      SOURCE_CONTEXT_SOURCE,
      'function getTaskSourceHostAvailabilityForHost',
      'function getTaskPageRepoCacheInput'
    )

    expect(section).toContain('TASK_SOURCE_CONTEXT_RUNTIME_CAPABILITY')
    expect(section).toContain("reason: 'checking-task-source-capability'")
    expect(section).toContain("reason: 'missing-task-source-capability'")
  })

  it('checks runtime-owned provider auth on the owning runtime', () => {
    const section = sourceBetween(
      RUNTIME_HOSTS_SOURCE,
      'const runtimeTaskSourceHostIds = useMemo(() => {',
      'const nextModel'
    )

    expect(section).toContain('TASK_SOURCE_CONTEXT_RUNTIME_CAPABILITY')
    expect(section).toContain("'preflight.check'")
    expect(section).toMatch(/\{\s*kind: 'environment',\s*environmentId: parsed\.environmentId\s*\}/)
    expect(RUNTIME_HOSTS_SOURCE).toContain('runtimePreflightStatusByHostId')
  })
})
