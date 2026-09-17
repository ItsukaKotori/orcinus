import { resolve } from 'node:path'
import { configDefaults, defineConfig } from 'vitest/config'

const windowsTestWorkerOptions = process.platform === 'win32' ? { maxWorkers: 4 } : {}

const electronTestShim = resolve('src/types/electron-vitest-stub.ts')

export default defineConfig({
  resolve: {
    alias: [
      // Phase 0: renderer tests can reach Electron-backed src/main modules; fail loudly through
      // the throwing stub instead of an unresolved-module crash. Remove with the Phase 1 shim cleanup.
      { find: /^electron$/, replacement: electronTestShim },
      { find: '@renderer', replacement: resolve('src/renderer/src') },
      { find: '@', replacement: resolve('src/renderer/src') }
    ]
  },
  test: {
    environment: 'node',
    // Why: Node 26's undefined Web Storage globals prevent Vitest from installing happy-dom's.
    // Why --expose-gc: retention tests need a deterministic collection point to measure what a queue really holds.
    execArgv: ['--no-experimental-webstorage', '--expose-gc'],
    // Why: happy-dom drops MutationObserver callbacks on GC; keep them alive like a browser does.
    setupFiles: [
      resolve('config/scripts/happy-dom-offscreen-canvas.ts'),
      resolve('config/scripts/happy-dom-mutation-observer-retention.ts'),
      resolve('config/scripts/vitest-host-ports-setup.ts')
    ],
    include: ['src/**/*.test.ts', 'src/**/*.test.tsx'],
    exclude: [
      ...configDefaults.exclude,
      // Phase 0: imports Electron-backed src/main module; see Phase 1 shim removal. (This file's failures are the un-forked mobile/ sources it reads.)
      'src/renderer/src/app-shell/workspace-view-cross-client-sync.test.tsx'
    ],
    // Why: the full suite runs heavy TS transforms plus real git/http fixtures;
    // the Vitest 5s defaults are too tight for the slowest integration cases.
    hookTimeout: 60_000,
    testTimeout: 30_000,
    // Why: Windows process and shell startup are slower under full-suite load;
    // macOS/Linux keep Vitest's default worker parallelism.
    ...windowsTestWorkerOptions
  }
})
