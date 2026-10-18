import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { dirname } from 'node:path'
import { expect, it } from 'vitest'

const here = dirname(fileURLToPath(import.meta.url))

function extractThemeInline(css: string): string {
  const start = css.indexOf('@theme inline {')
  if (start === -1) throw new Error('no @theme inline block')
  let depth = 0
  let i = css.indexOf('{', start)
  for (; i < css.length; i += 1) {
    if (css[i] === '{') depth += 1
    else if (css[i] === '}') {
      depth -= 1
      if (depth === 0) break
    }
  }
  return css.slice(start, i + 1).replace(/\s+/g, ' ').trim()
}

it('插件 @theme inline 与宿主 main.css 逐字一致', () => {
  const host = readFileSync(resolve(here, '../../../../src/renderer/src/assets/main.css'), 'utf8')
  const plugin = readFileSync(resolve(here, './theme.css'), 'utf8')
  expect(extractThemeInline(plugin)).toBe(extractThemeInline(host))
})
