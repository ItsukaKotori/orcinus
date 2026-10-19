import { z } from 'zod'
import type { GitHistoryItemRef, GitHistoryResult } from '@shared/git-history-types'
import type { GitBranchChangeEntry } from '@shared/git-diff-compare-types'
import type { GitLogFilter } from './query'

export type GitLogSnapshot = GitHistoryResult

export interface GitLogRepository {
  /** Loads a page of history for `filter`, starting at commit offset `skip`.
   *  Both args optional so a first load (`loadSnapshot()`) yields the default
   *  HEAD page (increment 2a behaviour). */
  loadSnapshot(filter?: GitLogFilter, skip?: number): Promise<GitLogSnapshot>
  /** Lists branch / remote / tag refs for the branch selector (host `git.refs`). */
  loadRefs(): Promise<GitHistoryItemRef[]>
  loadCommitFiles(oid: string): Promise<GitBranchChangeEntry[]>
}

/** 增量 2 数据缝占位：经 worker.invokeCommand/session.post 构造。
 *  本增量不实现、不实例化、不声明 worker:call capability。 */
export interface ChannelRepository extends GitLogRepository {
  readonly __reservedForIncrement2: true
}

// Runtime shape for the host `git.log`/`git.commitFiles` results, mirroring the
// Rust `ade-git` serde (camelCase) field-for-field. Guards the TS↔Rust contract at
// runtime instead of trusting an `as` cast; `host-contract.test.ts` locks this schema
// against the fixture (which shares the same shape).
const refSchema = z.object({
  id: z.string(),
  name: z.string(),
  revision: z.string().optional(),
  category: z.string().optional(),
  description: z.string().optional(),
  color: z.string().optional()
})
const statisticsSchema = z.object({
  files: z.number(),
  insertions: z.number(),
  deletions: z.number()
})
const itemSchema = z.object({
  id: z.string(),
  parentIds: z.array(z.string()),
  subject: z.string(),
  message: z.string(),
  displayId: z.string().optional(),
  author: z.string().optional(),
  authorEmail: z.string().optional(),
  timestamp: z.number().optional(),
  statistics: statisticsSchema.optional(),
  references: z.array(refSchema).optional()
})
export const gitHistoryResultSchema = z.object({
  items: z.array(itemSchema),
  currentRef: refSchema.optional(),
  remoteRef: refSchema.optional(),
  baseRef: refSchema.optional(),
  mergeBase: z.string().optional(),
  hasIncomingChanges: z.boolean(),
  hasOutgoingChanges: z.boolean(),
  hasMore: z.boolean(),
  limit: z.number()
})
export const gitCommitFilesSchema = z.array(
  z.object({
    path: z.string(),
    status: z.string(),
    oldPath: z.string().optional(),
    added: z.number().optional(),
    removed: z.number().optional()
  })
)
/** Host `git.refs` result: the same ref shape the log decorations use. */
export const gitRefsSchema = z.array(refSchema)
