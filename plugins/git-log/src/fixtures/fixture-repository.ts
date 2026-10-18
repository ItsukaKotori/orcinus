import type { GitBranchChangeEntry } from '@shared/git-diff-compare-types'
import type { GitLogRepository, GitLogSnapshot } from '../domain/types'
import { buildFixtureSnapshot } from './fixture-snapshot'

const FILES: GitBranchChangeEntry[] = [
  { path: 'src/core/engine.ts', status: 'modified', added: 12, removed: 3 },
  { path: 'src/core/types.ts', status: 'added', added: 40, removed: 0 },
  { path: 'docs/old.md', status: 'deleted', added: 0, removed: 27 }
]

export class FixtureRepository implements GitLogRepository {
  async loadSnapshot(): Promise<GitLogSnapshot> {
    return buildFixtureSnapshot()
  }
  async loadCommitFiles(_oid: string): Promise<GitBranchChangeEntry[]> {
    return FILES
  }
}
