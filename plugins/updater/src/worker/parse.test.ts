import { describe, expect, it } from 'vitest'
import { parseBrewOutdated, parseNpmOutdated } from './parse'

const NPM_OUTDATED = JSON.stringify({
  typescript: { current: '5.8.3', wanted: '5.8.3', latest: '5.9.2', location: 'global' },
  eslint: { current: '9.18.0', wanted: '9.21.0', latest: '9.21.0', location: 'global' }
})

const BREW_OUTDATED = JSON.stringify({
  formulae: [
    { name: 'node', installed_versions: ['22.13.0'], current_version: '22.21.0' },
    { name: 'git', installed_versions: ['2.47.1'], current_version: '2.49.0' }
  ],
  casks: [{ name: 'docker-desktop', installed_versions: ['4.38.0'], current_version: '4.40.0' }]
})

describe('parseNpmOutdated', () => {
  it('maps the npm json into sorted entries', () => {
    expect(parseNpmOutdated(NPM_OUTDATED)).toEqual([
      { manager: 'npm', kind: 'global', name: 'eslint', current: '9.18.0', latest: '9.21.0' },
      { manager: 'npm', kind: 'global', name: 'typescript', current: '5.8.3', latest: '5.9.2' }
    ])
  })

  it('treats empty stdout and {} as no updates', () => {
    expect(parseNpmOutdated('')).toEqual([])
    expect(parseNpmOutdated('{}')).toEqual([])
  })

  it('throws on malformed json (caller surfaces the raw failure)', () => {
    expect(() => parseNpmOutdated('npm ERR! network')).toThrowError()
  })
})

describe('parseBrewOutdated', () => {
  it('maps formulae and casks', () => {
    expect(parseBrewOutdated(BREW_OUTDATED)).toEqual([
      { manager: 'brew', kind: 'cask', name: 'docker-desktop', current: '4.38.0', latest: '4.40.0' },
      { manager: 'brew', kind: 'formula', name: 'git', current: '2.47.1', latest: '2.49.0' },
      { manager: 'brew', kind: 'formula', name: 'node', current: '22.13.0', latest: '22.21.0' }
    ])
  })

  it('handles missing arrays, empty stdout, and skips nameless items', () => {
    expect(parseBrewOutdated('')).toEqual([])
    expect(parseBrewOutdated('{}')).toEqual([])
    expect(parseBrewOutdated(JSON.stringify({ formulae: [{ current_version: '1.0.0' }] }))).toEqual([])
  })

  it('throws on malformed json', () => {
    expect(() => parseBrewOutdated('Warning: something')).toThrowError()
  })
})
