import type { GitBranchChangeEntry } from '../../../../shared/git-diff-compare-types'
import type { GitLogQuery } from '../../../../shared/preload-api/api/git-inspection-api'
import type { GitLogRepository, GitLogSnapshot, GitRefs } from './types'
import { logParams, type GitLogFilter } from './query'

/** Native Git Log data source: calls the `git_log_query` / `git_refs` /
 *  `git_commit_compare` tauri commands (results are typed by the bindings, so
 *  no runtime zod validation is needed here). */
export class TauriGitLogRepository implements GitLogRepository {
  constructor(private readonly worktreePath: string) {}

  async loadSnapshot(filter: GitLogFilter = {}, skip = 0): Promise<GitLogSnapshot> {
    return window.api.git.logQuery({
      worktreePath: this.worktreePath,
      query: logParams(filter, skip) as GitLogQuery
    })
  }

  async loadRefs(): Promise<GitRefs> {
    return window.api.git.refs({ worktreePath: this.worktreePath })
  }

  async loadCommitFiles(oid: string): Promise<GitBranchChangeEntry[]> {
    const result = await window.api.git.commitCompare({
      worktreePath: this.worktreePath,
      commitId: oid
    })
    return result.entries
  }
}
