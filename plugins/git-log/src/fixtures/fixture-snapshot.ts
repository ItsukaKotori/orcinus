import type { GitHistoryItem, GitHistoryItemRef, GitHistoryResult } from '@shared/git-history-types'

const DAY = 86_400_000
const BASE_TS = Date.UTC(2026, 9, 18, 12, 0, 0) // 固定，确保确定性

function oid(label: string): string {
  let h = 0x811c9dc5
  const s = `git-log-fixture:${label}`
  for (let i = 0; i < s.length; i += 1) {
    h ^= s.charCodeAt(i)
    h = Math.imul(h, 0x01000193)
  }
  const seed = (h >>> 0).toString(16).padStart(8, '0')
  return (seed.repeat(5) + '0').slice(0, 40) // 稳定 40-hex
}

const MAIN_LEN = 24
const FEAT_LEN = 6

// 拓扑序（newest → oldest，parent 必在其后）：merge tip → main0..7 → feature 链 → main8..23
const ORDER: string[] = ['M']
for (let k = 0; k <= 7; k += 1) ORDER.push(`main${k}`)
for (let k = 0; k < FEAT_LEN; k += 1) ORDER.push(`feat${k}`)
for (let k = 8; k < MAIN_LEN; k += 1) ORDER.push(`main${k}`)

function parentsOf(key: string): string[] {
  if (key === 'M') return ['main0', 'feat0'] // 合并 feature 分支 tip(feat0)，整条 feature 链可达
  if (key.startsWith('feat')) {
    const k = Number(key.slice(4))
    return k === FEAT_LEN - 1 ? ['main8'] : [`feat${k + 1}`]
  }
  const k = Number(key.slice(4)) // main<k>
  return k === MAIN_LEN - 1 ? [] : [`main${k + 1}`]
}

const AUTHORS = ['Ada', 'Lin', 'Grace']
function authorOf(key: string): string {
  if (key === 'M') return 'Ada'
  if (key.startsWith('feat')) return 'Grace'
  return AUTHORS[Number(key.slice(4)) % 3]!
}
function ageOf(key: string): number {
  if (key === 'M') return 0
  return key.startsWith('feat') ? 4 + Number(key.slice(4)) : Number(key.slice(4)) * 2
}
function subjectOf(key: string): string {
  if (key === 'M') return 'Merge branch feature into main'
  return key.startsWith('feat') ? `experiment: feat probe ${key.slice(4)}` : `feat(core): main change ${key.slice(4)}`
}

export function buildFixtureSnapshot(): GitHistoryResult {
  const items: GitHistoryItem[] = ORDER.map((key) => {
    const author = authorOf(key)
    return {
      id: oid(key),
      parentIds: parentsOf(key).map(oid),
      subject: subjectOf(key),
      message: subjectOf(key),
      author,
      authorEmail: `${author.toLowerCase()}@example.com`,
      timestamp: BASE_TS - ageOf(key) * DAY,
      statistics: { files: 1 + (key.length % 5), insertions: 10 + ((key.length * 3) % 40), deletions: 3 + (key.length % 7) }
    }
  })

  const at = (key: string): GitHistoryItem => items[ORDER.indexOf(key)]!
  const addRef = (item: GitHistoryItem, ref: GitHistoryItemRef): void => {
    item.references = [...(item.references ?? []), ref]
  }
  const tip = at('M')
  const featTip = at('feat0')
  const remoteTarget = at('main10')
  const tagA = at('main15')
  const tagB = at('main20')

  addRef(tip, { id: 'refs/heads/main', name: 'main', revision: tip.id, category: 'branches' })
  addRef(featTip, { id: 'refs/heads/feature', name: 'feature', revision: featTip.id, category: 'branches' })
  addRef(remoteTarget, { id: 'refs/remotes/origin/main', name: 'origin/main', revision: remoteTarget.id, category: 'remote branches' })
  addRef(tagA, { id: 'refs/tags/v1.0.0', name: 'v1.0.0', revision: tagA.id, category: 'tags' })
  addRef(tagB, { id: 'refs/tags/v0.9.0', name: 'v0.9.0', revision: tagB.id, category: 'tags' })

  return {
    items,
    currentRef: { id: 'refs/heads/main', name: 'main', revision: tip.id, category: 'branches' },
    remoteRef: { id: 'refs/remotes/origin/main', name: 'origin/main', revision: remoteTarget.id, category: 'remote branches' },
    baseRef: undefined,
    mergeBase: remoteTarget.id,
    hasIncomingChanges: true,
    hasOutgoingChanges: true,
    hasMore: false,
    limit: items.length
  }
}
