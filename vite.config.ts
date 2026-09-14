import { resolve } from 'node:path'
import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'
import tailwindcss from '@tailwindcss/vite'

export default defineConfig({
  root: resolve('src/renderer'),
  base: './',
  plugins: [react(), tailwindcss()],
  define: {
    ORCA_FEATURE_WALL_ENABLED: 'true'
  },
  resolve: {
    alias: {
      '@renderer': resolve('src/renderer/src'),
      '@': resolve('src/renderer/src')
    }
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
