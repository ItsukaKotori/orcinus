// @vitest-environment happy-dom
import { beforeEach, describe, expect, it } from 'vitest'
import {
  BRANCH_SECTION_VIEW_STATE_STORAGE_KEY,
  persistBranchSectionCollapsed,
  readPersistedBranchSectionCollapsed,
} from './source-control-branch-section-view-state'

describe('branch section view state（localStorage 折叠态）', () => {
  beforeEach(() => {
    window.localStorage.clear()
  })

  it('读不到 → 默认展开（false）', () => {
    expect(readPersistedBranchSectionCollapsed()).toBe(false)
  })

  it('persist collapsed=true 后读回 true', () => {
    persistBranchSectionCollapsed(true)
    expect(readPersistedBranchSectionCollapsed()).toBe(true)
  })

  it('persist collapsed=false 读回 false', () => {
    persistBranchSectionCollapsed(true)
    persistBranchSectionCollapsed(false)
    expect(readPersistedBranchSectionCollapsed()).toBe(false)
  })

  it('损坏 JSON → 回落 false，不抛', () => {
    window.localStorage.setItem(BRANCH_SECTION_VIEW_STATE_STORAGE_KEY, '{ not json')
    expect(readPersistedBranchSectionCollapsed()).toBe(false)
  })

  it('非对象（数组）载荷 → 回落 false', () => {
    window.localStorage.setItem(BRANCH_SECTION_VIEW_STATE_STORAGE_KEY, '[1,2]')
    expect(readPersistedBranchSectionCollapsed()).toBe(false)
  })
})
