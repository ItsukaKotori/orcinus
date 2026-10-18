import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { dirname } from 'node:path'
import { expect, it } from 'vitest'

const html = readFileSync(resolve(dirname(fileURLToPath(import.meta.url)), '../launcher.html'), 'utf8')

it('launcher 内联调用 workspace.openTab 且无外部引用', () => {
  expect(html).toContain("'workspace.openTab'")
  expect(html).toContain("tabId: 'git-log'")
  // 处理 host 回执（成功/失败可见）
  expect(html).toContain('orca-panel-action-result')
  // CSP：无外部 script/link/网络字体
  expect(html).not.toMatch(/<script[^>]+\ssrc=/i)
  expect(html).not.toMatch(/<link[^>]+\shref=/i)
  expect(html).not.toMatch(/@import\s+url\(/i)
  expect(html).not.toMatch(/url\(\s*['"]?https?:/i)
})
