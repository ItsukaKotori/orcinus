#!/usr/bin/env node
import { spawnSync } from 'node:child_process'

const result = spawnSync('pnpm', ['rebuild', 'node-pty'], {
  stdio: 'inherit',
  shell: process.platform === 'win32',
  env: { ...process.env, npm_config_build_from_source: 'true' }
})
process.exit(result.status ?? 1)
