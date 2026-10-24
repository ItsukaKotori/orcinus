import { useMemo } from 'react'
import { Files, GitBranch, Workflow } from 'lucide-react'
import { useAppStore } from '@/store'
import { useRepoById } from '@/store/selectors'
import { isFolderRepo } from '../../../../shared/repo-kind'
import { parseWorkspaceKey } from '../../../../shared/workspace-scope'
import { getVisibleRightSidebarActivityItems } from './right-sidebar-activity-visibility'
import { useShortcutLabel } from '@/hooks/useShortcutLabel'
import { translate } from '@/i18n/i18n'
import { AgentSessionHistoryIcon } from './agent-session-history-icon'
import type { ActivityBarItem } from './activity-bar-buttons'

export type RightSidebarActivityItems = {
  visibleItems: ActivityBarItem[]
  activeFolderWorkspaceKey: string | null
}

export function useRightSidebarActivityItems({
  rightSidebarOpen
}: {
  rightSidebarOpen: boolean
}): RightSidebarActivityItems {
  const explorerShortcut = useShortcutLabel('sidebar.explorer.toggle')
  const sourceControlShortcut = useShortcutLabel('sidebar.sourceControl.toggle')
  const activeWorktreeId = useAppStore((s) => (rightSidebarOpen ? s.activeWorktreeId : null))
  // Why: source control is meaningless for non-git folders.
  // Hide that tab so the activity bar only shows relevant actions.
  const activeWorktree = useAppStore((s) =>
    activeWorktreeId ? (s.getKnownWorktreeById(activeWorktreeId) ?? null) : null
  )
  const activeRepo = useRepoById(activeWorktree?.repoId ?? null)
  const activeWorkspaceScope = parseWorkspaceKey(activeWorktreeId ?? '')
  const isFolderWorkspace = activeWorkspaceScope?.type === 'folder'
  const isFolder = isFolderWorkspace || (activeRepo ? isFolderRepo(activeRepo) : false)
  const isSshRepo = Boolean(activeRepo?.connectionId)

  const activityItems = useMemo<ActivityBarItem[]>(
    () => [
      {
        id: 'explorer',
        icon: Files,
        title: translate('auto.components.right.sidebar.index.8bc2bbc3a0', 'Explorer'),
        shortcut: explorerShortcut === 'Unassigned' ? '' : explorerShortcut
      },
      {
        id: 'vault',
        icon: AgentSessionHistoryIcon,
        title: translate('auto.components.right.sidebar.index.aiVaultSessionHistory', 'Agents'),
        shortcut: ''
      },
      {
        id: 'workspaces',
        icon: Workflow,
        title: translate(
          'auto.components.right.sidebar.index.folderWorkspaces',
          'Attached worktrees'
        ),
        shortcut: '',
        folderOnly: true
      },
      {
        id: 'source-control',
        icon: GitBranch,
        title: translate('auto.components.right.sidebar.index.0314901467', 'Source Control'),
        shortcut: sourceControlShortcut === 'Unassigned' ? '' : sourceControlShortcut,
        gitOnly: true
      }
    ],
    [explorerShortcut, sourceControlShortcut]
  )

  const visibleItems = useMemo(
    () =>
      getVisibleRightSidebarActivityItems(activityItems, {
        isFolder,
        isFolderWorkspace,
        isSshRepo
      }),
    [activityItems, isFolder, isFolderWorkspace, isSshRepo]
  )

  const activeFolderWorkspaceKey = isFolderWorkspace ? (activeWorktreeId ?? null) : null

  return {
    visibleItems,
    activeFolderWorkspaceKey
  }
}
