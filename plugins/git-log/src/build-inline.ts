const FORBIDDEN: Array<[RegExp, string]> = [
  [/<script[^>]*\ssrc=/i, 'external'],
  [/url\(\s*['"]?https?:/i, 'external'],
  [/@import\s+url\(/i, 'external'],
  [/\bimport\s*\(/, 'dynamic-import'],
  [/\bimport\s+[\w{*]/, 'dynamic-import']
]

export function inlineSingleFile({ js, css }: { js: string; css: string }): string {
  return `<!doctype html>
<html>
  <head>
    <meta charset="utf-8" />
    <style>${css}</style>
  </head>
  <body>
    <div id="git-log-root"></div>
    <script>${js}</script>
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
