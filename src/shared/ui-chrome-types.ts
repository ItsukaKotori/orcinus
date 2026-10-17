export type TaskViewPresetId = 'all' | 'issues' | 'review' | 'my-issues' | 'my-prs' | 'prs'

export type OpenInApplication = {
  id: string
  label: string
  command: string
}

export type SourceControlViewMode = 'list' | 'tree'
export type SourceControlGroupOrder = 'changes-first' | 'staged-first' | 'untracked-first'

export type LeftSidebarAppearanceMode = 'default' | 'match-terminal' | 'tinted'

/** Strategy for the prefix prepended to worktree branch names. */
export type BranchPrefixStrategy = 'git-username' | 'custom' | 'none'

export type FloatingTerminalCwdRequest = {
  path?: string
  requireTrusted?: boolean
}

export type WorktreeCardProperty =
  | 'status'
  | 'unread'
  // Legacy persisted preference. CI status is now represented by linked PR metadata.
  | 'ci'
  // Migration-only: legacy detailed cards showed branch identity as a visible row.
  | 'branch'
  // Task metadata on workspace cards; provider-specific persisted values kept for older profiles.
  | 'issue'
  | 'linear-issue'
  | 'jira-issue'
  | 'pr'
  | 'automation'
  // Badge marking workspaces created through `orca worktree create`.
  | 'cli'
  | 'comment'
  // Inline agent-activity list rendered in each workspace card; on by default (see DEFAULT_WORKTREE_CARD_PROPERTIES in shared/constants.ts).
  | 'inline-agents'

export type WorktreeCardMode = 'Default' | 'Compact'

export type AgentActivityDisplayMode = 'compact' | 'full'

export type StatusBarItem =
  | 'claude'
  | 'codex'
  | 'gemini'
  | 'antigravity'
  | 'opencode-go'
  | 'kimi'
  | 'minimax'
  | 'grok'
  | 'ssh'
  | 'resource-usage'
export type FloatingTerminalTriggerLocation = 'floating-button' | 'status-bar'

export type RightSidebarTab =
  | 'explorer'
  | 'search'
  | 'vault'
  | 'workspaces'
  | 'source-control'
  // Plugin-contributed panels are keyed `plugin:<pluginId>/<panelId>` so the
  // static union stays closed while plugin tabs remain type-representable.
  | `plugin:${string}`
export type ActiveRightSidebarTab = Exclude<RightSidebarTab, 'search'>
export type RightSidebarExplorerView = 'files' | 'search'

export type ProjectOrderBy = 'manual' | 'recent'
export type WorkspaceHostScope = 'all' | 'local' | `ssh:${string}` | `runtime:${string}`
export type VisibleWorkspaceHostIds = Exclude<WorkspaceHostScope, 'all'>[] | null
export type WorkspaceHostOrder = Exclude<WorkspaceHostScope, 'all'>[]
export type ManualRepoOrderEntry = {
  hostId: WorkspaceHostOrder[number]
  repoId: string
}

/** Top-level views with a fixed key; the exhaustive lookup in top-level-view.ts polices this set. */
export type StaticTopLevelView =
  | 'terminal'
  | 'settings'
  | 'space'
  | 'artifacts'
  | 'plugin-center'

/** The active top-level section shown in the main content area; plugin-hosted views are keyed `plugin:<pluginKey>`. */
export type TopLevelView = StaticTopLevelView | `plugin:${string}`
