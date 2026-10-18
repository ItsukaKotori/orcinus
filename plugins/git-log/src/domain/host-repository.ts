import type { GitHistoryResult } from '@shared/git-history-types'
import type { GitBranchChangeEntry } from '@shared/git-diff-compare-types'
import { callHost } from '../ui/host-bridge'
import type { GitLogRepository, GitLogSnapshot } from './types'

const SNAPSHOT_LIMIT = 200

/** Real-data repository: reads the active worktree via host `git.log` /
 *  `git.commitFiles` (panel bridge). Result JSON is camelCase by host contract
 *  and matches `GitHistoryResult` / `GitBranchChangeEntry` field-for-field. */
export class HostRepository implements GitLogRepository {
  async loadSnapshot(): Promise<GitLogSnapshot> {
    return (await callHost('git.log', { limit: SNAPSHOT_LIMIT })) as GitHistoryResult
  }

  async loadCommitFiles(oid: string): Promise<GitBranchChangeEntry[]> {
    return (await callHost('git.commitFiles', { oid })) as GitBranchChangeEntry[]
  }
}
