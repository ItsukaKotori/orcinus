import { readFileSync } from 'node:fs'
import { join } from 'node:path'
import { describe, expect, it } from 'vitest'

const COMPONENT_ROOT = __dirname

function componentSource(relativePath: string): string {
  return readFileSync(join(COMPONENT_ROOT, relativePath), 'utf8')
}

function sourceBetween(source: string, startPattern: string, endPattern: string): string {
  const start = source.indexOf(startPattern)
  expect(start).toBeGreaterThanOrEqual(0)
  const end = source.indexOf(endPattern, start + startPattern.length)
  expect(end).toBeGreaterThan(start)
  return source.slice(start, end)
}

describe('feature interaction writer boundaries', () => {
  it('keeps Cmd+J feature writers in open/selection handlers, not query or navigation rendering', () => {
    // Selection and create callbacks now live in focused hooks; the surface only renders them.
    const selectionSource = componentSource('use-worktree-jump-palette-selection-actions.ts')
    const lifecycleSource = componentSource('use-worktree-jump-palette-selection-lifecycle.ts')
    const createSource = componentSource('use-worktree-jump-palette-create-action.ts')
    const handlerSection = [selectionSource, lifecycleSource, createSource].join('\n')
    const renderSection = componentSource('worktree-jump-palette-surface.tsx')

    const cmdJWriterPattern = /recordFeatureInteraction\('cmd-j/g
    const allCmdJWriterCount = handlerSection.match(cmdJWriterPattern)?.length ?? 0
    expect(allCmdJWriterCount).toBeGreaterThanOrEqual(6)
    expect(handlerSection.match(cmdJWriterPattern)?.length ?? 0).toBe(allCmdJWriterCount)
    expect(renderSection).not.toContain("recordFeatureInteraction('cmd-j")
    expect(
      sourceBetween(lifecycleSource, 'const handleQueryChange', 'const cancelFallbackFocusFrames')
    ).not.toContain("recordFeatureInteraction('cmd-j")
  })

  it('records Cmd+J create-workspace as its own destination, not a generic quick action', () => {
    const source = componentSource('use-worktree-jump-palette-selection-actions.ts')
    const section = sourceBetween(
      source,
      'const handleSelectQuickAction',
      'const handleSelectProjectTarget'
    )

    expect(section).toContain("recordFeatureInteraction('cmd-j-create-workspace')")
    expect(section).toContain("recordFeatureInteraction('cmd-j-quick-action')")
    expect(section.indexOf("recordFeatureInteraction('cmd-j-create-workspace')")).toBeLessThan(
      section.indexOf("recordFeatureInteraction('cmd-j-quick-action')")
    )
    expect(
      sourceBetween(
        section,
        "if (action.id === 'create-workspace')",
        "recordFeatureInteraction('cmd-j-quick-action')"
      )
    ).toContain('return')
  })













  it('records floating workspace hide only from explicit disable or hide actions', () => {
    const allowedSources = [
      componentSource('settings/FloatingWorkspacePane.tsx'),
      componentSource('floating-terminal/FloatingTerminalIconContextMenu.tsx')
    ].join('\n')
    const passiveSources = [
      componentSource('../App.tsx'),
      componentSource('floating-terminal/FloatingTerminalPanel.tsx')
    ].join('\n')

    expect(
      allowedSources.match(/recordFeatureInteraction\('floating-workspace-hidden'\)/g) ?? []
    ).toHaveLength(2)
    expect(passiveSources).not.toContain("recordFeatureInteraction('floating-workspace-hidden')")
  })
})
