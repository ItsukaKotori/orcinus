import type { GitHistoryResult } from '@shared/git-history-types'
import type { GitBranchChangeEntry } from '@shared/git-diff-compare-types'
import { callHost } from '../ui/host-bridge'
import { gitCommitFilesSchema, gitHistoryResultSchema } from './types'
import type { GitLogRepository, GitLogSnapshot } from './types'

const SNAPSHOT_LIMIT = 200

/** Real-data repository: reads the active worktree via host `git.log` /
 *  `git.commitFiles` (panel bridge). Results are validated at runtime against
 *  schemas mirroring the host serde (camelCase) shape, not blind casts. */
export class HostRepository implements GitLogRepository {
  async loadSnapshot(): Promise<GitLogSnapshot> {
    return gitHistoryResultSchema.parse(await callHost('git.log', { limit: SNAPSHOT_LIMIT })) as GitHistoryResult
  }

  async loadCommitFiles(oid: string): Promise<GitBranchChangeEntry[]> {
    return gitCommitFilesSchema.parse(await callHost('git.commitFiles', { oid })) as GitBranchChangeEntry[]
  }
}
