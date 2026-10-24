import type { GitRefEntry } from '../../../../shared/git-ref-types'
import type { GitHistoryResult } from '../../../../shared/git-history-types'
import type { GitBranchChangeEntry } from '../../../../shared/git-diff-compare-types'
import type { GitLogFilter } from './query'

export type GitLogSnapshot = GitHistoryResult
export type GitRefs = { refs: GitRefEntry[]; truncated: boolean }

/** Data source for the native Git Log view (implemented over tauri commands). */
export interface GitLogRepository {
  loadSnapshot(filter?: GitLogFilter, skip?: number): Promise<GitLogSnapshot>
  loadRefs(): Promise<GitRefs>
  loadCommitFiles(oid: string): Promise<GitBranchChangeEntry[]>
}
