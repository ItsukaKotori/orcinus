export type FeatureInteractionId =
  | 'cmd-j'
  | 'cmd-j-workspace-open'
  | 'cmd-j-settings-open'
  | 'cmd-j-quick-action'
  | 'cmd-j-create-workspace'
  | 'automations'
  | 'automation-created'
  | 'automation-run'
  | 'markdown-file-created'
  | 'workspace-creation'
  | 'agent-orchestration-setup'
  | 'ai-commit-generation'
  | 'claude-account-switching'
  | 'computer-use-setup'
  | 'computer-use'
  | 'codex-account-switching'
  | 'floating-workspace'
  | 'floating-workspace-hidden'
  | 'notifications'

  | 'quick-commands'
  | 'resource-manager'
  | 'review-notes'
  | 'ssh'
  | 'terminal-pane-split'
  | 'terminal-panes'
  | 'terminal-tabs'
  | 'tab-splits'
  | 'usage-tracking'
  | 'workspace-cleanup'

export type FeatureInteractionDefinition = {
  id: FeatureInteractionId
  /** The product action that counts as "the user has interacted with this feature." */
  interaction: string
}

// Why: these ids become persisted product state; changing them breaks
// feature-discovery interaction tracking.
export const FEATURE_INTERACTIONS = [
  { id: 'cmd-j', interaction: 'Cmd+J palette opened' },
  { id: 'cmd-j-workspace-open', interaction: 'workspace opened from Cmd+J' },
  { id: 'cmd-j-settings-open', interaction: 'settings opened from Cmd+J' },
  { id: 'cmd-j-quick-action', interaction: 'quick action run from Cmd+J' },
  { id: 'cmd-j-create-workspace', interaction: 'workspace creation started from Cmd+J' },
  { id: 'automations', interaction: 'Automations page opened' },
  { id: 'automation-created', interaction: 'automation created' },
  { id: 'automation-run', interaction: 'automation run queued' },
  { id: 'markdown-file-created', interaction: 'untitled markdown file explicitly created' },
  { id: 'workspace-creation', interaction: 'workspace creation flow opened' },
  {
    id: 'agent-orchestration-setup',
    interaction: 'Agent Orchestration setup enabled or opened'
  },
  {
    id: 'ai-commit-generation',
    interaction: 'AI commit message generation enabled or used'
  },
  {
    id: 'claude-account-switching',
    interaction: 'Claude managed account added, selected, reauthenticated, or removed'
  },
  {
    id: 'computer-use-setup',
    interaction: 'Computer Use setup or permission flow opened'
  },
  { id: 'computer-use', interaction: 'computer-use runtime method used' },
  {
    id: 'codex-account-switching',
    interaction: 'Codex managed account added, selected, reauthenticated, or removed'
  },
  { id: 'floating-workspace', interaction: 'Floating Workspace opened or configured' },
  {
    id: 'floating-workspace-hidden',
    interaction: 'Floating Workspace explicitly hidden or disabled'
  },
  { id: 'notifications', interaction: 'desktop notifications enabled or tested' },
  { id: 'quick-commands', interaction: 'terminal quick command created or edited' },
  { id: 'resource-manager', interaction: 'Resource Manager opened or configured' },
  { id: 'review-notes', interaction: 'review note added or sent to an agent' },
  {
    id: 'ssh',
    interaction: 'SSH target added, imported, tested, connected, disconnected, or configured'
  },
  {
    id: 'terminal-pane-split',
    interaction: 'terminal pane split from the split-pane command'
  },
  {
    id: 'terminal-panes',
    interaction: 'terminal/editor/browser pane created, resized, or merged'
  },
  {
    id: 'terminal-tabs',
    interaction: 'workspace tab created, moved, reordered, pinned, renamed, recolored, or closed'
  },
  { id: 'tab-splits', interaction: 'workspace tab split into another pane' },
  {
    id: 'usage-tracking',
    interaction: 'Stats & Usage or provider usage details opened or configured'
  },
  {
    id: 'workspace-cleanup',
    interaction: 'workspace disk space scan, review, or cleanup action used'
  }
] as const satisfies readonly FeatureInteractionDefinition[]

export const FEATURE_INTERACTION_IDS = FEATURE_INTERACTIONS.map((feature) => feature.id) as [
  FeatureInteractionId,
  ...FeatureInteractionId[]
]
