import { spawnSync } from 'node:child_process'
import { chmodSync, copyFileSync, cpSync, existsSync, mkdirSync, rmSync, writeFileSync } from 'node:fs'
import path from 'node:path'

const repoRoot = path.resolve(import.meta.dirname, '../..')
const packagePath = path.join(repoRoot, 'src-tauri', 'native', 'computer-use-macos')
const binaryPath = path.join(packagePath, '.build', 'release', 'orcinus-computer-use-macos')
const appName = 'Orcinus Computer Use.app'
const stagingAppPath = path.join(packagePath, '.build', 'release', appName)
const outputDir = path.join(repoRoot, 'src-tauri', 'resources', 'computer-use')
const outputAppPath = path.join(outputDir, appName)
const appExecutablePath = path.join('Contents', 'MacOS', 'orcinus-computer-use-macos')
const appIconPath = path.join('Contents', 'Resources', 'AppIcon.icns')
const iconSourcePath = path.join(repoRoot, 'src-tauri', 'icons', 'icon.icns')
const entitlementsPath = path.join(import.meta.dirname, 'entitlements.computer-use.mac.plist')
const bundleId = process.env.ORCINUS_COMPUTER_MACOS_BUNDLE_ID ?? 'dev.itsuka.orcinus.computer-use'
const displayName = 'Orcinus Computer Use'
const signingIdentity = process.env.APPLE_SIGNING_IDENTITY?.trim() || '-'

if (process.platform !== 'darwin') {
  console.log('[computer-use] skipping macOS helper build: platform is not darwin')
  process.exit(0)
}

buildReleaseBinary()
createHelperApp(stagingAppPath)
copyHelperApp(stagingAppPath, outputAppPath)
printSignatureSummary(outputAppPath)
console.log(`[computer-use] built ${outputAppPath}`)

function buildReleaseBinary() {
  console.log('[computer-use] swift build -c release')
  run('swift', ['build', '-c', 'release'], { cwd: packagePath })
  if (!existsSync(binaryPath)) {
    console.error(`[computer-use] expected release binary at ${binaryPath}`)
    process.exit(1)
  }
}

function createHelperApp(appPath) {
  rmSync(appPath, { recursive: true, force: true })
  const executableTarget = path.join(appPath, appExecutablePath)
  const iconTarget = path.join(appPath, appIconPath)
  mkdirSync(path.dirname(executableTarget), { recursive: true })
  mkdirSync(path.dirname(iconTarget), { recursive: true })
  copyFileSync(binaryPath, executableTarget)
  chmodSync(executableTarget, 0o755)
  copyFileSync(iconSourcePath, iconTarget)
  writeFileSync(path.join(appPath, 'Contents', 'Info.plist'), infoPlist(), 'utf8')
  signApp(appPath)
}

function copyHelperApp(from, to) {
  rmSync(to, { recursive: true, force: true })
  mkdirSync(path.dirname(to), { recursive: true })
  cpSync(from, to, { recursive: true })
}

function signApp(appPath) {
  const args = ['--force', '--sign', signingIdentity]
  if (signingIdentity !== '-') {
    args.push('--options', 'runtime', '--timestamp', '--entitlements', entitlementsPath)
  } else {
    console.warn(
      '[computer-use] ad-hoc signing: every rebuild changes the CDHash, so macOS TCC will ask for Accessibility/Screen Recording again'
    )
  }
  args.push(appPath)
  run('codesign', args)
}

function printSignatureSummary(appPath) {
  const result = spawnSync('codesign', ['-dv', appPath], { encoding: 'utf8' })
  if (result.stdout) console.log(result.stdout.trim())
  if (result.stderr) console.log(result.stderr.trim())
}

function run(command, args, options = {}) {
  const result = spawnSync(command, args, { stdio: 'inherit', ...options })
  if (result.error) {
    console.error(`[computer-use] failed to run ${command}: ${result.error.message}`)
    process.exit(1)
  }
  if (result.signal) {
    process.kill(process.pid, result.signal)
  }
  if (result.status !== 0) {
    process.exit(result.status ?? 1)
  }
}

function infoPlist() {
  return `<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleDevelopmentRegion</key>
  <string>en</string>
  <key>CFBundleExecutable</key>
  <string>orcinus-computer-use-macos</string>
  <key>CFBundleIdentifier</key>
  <string>${escapePlist(bundleId)}</string>
  <key>CFBundleInfoDictionaryVersion</key>
  <string>6.0</string>
  <key>CFBundleIconFile</key>
  <string>AppIcon</string>
  <key>CFBundleName</key>
  <string>${escapePlist(displayName)}</string>
  <key>CFBundleDisplayName</key>
  <string>${escapePlist(displayName)}</string>
  <key>CFBundlePackageType</key>
  <string>APPL</string>
  <key>CFBundleShortVersionString</key>
  <string>1.0</string>
  <key>CFBundleVersion</key>
  <string>1</string>
  <key>LSMinimumSystemVersion</key>
  <string>14.0</string>
  <key>LSUIElement</key>
  <false/>
  <key>NSAccessibilityUsageDescription</key>
  <string>Orcinus Computer Use needs Accessibility permission to read and interact with app interfaces when you ask Orcinus to use apps.</string>
  <key>NSScreenCaptureUsageDescription</key>
  <string>Orcinus Computer Use needs Screen Recording permission to capture app windows when you ask Orcinus to inspect your screen.</string>
</dict>
</plist>
`
}

function escapePlist(value) {
  return value
    .replaceAll('&', '&amp;')
    .replaceAll('<', '&lt;')
    .replaceAll('>', '&gt;')
    .replaceAll('"', '&quot;')
    .replaceAll("'", '&apos;')
}
