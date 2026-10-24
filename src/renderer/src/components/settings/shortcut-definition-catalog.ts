import {
  findKeybindingConflictsForDefinitions,
  formatKeybindingList,
  type KeybindingActionId,
  type KeybindingDefinition,
  type KeybindingOverrides
} from '../../../../shared/keybindings'
import {
  findMacSystemHotkeyConflicts,
  type MacCapturedDigitChord
} from '../../../../shared/macos-symbolic-hotkeys'
import type { TuiAgent } from '../../../../shared/tui-agent'
import { disabledAgentTabActionIds, groupDefinitions, type ShortcutGroup } from './shortcut-groups'

export type ShortcutDefinitionCatalog = {
  groups: ShortcutGroup[]
  definitions: KeybindingDefinition[]
  definitionsByAction: Map<KeybindingActionId, KeybindingDefinition>
  ignoredConflictActionIds: KeybindingActionId[]
  conflictByAction: Map<KeybindingActionId, string[]>
}

export function buildShortcutDefinitionCatalog(options: {
  disabledTuiAgents: readonly TuiAgent[]
  keybindings: KeybindingOverrides
  platform: NodeJS.Platform
  macCapturedDigitChords?: readonly MacCapturedDigitChord[]
  missionControlConflictMessage: string
}): ShortcutDefinitionCatalog {
  const groups = groupDefinitions(options.disabledTuiAgents)
  const definitions = groups.flatMap((group) => group.items)
  const definitionsByAction = new Map(definitions.map((definition) => [definition.id, definition]))
  const ignoredConflictActionIds = disabledAgentTabActionIds(options.disabledTuiAgents)
  const conflictByAction = new Map<KeybindingActionId, string[]>()
  const conflicts = findKeybindingConflictsForDefinitions(
    definitions,
    options.platform,
    options.keybindings,
    { ignoredActionIds: ignoredConflictActionIds }
  )
  for (const conflict of conflicts) {
    const labels = conflict.actionIds
      .map((id) => definitionsByAction.get(id)?.title ?? id)
      .join(', ')
    for (const actionId of conflict.actionIds) {
      conflictByAction.set(actionId, [
        ...(conflictByAction.get(actionId) ?? []),
        `${formatKeybindingList([conflict.binding], options.platform)} conflicts with ${labels}.`
      ])
    }
  }
  const systemConflicts = findMacSystemHotkeyConflicts(
    definitions,
    options.platform,
    options.keybindings,
    options.macCapturedDigitChords ?? []
  )
  for (const conflict of systemConflicts) {
    if (ignoredConflictActionIds.includes(conflict.actionId)) {
      continue
    }
    conflictByAction.set(conflict.actionId, [
      ...(conflictByAction.get(conflict.actionId) ?? []),
      options.missionControlConflictMessage
    ])
  }
  return {
    groups,
    definitions,
    definitionsByAction,
    ignoredConflictActionIds,
    conflictByAction
  }
}
