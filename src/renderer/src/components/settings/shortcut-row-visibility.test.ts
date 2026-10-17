import { describe, expect, it } from 'vitest'
import { getKeybindingDefinition } from '../../../../shared/keybindings'
import type { ShortcutGroup } from './shortcut-groups'
import { buildShortcutRowVisibility } from './shortcut-row-visibility'

function creationGroup(): ShortcutGroup {
  const items = (['tab.newTerminal', 'tab.newMarkdown'] as const).map((actionId) => {
    const definition = getKeybindingDefinition(actionId)
    if (!definition) {
      throw new Error(`Missing keybinding definition: ${actionId}`)
    }
    return definition
  })
  return { title: 'Tabs', items }
}

describe('buildShortcutRowVisibility', () => {
  it('keeps terminal and markdown creation shortcuts visible', () => {
    const result = buildShortcutRowVisibility({
      groups: [creationGroup()],
      keybindings: {},
      conflictByAction: new Map(),
      terminalShortcutPolicy: 'orca-first',
      platform: 'darwin',
      settingsSearchQuery: '',
      shortcutQuery: '',
      shortcutFilter: 'all'
    })

    expect(result.shortcutRows.map((row) => row.item.id)).toEqual([
      'tab.newTerminal',
      'tab.newMarkdown'
    ])
  })
})
