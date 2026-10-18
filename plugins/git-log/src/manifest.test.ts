import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { dirname } from 'node:path'
import { expect, it } from 'vitest'

const manifest = JSON.parse(
  readFileSync(resolve(dirname(fileURLToPath(import.meta.url)), '../ade-plugin.json'), 'utf8')
) as { capabilities: Array<{ kind: string }>; version: string }

it('manifest 声明 git:read（增量 2a）', () => {
  const kinds = manifest.capabilities.map((capability) => capability.kind)
  expect(kinds).toContain('git:read')
  expect(kinds).toContain('workspace:open-tab')
})
