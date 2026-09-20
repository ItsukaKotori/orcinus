import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import { glob } from 'tinyglobby'
import { expect, it } from 'vitest'

const ROOTS = [
  'src/renderer/**/*.{ts,tsx}',
  'src/shared/**/*.{ts,tsx}',
  'src/bridge/**/*.{ts,tsx}',
  'src/preload/api-types.ts',
  'src/preload/api/*-api.ts'
]

const ELECTRON_SPECIFIER =
  /(?:from\s*'electron'|from\s*"electron"|require\(\s*'electron'\s*\)|require\(\s*"electron"\s*\)|import\(\s*'electron'\s*\)|import\(\s*"electron"\s*\))/

it('keeps the renderer/shared/bridge type graph free of electron', async () => {
  const offenders = (await glob(ROOTS, { cwd: resolve('.'), absolute: false }))
    .filter((file) => !/\.test\.tsx?$/.test(file))
    .filter((file) => ELECTRON_SPECIFIER.test(readFileSync(file, 'utf8')))
  expect(offenders).toEqual([])
})
