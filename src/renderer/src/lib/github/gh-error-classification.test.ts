import { describe, expect, it } from 'vitest'
import {
  classifyPRRefreshError,
  safePRRefreshErrorMessage
} from './gh-error-classification'

describe('classifyPRRefreshError', () => {
  it('classifies an HTTP 429 as rate_limited', () => {
    expect(classifyPRRefreshError(new Error('HTTP 429 Too Many Requests'))).toBe('rate_limited')
  })

  it('classifies an HTTP 404 as repo_unavailable', () => {
    expect(classifyPRRefreshError(new Error('HTTP 404 Not Found'))).toBe('repo_unavailable')
  })

  it('classifies an HTTP 503 as server_error', () => {
    expect(classifyPRRefreshError(new Error('HTTP 503 Service Unavailable'))).toBe('server_error')
  })

  it('classifies ECONNRESET as network', () => {
    expect(classifyPRRefreshError(new Error('request failed: ECONNRESET'))).toBe('network')
  })

  it('classifies an HTTP 403 resource denial as permission', () => {
    expect(
      classifyPRRefreshError(new Error('HTTP 403: Resource not accessible by integration'))
    ).toBe('permission')
  })

  it('classifies a spawn gh ENOENT failure as gh_unavailable', () => {
    const err = Object.assign(new Error('spawn gh ENOENT'), { code: 'ENOENT' })
    expect(classifyPRRefreshError(err)).toBe('gh_unavailable')
  })

  it('classifies HTTP 401 bad credentials as auth', () => {
    expect(classifyPRRefreshError(new Error('HTTP 401: bad credentials'))).toBe('auth')
  })

  it('falls back to unknown for unrecognized failures', () => {
    expect(classifyPRRefreshError(new Error('something else'))).toBe('unknown')
  })

  it('reads the GhRunError-style stderr/stdout/code fields', () => {
    const err = Object.assign(new Error('gh exited with code 1'), {
      name: 'GhRunError',
      stderr: 'HTTP 403: Resource not accessible by integration',
      stdout: '',
      code: 1
    })
    expect(classifyPRRefreshError(err)).toBe('permission')
  })
})

describe('safePRRefreshErrorMessage', () => {
  it('returns the stable reference copy for every classified type', () => {
    expect(safePRRefreshErrorMessage('rate_limited')).toBe(
      'GitHub rate limit is low. Try again after the limit resets.'
    )
    expect(safePRRefreshErrorMessage('auth')).toBe(
      'GitHub authentication is unavailable. Check your gh login.'
    )
    expect(safePRRefreshErrorMessage('network')).toBe(
      'GitHub is unreachable right now. Check your network and try again.'
    )
    expect(safePRRefreshErrorMessage('server_error')).toBe(
      "GitHub's API is temporarily unavailable (server error). This is a GitHub-side issue."
    )
    expect(safePRRefreshErrorMessage('permission')).toBe(
      'GitHub did not allow access to this pull request.'
    )
    expect(safePRRefreshErrorMessage('repo_unavailable')).toBe(
      'The GitHub repository is unavailable or cannot be resolved.'
    )
    expect(safePRRefreshErrorMessage('gh_unavailable')).toBe('GitHub CLI is unavailable.')
    expect(safePRRefreshErrorMessage('unknown')).toBe('GitHub pull request refresh failed.')
  })
})
