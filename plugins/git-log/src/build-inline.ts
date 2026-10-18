// Structural external-reference checkers for the CSP panel contract.
// NOTE: we deliberately do NOT text-scan for `import`/`import()` here — the
// build uses rollup `format: 'iife'` + `inlineDynamicImports: true`, which
// provably emits no module `import`/`import()` at all, while a raw text scan
// would false-positive on UI/fixture copy (e.g. a label reading "import").
const FORBIDDEN: Array<[RegExp, string]> = [
  [/<script[^>]*\ssrc=/i, 'external-script'],
  [/<link[^>]*\shref=/i, 'external-link'],
  [/url\(\s*['"]?https?:/i, 'external-url'],
  [/@import\s+url\(/i, 'external-css-import']
]

export function inlineSingleFile({ js, css }: { js: string; css: string }): string {
  // Neutralize any premature `</script>` / `</style>` inside the inlined
  // payload so it cannot close the wrapper tag early. In JS `\/` === `/`, so
  // the runtime value is unchanged; in CSS `\/` is a valid escaped `/`.
  const safeJs = js.replace(/<\/(script)/gi, '<\\/$1')
  const safeCss = css.replace(/<\/(style)/gi, '<\\/$1')
  return `<!doctype html>
<html>
  <head>
    <meta charset="utf-8" />
    <style>${safeCss}</style>
  </head>
  <body>
    <div id="git-log-root"></div>
    <script>${safeJs}</script>
  </body>
</html>
`
}

export function assertInlinedSingleFile(html: string): void {
  for (const [pattern, label] of FORBIDDEN) {
    if (pattern.test(html)) {
      throw new Error(`inline violation (${label}): ${pattern}`)
    }
  }
}
