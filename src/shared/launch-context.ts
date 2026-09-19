import type { AgentKind } from './agent-kind'

// Launch context enums carried on pty spawn / worktree startup payloads.
// These were previously declared alongside telemetry event schemas; they are
// product contract types that the launch pipeline consumes (retry policy,
// tab seeding) independently of any analytics emission.

export const LAUNCH_SOURCE_VALUES = [
  'command_palette',
  'sidebar',
  'quick_command',
  'tab_bar_quick_launch',
  'task_page',
  'new_workspace_composer',
  'workspace_jump_palette',
  'shortcut',
  'onboarding',
  'diff_notes_send',
  'notes_send',
  'conflict_resolution',
  'source_control_recovery',
  'terminal_context_menu',
  'unknown'
] as const
export type LaunchSource = (typeof LAUNCH_SOURCE_VALUES)[number]

export const REQUEST_KIND_VALUES = ['new', 'resume', 'followup'] as const
export type RequestKind = (typeof REQUEST_KIND_VALUES)[number]

/** Launch context stamped onto agent-startup payloads (tab seeding, retry policy). */
export type AgentLaunchContext = {
  agent_kind: AgentKind
  launch_source: LaunchSource
  request_kind: RequestKind
}
