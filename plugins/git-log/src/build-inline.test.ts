import { describe, expect, it } from 'vitest'
import { assertInlinedSingleFile, inlineSingleFile } from './build-inline'

describe('inlineSingleFile', () => {
  it('在单个 HTML 内联 style 与 script，且无任何外部引用', () => {
    const html = inlineSingleFile({ js: 'console.log(1)', css: ':root{--x:1}' })
    expect(html).toContain('<style>:root{--x:1}</style>')
    expect(html).toContain('<script>console.log(1)</script>')
    expect(html).toContain('<div id="git-log-root"></div>')
    assertInlinedSingleFile(html) // 不抛异常
  })

  it('拒绝外链 script / import / @import url / http 引用', () => {
    expect(() => assertInlinedSingleFile('<script src="a.js"></script>')).toThrow(/external/)
    expect(() => assertInlinedSingleFile('<script>import("x")</script>')).toThrow(/dynamic-import/)
    expect(() => assertInlinedSingleFile('<style>@import url(http://a)</style>')).toThrow(/external/)
  })
})
