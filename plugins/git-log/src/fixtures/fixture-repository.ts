import type { GitHistoryItemRef } from '@shared/git-history-types'
import type { GitBranchChangeEntry } from '@shared/git-diff-compare-types'
import type { GitLogRepository, GitLogSnapshot } from '../domain/types'
import type { GitLogFilter } from '../domain/query'
import { buildFixtureSnapshot } from './fixture-snapshot'

const FILES: GitBranchChangeEntry[] = [
  { path: 'src/core/engine.ts', status: 'modified', added: 12, removed: 3 },
  { path: 'src/core/types.ts', status: 'added', added: 40, removed: 0 },
  { path: 'docs/old.md', status: 'deleted', added: 0, removed: 27 }
]

export class FixtureRepository implements GitLogRepository {
  // Fixture is a single, static page: any offset beyond the first yields nothing,
  // so infinite-scroll "load more" terminates in the demo too.
  async loadSnapshot(_filter: GitLogFilter = {}, skip = 0): Promise<GitLogSnapshot> {
    const snap = buildFixtureSnapshot()
    return skip > 0 ? { ...snap, items: [], hasMore: false } : snap
  }

  async loadRefs(): Promise<GitHistoryItemRef[]> {
    const snap = buildFixtureSnapshot()
    const seen = new Map<string, GitHistoryItemRef>()
    for (const ref of [snap.currentRef, snap.remoteRef, snap.baseRef]) {
      if (ref) seen.set(ref.id, ref)
    }
    for (const item of snap.items) {
      for (const ref of item.references ?? []) seen.set(ref.id, ref)
    }
    return [...seen.values()]
  }

  async loadCommitFiles(_oid: string): Promise<GitBranchChangeEntry[]> {
    return FILES
  }
}
