import { describe, expect, it } from 'vitest'
import { resolveSourceControlCompareBaseRef } from './base-ref-resolution'

// 用户偏好：对比只跟自己分支的 upstream（自己的远程分支），仓库配置的 base 退居其后。
describe('resolveSourceControlCompareBaseRef', () => {
  it('有 upstream 时优先用 upstream，胜过仓库/工作区 pinned base', () => {
    expect(
      resolveSourceControlCompareBaseRef({
        enabled: true,
        worktreeBaseRef: 'origin/main',
        repoBaseRef: 'origin/main',
        upstreamName: 'origin/feature',
      })
    ).toBe('origin/feature')
  })

  it('无 upstream 时回落到 pinned（worktree 先于 repo）', () => {
    expect(
      resolveSourceControlCompareBaseRef({
        enabled: true,
        worktreeBaseRef: ' origin/dev ',
        upstreamName: null,
      })
    ).toBe('origin/dev')
    expect(
      resolveSourceControlCompareBaseRef({
        enabled: true,
        repoBaseRef: 'origin/main',
        upstreamName: undefined,
      })
    ).toBe('origin/main')
  })

  it('pinned 也空时用 fallback', () => {
    expect(
      resolveSourceControlCompareBaseRef({ enabled: true, fallbackBaseRef: 'origin/main' })
    ).toBe('origin/main')
  })

  it('disabled 时只给 fallback（不看 upstream/pinned）', () => {
    expect(
      resolveSourceControlCompareBaseRef({
        enabled: false,
        upstreamName: 'origin/feature',
        fallbackBaseRef: 'origin/main',
      })
    ).toBe('origin/main')
  })

  it('全空 → null', () => {
    expect(resolveSourceControlCompareBaseRef({ enabled: true })).toBeNull()
  })
})
