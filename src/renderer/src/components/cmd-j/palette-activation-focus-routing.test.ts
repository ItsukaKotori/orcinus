import { readFileSync } from 'node:fs'
import { join } from 'node:path'
import { describe, expect, it } from 'vitest'

// Why: happy-dom does not reproduce Chromium's display:none focus no-op, so the #9939 regression
// is invisible to behavioral tests. Pin the routing decision at the source level instead.
function paletteSource(fileName: string): string {
  return readFileSync(join(__dirname, '..', fileName), 'utf8')
}

function sourceBetween(source: string, startPattern: string, endPattern: string): string {
  const start = source.indexOf(startPattern)
  expect(start).toBeGreaterThanOrEqual(0)
  const end = source.indexOf(endPattern, start + startPattern.length)
  expect(end).toBeGreaterThan(start)
  return source.slice(start, end)
}

describe('Cmd+J activation focus routing (#9939)', () => {
  it('routes worktree selection through the scoped helper before any unscoped fallback', () => {
    const handler = sourceBetween(
      paletteSource('use-worktree-jump-palette-selection-actions.ts'),
      'const handleSelectWorktree = useCallback',
      'const handleSelectWorkspaceTab'
    )

    expect(handler).toContain('queueWorkspaceActivationTerminalFocus(worktree.id, activation)')
    // The fallback must be reachable only when the helper declines the destination.
    expect(handler).toMatch(
      /if \(!queueWorkspaceActivationTerminalFocus\(worktree\.id, activation\)\) \{\s*focusFallbackSurface\(\)\s*\}/
    )
    // An unconditional fallback is the exact shape of the original bug, so the only bare call
    // allowed is the guarded one inside the if-block above.
    expect(handler.match(/focusFallbackSurface\(\)/g)?.length).toBe(1)
  })

  it('restores the pre-palette element for project targets instead of the first terminal found', () => {
    const handler = sourceBetween(
      paletteSource('use-worktree-jump-palette-selection-actions.ts'),
      'const handleSelectProjectTarget = useCallback',
      'const handleSelectItem = useCallback'
    )

    expect(handler).toContain('focusFallbackSurface(previousFocusElementRef.current)')
    expect(handler).not.toMatch(/focusFallbackSurface\(\)/)
  })

})
