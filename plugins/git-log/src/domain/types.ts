import type { GitHistoryResult } from '@shared/git-history-types'
import type { GitBranchChangeEntry } from '@shared/git-diff-compare-types'

export type GitLogSnapshot = GitHistoryResult

export interface GitLogRepository {
  loadSnapshot(): Promise<GitLogSnapshot>
  loadCommitFiles(oid: string): Promise<GitBranchChangeEntry[]>
}

/** 增量 2 数据缝占位：经 worker.invokeCommand/session.post 构造。
 *  本增量不实现、不实例化、不声明 worker:call capability。 */
export interface ChannelRepository extends GitLogRepository {
  readonly __reservedForIncrement2: true
}
