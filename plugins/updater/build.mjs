#!/usr/bin/env node
import { rm } from 'node:fs/promises'
import path from 'node:path'
import { fileURLToPath } from 'node:url'
import { build as esbuildBuild } from 'esbuild'
import { build as viteBuild } from 'vite'
import react from '@vitejs/plugin-react'
import tailwindcss from '@tailwindcss/vite'

const root = path.dirname(fileURLToPath(import.meta.url))
const distDir = path.join(root, 'dist')
const uiRoot = path.join(root, 'src', 'ui')

// Inlines every emitted JS/CSS asset into its HTML entry: the panel CSP allows
// only inline scripts/styles, so dist entries must be single-file documents.
function singleFileInline() {
  return {
    name: 'updater-single-file-inline',
    enforce: 'post',
    generateBundle(_options, bundle) {
      for (const html of Object.values(bundle)) {
        if (html.type !== 'asset' || !html.fileName.endsWith('.html')) continue
        let source = String(html.source)
        for (const [fileName, file] of Object.entries(bundle)) {
          if (file === html) continue
          const escaped = fileName.replaceAll('.', '\\.')
          if (file.type === 'chunk') {
            const pattern = new RegExp(`<script[^>]*src="[^"]*${escaped}"[^>]*></script>`)
            const code = file.code.replaceAll('</script', '<\\/script')
            // Function replacement (not a string) so that `$&`, `$1`, `$'`, …
            // inside the bundled code are treated literally instead of being
            // interpreted as String.replace substitution patterns.
            source = source.replace(pattern, () => `<script type="module">${code}</script>`)
            delete bundle[fileName]
          } else if (fileName.endsWith('.css')) {
            const pattern = new RegExp(`<link[^>]*href="[^"]*${escaped}"[^>]*>`)
            source = source.replace(pattern, () => `<style>${String(file.source)}</style>`)
            delete bundle[fileName]
          }
        }
        html.source = source
      }
    }
  }
}

await rm(distDir, { recursive: true, force: true })

await esbuildBuild({
  entryPoints: [path.join(root, 'src', 'worker', 'main.ts')],
  bundle: true,
  platform: 'node',
  format: 'esm',
  target: 'node18',
  outfile: path.join(distDir, 'main.mjs'),
  logLevel: 'warning'
})

// Each UI entry is built in its own single-input pass: a multi-entry build code-
// splits the shared React jsx-runtime into a chunk that the single-file inliner
// cannot reach (it is imported from the entry chunk's code, not referenced by a
// <script src>), leaving a dangling module import in the emitted HTML. Building
// one entry per pass yields a fully self-contained chunk with no imports, so the
// inlined document has no external references.
for (const entry of [path.join(uiRoot, 'tab.html'), path.join(uiRoot, 'panel.html')]) {
  await viteBuild({
    configFile: false,
    root: uiRoot,
    publicDir: false,
    base: './',
    logLevel: 'warn',
    plugins: [react(), tailwindcss(), singleFileInline()],
    build: {
      target: 'es2022',
      outDir: distDir,
      emptyOutDir: false,
      assetsInlineLimit: 128 * 1024,
      modulePreload: false,
      cssCodeSplit: false,
      sourcemap: false,
      rollupOptions: {
        input: entry
      }
    }
  })
}

console.log(`updater plugin built -> ${distDir}`)
