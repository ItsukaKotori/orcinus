import { describe, expect, it } from 'vitest'
import { isTopLevelView } from './top-level-view'

describe('isTopLevelView', () => {
  it('accepts plugin-center and plugin-scoped views', () => {
    expect(isTopLevelView('plugin-center')).toBe(true)
    expect(isTopLevelView('plugin:database-manager.main')).toBe(true)
  })
  it('rejects removed views and unknown prefixes', () => {
    expect(isTopLevelView('skills')).toBe(false)
    expect(isTopLevelView('mobile')).toBe(false)
    expect(isTopLevelView('plugin:')).toBe(false)
    expect(isTopLevelView('plugin:x y')).toBe(false)
  })
})
