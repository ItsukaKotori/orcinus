import { describe, expect, it } from 'vitest'

function readRuntimeSpecifiers(source: string): string[] {
  const specifiers: string[] = []
  const lines = source.split('\n')
  for (let index = 0; index < lines.length; index += 1) {
    const firstLine = lines[index].trimStart()
    const isRuntimeImport =
      firstLine.startsWith('import ') &&
      !firstLine.startsWith('import type ') &&
      !/^import\s+[\w$]+\s*=\s*require\s*\(/.test(firstLine)
    const isRuntimeReexport =
      (firstLine.startsWith('export {') || firstLine.startsWith('export *')) &&
      !firstLine.startsWith('export type ')
    if (!isRuntimeImport && !isRuntimeReexport) {
      continue
    }
    let statement = firstLine
    while (!/(?:^import\s*['"]|\bfrom\s*['"])/.test(statement) && index + 1 < lines.length) {
      index += 1
      statement += `\n${lines[index]}`
    }
    const match =
      statement.match(/^import\s*['"]([^'"]+)['"]/) ?? statement.match(/\bfrom\s*['"]([^'"]+)['"]/)
    if (match) {
      specifiers.push(match[1])
    }
  }
  for (const match of source.matchAll(/\bimport\s*\(\s*['"]([^'"]+)['"]\s*\)/g)) {
    specifiers.push(match[1])
  }
  for (const match of source.matchAll(/\brequire\s*\(\s*['"]([^'"]+)['"]\s*\)/g)) {
    specifiers.push(match[1])
  }
  return specifiers
}

describe('agent hook listener relay dependency boundary', () => {
  it('detects every supported runtime dependency syntax while excluding type imports', () => {
    const source = [
      "import type { TypeOnly } from './type-only'",
      'import {',
      '  runtime',
      "} from './static-runtime'",
      "import './side-effect'",
      "const dynamic = import('./dynamic-runtime')",
      "export { runtimeExport } from './runtime-export'",
      "export * as runtimeNamespace from './namespace-runtime'",
      "import imported = require('./ts-import-equals')",
      "const required = require('./required-runtime')"
    ].join('\n')

    expect(readRuntimeSpecifiers(source)).toEqual([
      './static-runtime',
      './side-effect',
      './runtime-export',
      './namespace-runtime',
      './dynamic-runtime',
      './ts-import-equals',
      './required-runtime'
    ])
  })
})
