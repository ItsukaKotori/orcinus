import type { GitHistoryResult } from '@shared/git-history-types'
import type { GitBranchChangeEntry } from '@shared/git-diff-compare-types'
import { callHost } from '../ui/host-bridge'
import { gitCommitFilesSchema, gitHistoryResultSchema, gitRefsSchema } from './types'
import type { GitLogRepository, GitLogSnapshot, GitRefs } from './types'
import { logParams, type GitLogFilter } from './query'

/** Real-data repository: reads the active worktree via host `git.log` /
 *  `git.refs` / `git.commitFiles` (panel bridge). Results are validated at runtime
 *  against schemas mirroring the host serde (camelCase) shape, not blind casts.
 *  Filtering, range (`refs`) and paging (`skip`) are executed by the backend. */
export class HostRepository implements GitLogRepository {
  async loadSnapshot(filter: GitLogFilter = {}, skip = 0): Promise<GitLogSnapshot> {
    const value = await callHost('git.log', logParams(filter, skip))
    return gitHistoryResultSchema.parse(value) as GitHistoryResult
  }

  async loadRefs(): Promise<GitRefs> {
    return gitRefsSchema.parse(await callHost('git.refs', {})) as GitRefs
  }

  async loadCommitFiles(oid: string): Promise<GitBranchChangeEntry[]> {
    return gitCommitFilesSchema.parse(await callHost('git.commitFiles', { oid })) as GitBranchChangeEntry[]
  }
}
