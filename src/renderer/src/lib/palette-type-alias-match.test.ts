import { describe, expect, it } from 'vitest'
import { selectPaletteTypeAliasMatch } from './palette-type-alias-match'

const ALIASES = ['workspace board tab', 'workspace board', 'project board', 'board'] as const

describe('selectPaletteTypeAliasMatch', () => {
  it('prefers the alias with the earliest match, not declaration order', () => {
    expect(selectPaletteTypeAliasMatch(ALIASES, 'board')).toEqual({
      text: 'board',
      range: { start: 0, end: 5 }
    })
  })

  it('keeps the first alias when several match at the same offset', () => {
    expect(selectPaletteTypeAliasMatch(ALIASES, 'workspace')).toEqual({
      text: 'workspace board tab',
      range: { start: 0, end: 9 }
    })
  })

  it('still reports a mid-string hit when no alias starts with the query', () => {
    expect(selectPaletteTypeAliasMatch(ALIASES, 'tab')).toEqual({
      text: 'workspace board tab',
      range: { start: 16, end: 19 }
    })
  })

  it('returns null for an empty query or no match', () => {
    expect(selectPaletteTypeAliasMatch(ALIASES, '')).toBeNull()
    expect(selectPaletteTypeAliasMatch(ALIASES, 'terminal')).toBeNull()
    expect(selectPaletteTypeAliasMatch([], 'board')).toBeNull()
  })
})
