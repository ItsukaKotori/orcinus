import { build } from 'vite'
import react from '@vitejs/plugin-react'
import tailwindcss from '@tailwindcss/vite'
import { readFile, writeFile, rm } from 'node:fs/promises'
import { fileURLToPath } from 'node:url'
import { dirname, resolve } from 'node:path'
import { assertInlinedSingleFile, inlineSingleFile } from './src/build-inline.ts'

const here = dirname(fileURLToPath(import.meta.url))
const distDir = resolve(here, 'dist')
const repoRoot = resolve(here, '../..')

await rm(distDir, { recursive: true, force: true })

await build({
  root: here,
  configFile: false,
  logLevel: 'warn',
  plugins: [react(), tailwindcss()],
  define: { 'process.env.NODE_ENV': '"production"' },
  resolve: { alias: { '@shared': resolve(repoRoot, 'src/shared') } },
  build: {
    outDir: distDir,
    emptyOutDir: true,
    cssCodeSplit: false,
    assetsInlineLimit: Number.MAX_SAFE_INTEGER,
    minify: 'esbuild',
    rollupOptions: {
      input: resolve(here, 'src/ui/tab-main.tsx'),
      output: {
        format: 'iife',
        inlineDynamicImports: true,
        entryFileNames: 'git-log.js',
        assetFileNames: (info) => (info.name && info.name.endsWith('.css') ? 'git-log[extname]' : 'assets/[name][extname]')
      }
    }
  }
})

const js = await readFile(resolve(distDir, 'git-log.js'), 'utf8')
const css = await readFile(resolve(distDir, 'git-log.css'), 'utf8')
const html = inlineSingleFile({ js, css })
assertInlinedSingleFile(html)
await writeFile(resolve(distDir, 'tab.html'), html)
console.log(`built dist/tab.html (${(html.length / 1024).toFixed(0)} KiB)`)
