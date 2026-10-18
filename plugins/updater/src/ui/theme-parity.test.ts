import { readFileSync } from 'node:fs'
import { describe, expect, it } from 'vitest'

const hostCss = readFileSync('src/renderer/src/assets/main.css', 'utf8')
const pluginCss = readFileSync('plugins/updater/src/ui/theme.css', 'utf8')

function hostThemeBlock(): string {
  const match = hostCss.match(/@theme inline \{[\s\S]*?\n\}/)
  if (!match) {
    throw new Error('host main.css has no @theme inline block')
  }
  return match[0]
}

function parityRegion(): string {
  const match = pluginCss.match(
    /\/\* plugin-theme-parity:begin \*\/([\s\S]*?)\/\* plugin-theme-parity:end \*\//
  )
  if (!match) {
    throw new Error('plugin theme.css has no parity region')
  }
  return match[1]
}

describe('host theme parity', () => {
  it('keeps the host dark variant and @theme mapping verbatim', () => {
    const region = parityRegion()
    expect(region).toContain('@custom-variant dark (&:is(.dark *))')
    expect(region).toContain(hostThemeBlock())
  })

  it('inlines the host Geist font beside the mapping', () => {
    const region = parityRegion()
    expect(region).toContain(
      "url('../../../../src/renderer/src/assets/fonts/Geist-Variable.woff2')"
    )
  })
})
