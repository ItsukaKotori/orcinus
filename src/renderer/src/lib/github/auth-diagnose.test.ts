import { describe, expect, it } from 'vitest'
import { computeAuthDiagnostic, parseAuthStatus } from './auth-diagnose'

const TWO_HOSTS = `github.com
  ✓ Logged in to github.com account alice (keyring)
  - Active account: true
  - Token scopes: 'gist', 'read:org', 'repo'

ghe.internal:8443
  ✓ Logged in to ghe.internal:8443 account bob (GITHUB_TOKEN)
  - Active account: false
  - Token scopes: 'repo'
`

describe('parseAuthStatus', () => {
  it('parses multiple hosts, env tokens, scopes, and active flag', () => {
    const accounts = parseAuthStatus(TWO_HOSTS)
    expect(accounts).toHaveLength(2)
    expect(accounts[0]).toMatchObject({
      host: 'github.com', user: 'alice', active: true, envToken: null, source: 'keyring',
      scopes: ['gist', 'read:org', 'repo']
    })
    expect(accounts[1]).toMatchObject({
      host: 'ghe.internal:8443', user: 'bob', active: false, envToken: 'GITHUB_TOKEN', source: 'env'
    })
  })

  it('returns empty for empty input and tolerates missing host header', () => {
    expect(parseAuthStatus('')).toEqual([])
    const accounts = parseAuthStatus('  ✓ Logged in to github.com account carol (keyring)\n')
    expect(accounts[0]?.host).toBe('github.com')
  })
})

describe('computeAuthDiagnostic', () => {
  it('computes missing scopes, keyring fallback, and env token in process', () => {
    const accounts = parseAuthStatus(TWO_HOSTS)
    const diag = computeAuthDiagnostic({ accounts, ghAvailable: true, envTokenInProcess: 'GH_TOKEN', requiredHost: null })
    expect(diag.activeAccount?.user).toBe('alice')
    expect(diag.missingScopes).toEqual(['project'])
    expect(diag.requiredScopes).toEqual(['project', 'read:org', 'repo'])
    expect(diag.envTokenInProcess).toBe('GH_TOKEN')
    expect(diag.hasKeyringFallback).toBe(false)
    expect(diag.requiredHostAuthenticated).toBeNull()
  })

  it('scopes to a required host', () => {
    const accounts = parseAuthStatus(TWO_HOSTS)
    const diag = computeAuthDiagnostic({
      accounts, ghAvailable: true, envTokenInProcess: null, requiredHost: 'GHE.INTERNAL:8443'
    })
    expect(diag.activeAccount?.user).toBe('bob')
    expect(diag.requiredHost).toBe('ghe.internal:8443')
    expect(diag.requiredHostAuthenticated).toBe(true)
  })

  it('reports gh unavailable with no accounts', () => {
    const diag = computeAuthDiagnostic({
      accounts: [], ghAvailable: false, envTokenInProcess: null, requiredHost: null
    })
    expect(diag.ghAvailable).toBe(false)
    expect(diag.activeAccount).toBeNull()
    expect(diag.missingScopes).toEqual(['project', 'read:org', 'repo'])
  })
})
