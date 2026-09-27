import { readFileSync, writeFileSync, mkdirSync } from 'node:fs'
import { dirname, resolve } from 'node:path'
import { expect, it } from 'vitest'
import {
  getDefaultOnboardingState,
  getDefaultSettings,
  getDefaultUIState
} from '../../src/shared/constants'

const OUT = resolve('src-tauri/crates/ade-core/src/defaults/ade-defaults.generated.json')
const HOME_PLACEHOLDER = '{{HOME}}'

function buildPayload() {
  const settings = getDefaultSettings(HOME_PLACEHOLDER)
  const uiState = getDefaultUIState()
  const onboarding = getDefaultOnboardingState()
  return `${JSON.stringify({ schemaVersion: 1, settings, uiState, onboarding }, null, 2)}\n`
}

it('keeps ade-defaults.generated.json fresh', () => {
  const payload = buildPayload()
  if (process.env.ADE_WRITE_DEFAULTS === '1') {
    mkdirSync(dirname(OUT), { recursive: true })
    writeFileSync(OUT, payload)
    return
  }
  expect(readFileSync(OUT, 'utf8')).toBe(payload)
})
