import { readFileSync } from 'node:fs'
import { join } from 'node:path'
import { describe, expect, it } from 'vitest'

function readSource(fileName: string): string {
  return readFileSync(join(__dirname, fileName), 'utf8').replace(/\r\n?/g, '\n')
}

const MODEL_SOURCE = readSource('smart-workspace-name-field-model.ts')
const AVAILABILITY_SOURCE = readSource('use-smart-workspace-field-availability.ts')
const FOCUS_SOURCE = readSource('use-smart-workspace-field-focus-controls.ts')
const STATE_SOURCE = readSource('use-smart-workspace-name-field-state.ts')
const INPUT_SOURCE = readSource('smart-workspace-name-input-surface.tsx')
const SURFACE_SOURCE = readSource('smart-workspace-name-field-surface.tsx')

describe('SmartWorkspaceNameField repo-backed source boundaries', () => {
  it('reports the active source mode without lifting source search state', () => {
    expect(MODEL_SOURCE).toContain('onActiveSourceModeChange?: (mode: SmartNameMode) => void')
    expect(AVAILABILITY_SOURCE).toContain('onActiveSourceModeChange?.(mode)')
    expect(AVAILABILITY_SOURCE).toContain('[mode, onActiveSourceModeChange]')
  })

  it('defers the source popover until composer interaction', () => {
    expect(STATE_SOURCE).toContain('deferSourcePopoverUntilInteractionRef')
    expect(FOCUS_SOURCE).toContain('handleSourcePopoverOpenChange')
    expect(INPUT_SOURCE).toContain('isComposerFieldToFieldFocus')
    expect(INPUT_SOURCE).toContain('onPointerDown={() => {')
    expect(INPUT_SOURCE).toContain('markSourcePopoverUserEngaged()')
  })

  it('confines source-mode overflow to the source strip', () => {
    expect(SURFACE_SOURCE).toContain('overflow-x-auto overflow-y-hidden px-0 scrollbar-sleek')
  })
})
