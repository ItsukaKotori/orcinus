import { spawnSync } from 'node:child_process'
import { chmodSync, copyFileSync, existsSync, mkdirSync } from 'node:fs'
import path from 'node:path'

const repoRoot = path.resolve(import.meta.dirname, '../..')
const tauriDir = path.join(repoRoot, 'src-tauri')
const sourcePath = path.join(tauriDir, 'target', 'release', 'orcinus')
const outputDir = path.join(tauriDir, 'binaries')
const targetPath = path.join(outputDir, 'orcinus-aarch64-apple-darwin')

console.log('[orcinus-cli] cargo build -p orcinus-cli --release')
run('cargo', ['build', '-p', 'orcinus-cli', '--release'], { cwd: tauriDir })

if (!existsSync(sourcePath)) {
  console.error(`[orcinus-cli] cargo reported success but ${sourcePath} does not exist`)
  process.exit(1)
}

mkdirSync(outputDir, { recursive: true })
copyFileSync(sourcePath, targetPath)
chmodSync(targetPath, 0o755)
console.log(`[orcinus-cli] copied ${sourcePath} -> ${targetPath}`)

function run(command, args, options = {}) {
  const result = spawnSync(command, args, { stdio: 'inherit', ...options })
  if (result.error) {
    fail(`${command} failed to start: ${result.error.message}`)
  }
  if (result.signal) {
    fail(`${command} was killed by signal ${result.signal}`)
  }
  if (result.status !== 0) {
    fail(`${command} exited with status ${result.status}`)
  }
}

function fail(message) {
  console.error(`[orcinus-cli] ${message}`)
  console.error(
    '[orcinus-cli] hint: the Tauri build of orcinus-app copies src-tauri/binaries/orcinus-aarch64-apple-darwin unconditionally, so a fresh clone must run `pnpm build:orcinus-cli` before `cargo test --workspace`, `cargo check -p orcinus-app`, or any `tauri build`'
  )
  process.exit(1)
}
