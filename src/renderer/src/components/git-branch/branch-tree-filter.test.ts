import { describe, expect, it } from 'vitest'
import type { GitRefEntry } from '../../../../shared/git-ref-types'
import { matchesBranchQuery, normalizeBranchQuery } from './branch-tree-filter'

const entry = (name: string, over: Partial<GitRefEntry> = {}): GitRefEntry => ({
  name,
  fullName: `refs/heads/${name}`,
  category: 'local',
  oid: '0'.repeat(40),
  isHead: false,
  ...over,
})

describe('normalizeBranchQuery', () => {
  it('trim + 小写', () => {
    expect(normalizeBranchQuery('  FeAture/Login ')).toBe('feature/login')
    expect(normalizeBranchQuery('')).toBe('')
  })
})

describe('matchesBranchQuery', () => {
  it('空查询命中一切（含全空格）', () => {
    expect(matchesBranchQuery(entry('main'), '')).toBe(true)
    expect(matchesBranchQuery(entry('main'), '   ')).toBe(true)
  })

  it('子串命中：中段、尾段、大小写无关', () => {
    expect(matchesBranchQuery(entry('feature/login'), 'log')).toBe(true)
    expect(matchesBranchQuery(entry('feature/login'), 'LOGIN')).toBe(true)
    expect(matchesBranchQuery(entry('feature/login'), 'eat')).toBe(true)
  })

  it('远程条目可按裸分支名搜（origin/main 命中 "main"）', () => {
    expect(
      matchesBranchQuery(
        entry('origin/main', {
          category: 'remote',
          remote: 'origin',
          fullName: 'refs/remotes/origin/main',
        }),
        'main'
      )
    ).toBe(true)
  })

  it('段前缀命中：f/log → feature/login（子串规则做不到，是本函数的增量）', () => {
    expect(matchesBranchQuery(entry('feature/login'), 'f/log')).toBe(true)
    expect(matchesBranchQuery(entry('feature/login'), 'feat/log')).toBe(true)
    expect(matchesBranchQuery(entry('team/feature/login'), 't/f/l')).toBe(true)
  })

  it('每一段都必须真的命中：feat/i 不命中 feature/login（无段以 i 开头）', () => {
    expect(matchesBranchQuery(entry('feature/login'), 'feat/i')).toBe(false)
    expect(matchesBranchQuery(entry('feature/login'), 'x/login')).toBe(false)
  })

  it('段前缀必须对齐尾部段数：x/log 不命中 feature/login', () => {
    expect(matchesBranchQuery(entry('feature/login'), 'x/log')).toBe(false)
    expect(matchesBranchQuery(entry('feature/login'), 'f/x')).toBe(false)
  })

  it('段数多于条目段数不命中', () => {
    expect(matchesBranchQuery(entry('main'), 'a/b/c/main')).toBe(false)
  })

  it('段前缀只在尾部对齐，不匹配中间段：feat/login 不命中 feature/login/x', () => {
    // feature/login/x 的末两段是 [login, x]，与 [feat, login] 错位。
    expect(matchesBranchQuery(entry('feature/login/x'), 'feat/login')).toBe(false)
  })

  it('不命中时返回 false', () => {
    expect(matchesBranchQuery(entry('main'), 'zzz')).toBe(false)
  })

  it('空段（尾随斜杠）不炸', () => {
    expect(matchesBranchQuery(entry('feature/login'), 'feature/')).toBe(true)
    expect(matchesBranchQuery(entry('feature/login'), '/')).toBe(true)
  })
})
