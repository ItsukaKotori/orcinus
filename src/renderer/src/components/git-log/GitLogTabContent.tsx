import { useMemo } from 'react'
import { useAppStore } from '@/store'
import { translate } from '@/i18n/i18n'
import { findWorktreeById } from '@/store/slices/worktree-helpers'
import { detectLanguage } from '@/lib/language-detect'
import type { GitBranchChangeEntry } from '../../../../shared/git-diff-compare-types'
import type { GitHistoryItem } from '../../../../shared/git-history-types'
import type { Tab } from '../../../../shared/tab-types'
import { GitLogView } from './GitLogView'
import { TauriGitLogRepository } from './tauri-repository'

/** Hosts the native Git Log view in a main-area tab; resolves the tab's worktree
 *  to a filesystem path, memoizes the repository, and wires commit-file clicks to
 *  the app's existing diff editor (`openCommitDiff`). */
export function GitLogTabContent({ tab }: { tab: Tab }): React.JSX.Element {
  const worktreeId = tab.gitLog?.worktreeId ?? tab.worktreeId
  const worktreePath = useAppStore(
    (s) => findWorktreeById(s.worktreesByRepo, worktreeId)?.path ?? null,
  )
  const openCommitDiff = useAppStore((s) => s.openCommitDiff)

  const repository = useMemo(
    () => (worktreePath ? new TauriGitLogRepository(worktreePath) : null),
    [worktreePath],
  )

  const onOpenFileDiff = useMemo(() => {
    if (!worktreePath) return undefined
    return async (item: GitHistoryItem, entry: GitBranchChangeEntry): Promise<void> => {
      const cmp = await window.api.git.commitCompare({ worktreePath, commitId: item.id })
      if (cmp.summary.status !== 'ready') return
      openCommitDiff(
        worktreeId,
        worktreePath,
        entry,
        {
          commitOid: cmp.summary.commitOid,
          parentOid: cmp.summary.parentOid,
          compareRef: cmp.summary.compareRef,
          baseRef: cmp.summary.baseRef,
          subject: item.subject,
          message: item.message,
        },
        detectLanguage(entry.path),
        { targetGroupId: tab.groupId, preview: false },
      )
    }
  }, [openCommitDiff, worktreePath, worktreeId, tab.groupId])

  if (!repository) {
    return (
      <div className="flex h-full items-center justify-center text-sm text-muted-foreground">
        {translate('auto.components.git-log.workspaceUnavailable', 'Git Log: workspace unavailable')}
      </div>
    )
  }
  return <GitLogView repository={repository} onOpenFileDiff={onOpenFileDiff} />
}

export default GitLogTabContent
