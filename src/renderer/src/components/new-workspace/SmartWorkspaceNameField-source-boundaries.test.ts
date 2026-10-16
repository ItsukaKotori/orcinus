import { readFileSync } from 'node:fs'
import { join } from 'node:path'
import { describe, expect, it } from 'vitest'

function readSource(fileName: string): string {
  return readFileSync(join(__dirname, fileName), 'utf8').replace(/\r\n?/g, '\n')
}

const MODEL_SOURCE = readSource('smart-workspace-name-field-model.ts')
const CONTROLLER_SOURCE = readSource('use-smart-workspace-name-field-controller.ts')
const FOUNDATION_SOURCE = readSource('use-smart-workspace-name-field-foundation.ts')
const AVAILABILITY_SOURCE = readSource('use-smart-workspace-field-availability.ts')
const FOCUS_SOURCE = readSource('use-smart-workspace-field-focus-controls.ts')
const STATE_SOURCE = readSource('use-smart-workspace-name-field-state.ts')
const GITHUB_SOURCE = readSource('use-smart-workspace-github-search.ts')
const SECONDARY_SEARCH_SOURCE = readSource('use-smart-workspace-secondary-searches.ts')
const ACTIONS_SOURCE = readSource('use-smart-workspace-name-field-actions.ts')
const COPY_SOURCE = readSource('smart-workspace-name-field-copy.ts')
const INPUT_SOURCE = readSource('smart-workspace-name-input-surface.tsx')
const SURFACE_SOURCE = readSource('smart-workspace-name-field-surface.tsx')
const DIALOG_SOURCE = readSource('smart-workspace-cross-repo-dialog.tsx')

function sourceBetween(source: string, startPattern: string, endPattern: string): string {
  const start = source.indexOf(startPattern)
  expect(start).toBeGreaterThanOrEqual(0)
  const end = source.indexOf(endPattern, start + startPattern.length)
  expect(end).toBeGreaterThan(start)
  return source.slice(start, end)
}

describe('SmartWorkspaceNameField repo-backed source boundaries', () => {
  it('resets hidden repo-backed modes and stale results when source lookup is disabled', () => {
    const modeResetSection = sourceBetween(
      AVAILABILITY_SOURCE,
      'useEffect(() => {\n    if (availableModes.some((item) => item.id === mode))',
      'const focusControls'
    )

    expect(modeResetSection).toContain("setMode(availableModes[0]?.id ?? 'text')")
    expect(modeResetSection).toContain('repoBackedSourcesDisabled')
    expect(modeResetSection).toContain('setGithubItems([])')
    expect(modeResetSection).toContain('setBranches([])')
    expect(modeResetSection).toContain('setCrossRepoPrompt(null)')

    const availableModesSection = sourceBetween(
      AVAILABILITY_SOURCE,
      'const availableModes = getSmartWorkspaceNameModes().filter',
      'useEffect(() => {'
    )
    expect(availableModesSection).toContain('return !repoBackedSourcesDisabled')
    expect(availableModesSection).toContain('branchesEnabled && !repoBackedSourcesDisabled')
    expect(CONTROLLER_SOURCE).toContain('repoBackedSourcesDisabled')

    const placeholderSection = sourceBetween(
      COPY_SOURCE,
      'const smartPlaceholder = repoBackedSourcesDisabled',
      'return {'
    )
    expect(placeholderSection).toContain('Type a workspace name')
    expect(placeholderSection).toContain('Search branches')
  })

  it('can hide the global add-project cross-repo action for subordinate task sources', () => {
    expect(MODEL_SOURCE).toContain('allowCrossRepoProjectAdd?: boolean')
    expect(CONTROLLER_SOURCE).toContain('allowCrossRepoProjectAdd = true')
    expect(ACTIONS_SOURCE).toContain('!crossRepoPrompt || !allowCrossRepoProjectAdd')
    expect(DIALOG_SOURCE).toContain(') : allowCrossRepoProjectAdd ? (')
  })

  it('searches repo-backed task sources through implicit repo targets instead of a menu', () => {
    // The menu declared a prop, derived a visibility flag, and rendered a control; implicit repo
    // targets replaced all three, so each former host is pinned separately.
    expect(MODEL_SOURCE).not.toContain('repoBackedSourceOptions')
    expect(CONTROLLER_SOURCE).not.toContain('repoBackedSourceOptions')
    expect(FOUNDATION_SOURCE).not.toContain('repoBackedSourceOptions')
    expect(SURFACE_SOURCE).not.toContain('RepoBackedSourceMenu')
    expect(INPUT_SOURCE).not.toContain('RepoBackedSourceMenu')
    expect(MODEL_SOURCE).toContain('repoBackedSearchRepos?: readonly RepoOption[]')

    const targetSection = sourceBetween(
      FOUNDATION_SOURCE,
      'const repoBackedSearchTargets = useMemo',
      'const state = useSmartWorkspaceNameFieldState'
    )

    expect(targetSection).toContain('repoBackedSearchRepos.length > 0')
    expect(targetSection).toContain('githubSourceContext')

    expect(CONTROLLER_SOURCE).toContain('const shouldQueryGithub =')
    expect(CONTROLLER_SOURCE).toContain('foundation.repoBackedSearchTargets.length > 0')
    expect(GITHUB_SOURCE).toContain('fetchWorkItemsAcrossRepos')
    expect(GITHUB_SOURCE).toContain('repoBackedSearchTargets.map')
    expect(SECONDARY_SEARCH_SOURCE).toContain('const branchSearchRequest = useMemo')
  })

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
