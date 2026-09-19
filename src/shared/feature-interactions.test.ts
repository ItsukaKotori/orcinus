import { readdirSync, readFileSync, statSync } from 'node:fs'
import { join, relative } from 'node:path'
import { describe, expect, it } from 'vitest'
import {
  FEATURE_INTERACTIONS,
  hasFeatureInteraction,
  normalizeFeatureInteractions,
  type FeatureInteractionId
} from './feature-interactions'
import { escapeRegex } from './string-utils'

type DefinedFeatureInteractionId = (typeof FEATURE_INTERACTIONS)[number]['id']
type MissingFeatureInteractionId = Exclude<FeatureInteractionId, DefinedFeatureInteractionId>
type ExtraFeatureInteractionId = Exclude<DefinedFeatureInteractionId, FeatureInteractionId>

const REPO_ROOT = join(__dirname, '..', '..')
const SOURCE_ROOTS = ['src/main', 'src/renderer/src', 'src/preload']
const PRODUCTION_FILE_PATTERN = /\.(ts|tsx)$/
const TEST_FILE_PATTERN = /(?:^|\.)(test|spec)\.(ts|tsx)$/

describe('feature interactions', () => {
  it('defines local interaction semantics for product education features', () => {
    const catalogMatchesPublicUnion: [
      MissingFeatureInteractionId,
      ExtraFeatureInteractionId
    ] extends [never, never]
      ? true
      : never = true
    const expectedIds: FeatureInteractionId[] = [
      'workspace-agent-sessions',
      'cmd-j',
      'cmd-j-workspace-open',
      'cmd-j-browser-page-open',
      'cmd-j-settings-open',
      'cmd-j-quick-action',
      'cmd-j-create-workspace',
      'browser',
      'client-hosted-browser',
      'browser-tab-created',
      'tasks',
      'github-tasks',
      'gitlab-tasks',
      'linear-tasks',
      'jira-tasks',
      'automations',
      'automation-created',
      'automation-run',
      'browser-annotations',
      'browser-annotations-sent-to-agent',
      'browser-grab',
      'markdown-file-created',
      'workspace-creation',
      'agent-browser-setup',
      'agent-browser-use',
      'ephemeral-vm-setup',
      'agent-orchestration-setup',
      'agent-orchestration',
      'ai-commit-generation',
      'ai-pr-generation',
      'claude-account-switching',
      'computer-use-setup',
      'computer-use',
      'codex-account-switching',
      'cookie-import',
      'floating-workspace',
      'floating-workspace-hidden',
      'mobile-pairing',
      'notifications',
      'ports',
      'quick-commands',
      'resource-manager',
      'review-notes',
      'ssh',
      'terminal-pane-split',
      'terminal-panes',
      'terminal-tabs',
      'tab-splits',
      'usage-tracking',
      'workspace-cleanup'
    ]

    expect(catalogMatchesPublicUnion).toBe(true)
    expect(FEATURE_INTERACTIONS.map((feature) => feature.id)).toEqual(expectedIds)
    for (const feature of FEATURE_INTERACTIONS) {
      expect(feature.interaction.length).toBeGreaterThan(0)
    }
  })

  it('normalizes persisted records by removing unknown ids and malformed values', () => {
    expect(
      normalizeFeatureInteractions({
        tasks: { firstInteractedAt: 100 },
        browser: { firstInteractedAt: Number.NaN },
        automations: { firstInteractedAt: 200, interactionCount: 3 },
        'browser-grab': { firstInteractedAt: 250, interactionCount: 0 },
        unknown: { firstInteractedAt: 200 },
        'usage-tracking': { firstInteractedAt: 300 }
      })
    ).toEqual({
      tasks: { firstInteractedAt: 100, interactionCount: 1 },
      automations: { firstInteractedAt: 200, interactionCount: 3 },
      'browser-grab': { firstInteractedAt: 250, interactionCount: 1 },
      'usage-tracking': { firstInteractedAt: 300, interactionCount: 1 }
    })
  })

  it('treats only valid known records as interacted', () => {
    expect(
      hasFeatureInteraction({ tasks: { firstInteractedAt: 100, interactionCount: 1 } }, 'tasks')
    ).toBe(true)
    expect(
      hasFeatureInteraction({ tasks: { firstInteractedAt: 100, interactionCount: 1 } }, 'browser')
    ).toBe(false)
    expect(
      hasFeatureInteraction(
        { tasks: { firstInteractedAt: Number.POSITIVE_INFINITY, interactionCount: 1 } },
        'tasks'
      )
    ).toBe(false)
  })

  it('keeps every catalog id wired to a production writer', () => {
    const productionText = collectProductionSourceText()
    const missingWriters = FEATURE_INTERACTIONS.map((feature) => feature.id).filter((id) => {
      const escaped = escapeRegex(id)
      const directRecord = new RegExp(
        `recordFeatureInteraction(?:\\?\\.)?\\(\\s*['"]${escaped}['"]`
      )
      const runtimeMappingReturn = new RegExp(`return[^\\n]*['"]${escaped}['"]`)
      return !directRecord.test(productionText) && !runtimeMappingReturn.test(productionText)
    })

    expect(missingWriters).toEqual([])
  }, 15_000)
})

function collectProductionSourceText(): string {
  const files = SOURCE_ROOTS.flatMap((root) => collectSourceFiles(join(REPO_ROOT, root)))
  return files
    .sort()
    .map((file) => readFileSync(file, 'utf8'))
    .join('\n')
}

function collectSourceFiles(directory: string): string[] {
  const files: string[] = []
  for (const entry of readdirSync(directory)) {
    const path = join(directory, entry)
    const stats = statSync(path)
    if (stats.isDirectory()) {
      if (entry === 'node_modules' || entry === 'dist' || entry === 'out') {
        continue
      }
      files.push(...collectSourceFiles(path))
      continue
    }
    const repoRelativePath = relative(REPO_ROOT, path)
    if (!PRODUCTION_FILE_PATTERN.test(entry) || TEST_FILE_PATTERN.test(entry)) {
      continue
    }
    // Why: the catalog itself proves the id exists, not that runtime code writes it.
    if (repoRelativePath === 'src/shared/feature-interactions.ts') {
      continue
    }
    files.push(path)
  }
  return files
}
