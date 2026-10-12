import { describe, expect, it } from 'vitest'
import { pluginConsentRequestSchema } from './plugin-consent-request'

describe('pluginConsentRequestSchema', () => {
  it('requires the exact reviewed consent fingerprint', () => {
    expect(
      pluginConsentRequestSchema.safeParse({
        pluginKey: 'orca-samples.demo',
        scope: 'global',
        decision: 'approve'
      }).success
    ).toBe(false)
    expect(
      pluginConsentRequestSchema.safeParse({
        pluginKey: 'orca-samples.demo',
        scope: 'global',
        reviewedFingerprint: '',
        decision: 'approve'
      }).success
    ).toBe(false)
  })

  it('requires a scope so dual-scope plugins resolve to one instance', () => {
    expect(
      pluginConsentRequestSchema.safeParse({
        pluginKey: 'orca-samples.demo',
        reviewedFingerprint: 'sha256-reviewed',
        decision: 'approve'
      }).success
    ).toBe(false)
    expect(
      pluginConsentRequestSchema.safeParse({
        pluginKey: 'orca-samples.demo',
        scope: 'project',
        repoId: 'repo-1',
        reviewedFingerprint: 'sha256-reviewed',
        decision: 'approve'
      }).success
    ).toBe(true)
  })

  it('accepts an explicit opaque fingerprint and rejects contract drift', () => {
    expect(
      pluginConsentRequestSchema.parse({
        pluginKey: 'orca-samples.demo',
        scope: 'global',
        reviewedFingerprint: 'sha256-reviewed',
        decision: 'keep-disabled'
      })
    ).toEqual({
      pluginKey: 'orca-samples.demo',
      scope: 'global',
      reviewedFingerprint: 'sha256-reviewed',
      decision: 'keep-disabled'
    })
    expect(() =>
      pluginConsentRequestSchema.parse({
        pluginKey: 'orca-samples.demo',
        scope: 'global',
        reviewedFingerprint: 'sha256-reviewed',
        decision: 'approve',
        unexpected: true
      })
    ).toThrow()
  })
})
