// @vitest-environment happy-dom

import { act } from 'react'
import { createRoot, type Root } from 'react-dom/client'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { SkillFreshnessInventory } from '../../../../shared/skill-freshness'
import { SkillFreshnessStatusPill } from './SkillFreshnessStatusPill'

const mocks = vi.hoisted(() => ({
  inventory: null as SkillFreshnessInventory | null,
  loading: false,
  error: null as string | null
}))

vi.mock('@/hooks/useSkillFreshness', () => ({
  useSkillFreshness: () => ({
    inventory: mocks.inventory,
    loading: mocks.loading,
    error: mocks.error,
    refresh: vi.fn()
  })
}))

function pillText(container: HTMLDivElement): string {
  return container.textContent ?? ''
}

function inventory(
  entries: { name: string; status: 'current' | 'outdated' | 'unrecognized' }[],
  eligibleUpdateNames: string[]
): SkillFreshnessInventory {
  return {
    schemaVersion: 1,
    installations: entries.map((entry, index) => ({
      id: `${entry.name}-${index}`,
      name: entry.name,
      rootId: 'home-agents',
      providers: ['agent-skills'],
      sourceKind: 'home',
      sourceLabel: 'Agent skills home',
      unresolvedPath: `/home/.agents/skills/${entry.name}`,
      resolvedPath: `/home/.agents/skills/${entry.name}`,
      physicalIdentity: `physical-${entry.name}-${index}`,
      topology: 'canonical-copy',
      status: entry.status,
      installedReleaseRevision: 1,
      installedAppVersion: '1.0.0',
      currentReleaseRevision: 2,
      currentPackageDigest: 'current',
      currentAppVersion: '2.0.0',
      observedPackageDigest: 'old',
      errorCategory: null
    })),
    eligibleUpdateNames,
    scanIssues: [],
    scannedAt: 1
  }
}

let root: Root | null = null
let container: HTMLDivElement | null = null

async function renderPill(skillName: string): Promise<HTMLDivElement> {
  container = document.createElement('div')
  document.body.appendChild(container)
  root = createRoot(container)
  await act(async () => {
    root?.render(<SkillFreshnessStatusPill skillName={skillName} />)
  })
  return container
}

describe('SkillFreshnessStatusPill', () => {
  beforeEach(() => {
    mocks.inventory = null
    mocks.loading = false
    mocks.error = null
  })

  afterEach(async () => {
    if (root) {
      await act(async () => root?.unmount())
    }
    root = null
    container?.remove()
    container = null
  })

  it('shows Update available for an eligible outdated skill', async () => {
    mocks.inventory = inventory([{ name: 'orca-cli', status: 'outdated' }], ['orca-cli'])

    const rendered = await renderPill('orca-cli')
    expect(pillText(rendered)).toBe('Update available')
    // Why: the review dialog was removed in Phase 0, so no inert Details action may render.
    expect(rendered.querySelector('[data-slot="button"]')).toBeNull()
  })

  it('shows Up to date when every placement is current', async () => {
    mocks.inventory = inventory([{ name: 'orca-cli', status: 'current' }], [])

    const rendered = await renderPill('orca-cli')
    expect(pillText(rendered)).toBe('Up to date')
    expect(rendered.querySelector('[data-slot="button"]')).toBeNull()
  })

  it('flags a blocked outdated placement instead of reading as all-clear', async () => {
    mocks.inventory = inventory(
      [
        { name: 'orca-cli', status: 'outdated' },
        { name: 'orca-cli', status: 'unrecognized' }
      ],
      []
    )

    const rendered = await renderPill('orca-cli')
    // Why: a green pill over a copy the update cannot reach hides real drift.
    expect(pillText(rendered)).toBe('Review skill')
    expect(rendered.querySelector('[data-slot="button"]')).toBeNull()
  })

  it('falls back to Installed before the inventory loads', async () => {
    const rendered = await renderPill('orca-cli')
    expect(pillText(rendered)).toBe('Installed')
  })

  it('shows a neutral verdict while freshness is being checked', async () => {
    mocks.loading = true

    const rendered = await renderPill('orca-cli')
    expect(pillText(rendered)).toBe('Checking...')
  })

  it('does not report success after a freshness check fails', async () => {
    mocks.error = 'Could not read skills.'

    const rendered = await renderPill('orca-cli')
    expect(pillText(rendered)).toBe('Check failed')
  })
})
