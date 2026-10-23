import { describe, expect, it } from 'vitest'
import { normalizeRightSidebarRoute } from './right-sidebar-route'

describe('normalizeRightSidebarRoute', () => {
  it('still normalizes invalid tabs to Explorer files', () => {
    expect(normalizeRightSidebarRoute('missing')).toEqual({
      rightSidebarTab: 'explorer',
      rightSidebarExplorerView: 'files'
    })
  })

  it('preserves the built-in activity tabs', () => {
    expect(normalizeRightSidebarRoute('vault')).toEqual({
      rightSidebarTab: 'vault',
      rightSidebarExplorerView: 'files'
    })
    expect(normalizeRightSidebarRoute('source-control')).toEqual({
      rightSidebarTab: 'source-control',
      rightSidebarExplorerView: 'files'
    })
  })

  it('keeps the explorer sub-view (files/search) and defaults unknown views to files', () => {
    expect(normalizeRightSidebarRoute('explorer', 'search')).toEqual({
      rightSidebarTab: 'explorer',
      rightSidebarExplorerView: 'search'
    })
    expect(normalizeRightSidebarRoute('explorer', 'bogus')).toEqual({
      rightSidebarTab: 'explorer',
      rightSidebarExplorerView: 'files'
    })
  })
})
