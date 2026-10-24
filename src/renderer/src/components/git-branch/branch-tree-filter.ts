import type { GitRefEntry } from '../../../../shared/git-ref-types'

export function normalizeBranchQuery(query: string): string {
  return query.trim().toLowerCase()
}

/**
 * 两条规则，缺一不可：
 *  1. 子串 —— `eat` 命中 `feature/login`（IDEA 的宽松匹配）。
 *  2. 段前缀 —— `f/log` 命中 `feature/login`。子串规则做不到（`f/log` 不是连续子串），
 *     而这是长层级分支名（`team/feature/foo`）的主要检索方式。
 *
 * 刻意不做 fuzzy / camelHump：误报率高于收益，且与 IDEA 的实际行为不必强行一致。
 */
export function matchesBranchQuery(ref: GitRefEntry, query: string): boolean {
  const needle = normalizeBranchQuery(query)
  if (!needle) return true
  const name = ref.name.toLowerCase()
  if (name.includes(needle)) return true
  return segmentPrefixMatch(name, needle.split('/').filter(Boolean))
}

/**
 * 段前缀只在**尾部**对齐：查询的段数必须 ≤ 条目段数，且逐段 startsWith。
 * 不做全串匹配，否则 `feat/login` 会命中 `feature/login/x`（错位段），
 * 那是比「搜不到」更糟的误报。
 */
function segmentPrefixMatch(name: string, needleSegments: readonly string[]): boolean {
  if (needleSegments.length === 0) return false
  const parts = name.split('/')
  if (parts.length < needleSegments.length) return false
  const offset = parts.length - needleSegments.length
  return needleSegments.every((seg, i) => parts[offset + i]!.startsWith(seg))
}
