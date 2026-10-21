import { useMemo } from 'react'
import { useAppStore } from '@/store'
import { findWorktreeById } from '@/store/slices/worktree-helpers'
import type { Tab } from '../../../../shared/tab-types'
import { GitLogView } from './GitLogView'
import { TauriGitLogRepository } from './tauri-repository'

/** Hosts the native Git Log view in a main-area tab; resolves the tab's worktree
 *  to a filesystem path and memoizes the repository so re-renders don't refetch. */
export function GitLogTabContent({ tab }: { tab: Tab }): React.JSX.Element {
  const worktreeId = tab.gitLog?.worktreeId ?? tab.worktreeId
  const worktreePath = useAppStore(
    (s) => findWorktreeById(s.worktreesByRepo, worktreeId)?.path ?? null,
  )
  const repository = useMemo(
    () => (worktreePath ? new TauriGitLogRepository(worktreePath) : null),
    [worktreePath],
  )
  if (!repository) {
    return (
      <div className="flex h-full items-center justify-center text-sm text-muted-foreground">
        Git Log: workspace unavailable
      </div>
    )
  }
  return <GitLogView repository={repository} />
}

export default GitLogTabContent
