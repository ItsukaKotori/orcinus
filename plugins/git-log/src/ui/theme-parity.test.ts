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

// git-graph 基色是漂移的另一半高危面（宿主调图谱色时插件不会自动失败）。
const GIT_GRAPH_NAMES = ['ref', 'remote-ref', 'base-ref', 'lane-1', 'lane-2', 'lane-3', 'lane-4', 'lane-5']

// 同一变量在宿主出现两次：第 1 次属 `:root`（浅色），第 2 次属 `.dark`（深色）。
function gitGraphByOccurrence(css: string): { light: Record<string, string>; dark: Record<string, string> } {
  const light: Record<string, string> = {}
  const dark: Record<string, string> = {}
  for (const n of GIT_GRAPH_NAMES) {
    const re = new RegExp(`--git-graph-${n}:\\s*([^;]+);`, 'g')
    const vals = [...css.matchAll(re)].map((m) => m[1]!.trim())
    expect(vals.length, `--git-graph-${n} 应出现两次`).toBeGreaterThanOrEqual(2)
    light[n] = vals[0]
    dark[n] = vals[1]
  }
  return { light, dark }
}

it('插件 git-graph 基色（:root / html.dark）与宿主一致', () => {
  const host = gitGraphByOccurrence(readFileSync(resolve(here, '../../../../src/renderer/src/assets/main.css'), 'utf8'))
  const plugin = gitGraphByOccurrence(readFileSync(resolve(here, './theme.css'), 'utf8'))
  expect(plugin.light).toEqual(host.light)
  expect(plugin.dark).toEqual(host.dark)
})
