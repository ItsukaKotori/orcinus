import { resolve } from 'node:path'
import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'
import tailwindcss from '@tailwindcss/vite'

const agentHookNodeShim = resolve('src/renderer/src/lib/browser-node-shims.ts')

export default defineConfig({
  root: resolve('src/renderer'),
  base: './',
  plugins: [react(), tailwindcss()],
  resolve: {
    alias: [
      { find: /^node:buffer$/, replacement: agentHookNodeShim },
      { find: /^node:fs$/, replacement: agentHookNodeShim },
      { find: /^node:fs\/promises$/, replacement: agentHookNodeShim },
      { find: /^node:crypto$/, replacement: agentHookNodeShim },
      { find: /^node:os$/, replacement: agentHookNodeShim },
      { find: /^node:path$/, replacement: agentHookNodeShim },
      { find: /^node:http$/, replacement: agentHookNodeShim },
      { find: '@renderer', replacement: resolve('src/renderer/src') },
      { find: '@', replacement: resolve('src/renderer/src') }
    ]
  },
  clearScreen: false,
  server: {
    host: '127.0.0.1',
    port: 1420,
    strictPort: true
  },
  envPrefix: ['VITE_', 'TAURI_'],
  build: {
    outDir: resolve('dist'),
    emptyOutDir: true
  },
  worker: { format: 'es' }
})
