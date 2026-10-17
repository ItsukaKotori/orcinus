import { describe, expect, it } from 'vitest'
import { getLandingPreflightIssues, type LandingPreflightStatus } from './landing-preflight-issues'

describe('landing preflight issues', () => {
  it('reports the Git install issue', () => {
    const status: LandingPreflightStatus = { git: { installed: false } }
    const issues = getLandingPreflightIssues(status)

    expect(issues.map((issue) => issue.id)).toEqual(['git'])
  })

  it('reports no issues when Git is installed', () => {
    expect(getLandingPreflightIssues({ git: { installed: true } })).toEqual([])
  })
})
