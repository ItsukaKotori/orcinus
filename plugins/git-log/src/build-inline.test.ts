import { describe, expect, it } from 'vitest'
import { assertInlinedSingleFile, inlineSingleFile } from './build-inline'

describe('inlineSingleFile', () => {
  it('在单个 HTML 内联 style 与 script，且无外部引用', () => {
    const html = inlineSingleFile({ js: 'console.log(1)', css: ':root{--x:1}' })
    expect(html).toContain('<style>:root{--x:1}</style>')
    expect(html).toContain('<script>console.log(1)</script>')
    expect(html).toContain('<div id="git-log-root"></div>')
    assertInlinedSingleFile(html) // 不抛异常
  })

  it('中和内容里的提前闭合 </script>，只保留一个真实闭合标签', () => {
    const html = inlineSingleFile({ js: 'var t="</script><img>", u=1', css: '' })
    expect(html).toContain('<\\/script') // 内容里的 </script 被转义
    expect((html.match(/<\/script>/g) ?? []).length).toBe(1) // 只剩包裹的真实闭合
  })
})

describe('assertInlinedSingleFile', () => {
  it('拒绝外部 script / link / css 引用', () => {
    expect(() => assertInlinedSingleFile('<script src="a.js"></script>')).toThrow(/external/)
    expect(() => assertInlinedSingleFile('<link rel="stylesheet" href="a.css">')).toThrow(/external/)
    expect(() => assertInlinedSingleFile('<style>@import url(http://a)</style>')).toThrow(/external/)
    expect(() => assertInlinedSingleFile('<style>*{background:url(https://x/y.png)}</style>')).toThrow(/external/)
  })

  it('放过仅内联、含 data: 字体与 #hash 的合法文档', () => {
    assertInlinedSingleFile(
      '<style>@font-face{src:url(data:font/woff2;base64,AAA)} a{color:var(--x)}</style><script>/* no imports */</script>'
    )
  })
})
