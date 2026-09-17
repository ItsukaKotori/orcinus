# Orcinus 功能删减实施计划（Phase 1 瘦身）

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 从 Orcinus 渲染层彻底删除 10 个功能域（Pet、Contextual Tours、Feature Tips、Feature Wall、Setup Guide、Dictation、Emulator、Activity/Dashboard、Native Chat、Telemetry），含测试、mock bridge 域、preload 契约、i18n 键与独占依赖，删除后应用可构建、可启动、保留功能可用。

**Architecture:** 按耦合度排序 8 步执行；每域先做生产可达性分析确定「独占文件」与「共享引用点」，用统一工具整域 `git rm` 并在每次删除后跑 `pnpm typecheck && pnpm build:web`，编译器驱动解耦共享文件；收尾清理契约/mock/store/设置/i18n/ratchet；每域一个可回滚提交；最后全局可达性兜底 + 文档收尾。

**Tech Stack:** TypeScript 7（`tsc --noEmit`）、Vite（rolldown-vite）、React 19、Vitest 4、pnpm、git。无 Rust 改动。

**Spec:** `docs/superpowers/specs/2026-09-17-feature-trim-design.md`

## Global Constraints

- 分支 `phase1-trim-features`（已创建，spec 提交 `de1f6b8`）。全程不切分支、不 merge、不 push。
- 门禁：每次删除后 `pnpm typecheck && pnpm build:web` 必须 exit 0（`build:web` = `vite build`）。
- 删除一律用 `git rm -q --`（工具已封装），禁止裸 `rm`。
- 删除节奏：域目录内部文件互相引用，默认**整域一次 `git rm` + 一次门禁**（`orcinus-batch-rm.mjs` 默认行为）；跨域混合批或需控制爆炸半径时用 `--chunk 20`（每 20 文件一次门禁）。门禁失败时按报错修引用后重跑，已删文件不会重复列出。
- 每域收尾跑守卫测试：`pnpm test src/renderer/src/lazy-use-ref-ratchet.test.ts src/renderer/src/lazy-modal-mount-state.test.ts src/renderer/src/renderer-node-builtin-boundary.test.ts src/bridge src/renderer/src/i18n`
- `pnpm test` 基线存在既有失败（Phase 1 backlog 第 2 项）。Task 0 记录基线，Task 8 对比；不得新增失败。
- 不删：终端/PTY、worktree/项目模型、agent 状态、编辑器、source control、设置页宿主、i18n 框架、mock bridge 主体、Onboarding、SSH/远程/Web（死树另行处理）。
- 本次与 Phase 0 惯例不同：**含 preload 契约删除**（`src/preload/api/*`、`api-types.ts`、`index.ts` 对应项）。
- 一次性工具脚本放 `/tmp`，不进仓库；不新增产品代码。
- 每域一个提交，消息格式 `chore: 移除 <域>（含契约与测试）`。
- 人工 GUI 冒烟合并为 Task 8 一次执行（spec §5 的人工冒烟按域执行的替代：mock bridge 下每域开窗成本高，改为自动门禁逐域 + 收尾一次人工冒烟）。

## 文件结构

**一次性工具（Task 0 创建，均不进仓库）**

- `/tmp/orcinus-reach.mjs` — 生产可达性分析：入口 → 全库，输出不可达文件清单
- `/tmp/orcinus-batch-rm.mjs` — `git rm` 匹配文件 + `typecheck`/`build:web` 门禁（默认整域一次；`--chunk N` 可选分批门禁）
- `/tmp/orcinus-domain-keys.sh` — 从待删文件收集 i18n 候选键
- `/tmp/orcinus-i18n-prune.mjs` — 安全删键：剩余源码无引用才从 7 个 catalog 删除

**仓库内改动**

- Create（Task 8）: `docs/phase1-feature-trim-record.md`
- Modify（Task 8）: `docs/superpowers/specs/2026-09-14-ade-design.md`
- Delete/Modify（Tasks 1–7）: 见各任务「Files」；每域的「独占文件清单」以 reach 脚本的 before/after diff 为准

---

## Task 0: 基线与一次性工具

**Files:**
- Create: `/tmp/orcinus-reach.mjs`
- Create: `/tmp/orcinus-batch-rm.mjs`
- Create: `/tmp/orcinus-domain-keys.sh`
- Create: `/tmp/orcinus-i18n-prune.mjs`
- Create: `/tmp/orcinus-reach-baseline.txt`、`/tmp/orcinus-test-baseline.log`、`/tmp/orcinus-test-baseline-failures.txt`

**Interfaces:**
- Consumes: 无
- Produces: 上述 4 个工具（Tasks 1–8 调用）；`/tmp/orcinus-reach-baseline.txt`（既有不可达文件基线，Task 8 用）；`/tmp/orcinus-test-baseline-failures.txt`（既有失败清单，Task 8 用）

- [ ] **Step 1: 确认分支与工作区**

```bash
git status -sb   # 期望：## phase1-trim-features，无未提交改动
```

- [ ] **Step 2: 记录构建门禁基线**

```bash
pnpm typecheck        # 期望 exit 0
pnpm build:web        # 期望 exit 0
```

- [ ] **Step 3: 记录测试基线（耗时较长，可 30+ 分钟）**

```bash
pnpm test 2>&1 | tee /tmp/orcinus-test-baseline.log; echo "exit=${PIPESTATUS[0]}"
grep -E "^ *(Test Files|Tests) " /tmp/orcinus-test-baseline.log | tail -4
grep -E "FAIL " /tmp/orcinus-test-baseline.log | sed 's/.*FAIL //' | sort -u > /tmp/orcinus-test-baseline-failures.txt
wc -l /tmp/orcinus-test-baseline-failures.txt   # 期望非 0（既有失败）
```

- [ ] **Step 4: 创建可达性分析脚本** `/tmp/orcinus-reach.mjs`

```js
// 生产可达性分析：入口 = renderer 三个 html + src/main、src/preload、src/bridge、src/types 全量
// 解析 from '...' / import('...') / require('...')，支持相对路径、@/、@renderer/ 别名（仅字面量）。
// Usage: node /tmp/orcinus-reach.mjs [--out list.txt]
import { existsSync, readFileSync, readdirSync, statSync, writeFileSync } from 'node:fs'
import { dirname, join, resolve } from 'node:path'

const ROOT = process.cwd()
const SRC = join(ROOT, 'src')
const TESTS = /\.(test|spec)\.tsx?$/
const CODE = /\.(ts|tsx)$/

function walk(dir, out = []) {
  if (!existsSync(dir)) return out
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    const full = join(dir, entry.name)
    if (entry.isDirectory()) {
      if (entry.name === 'node_modules' || entry.name === 'dist') continue
      walk(full, out)
    } else if (CODE.test(entry.name) && !TESTS.test(entry.name)) {
      out.push(full)
    }
  }
  return out
}

const universe = [
  ...walk(join(SRC, 'renderer', 'src')),
  ...walk(join(SRC, 'shared')),
  ...walk(join(SRC, 'preload')),
  ...walk(join(SRC, 'bridge')),
  ...walk(join(SRC, 'main')),
  ...walk(join(SRC, 'types'))
]

const EXTS = ['', '.ts', '.tsx', '.js', '.jsx']
function tryResolve(base) {
  for (const ext of EXTS) {
    const p = base + ext
    if (existsSync(p) && statSync(p).isFile()) return p
  }
  for (const index of ['index.ts', 'index.tsx']) {
    const p = join(base, index)
    if (existsSync(p) && statSync(p).isFile()) return p
  }
  return null
}

function resolveSpecifier(fromFile, spec) {
  if (spec.startsWith('@renderer/')) return tryResolve(join(SRC, 'renderer', 'src', spec.slice('@renderer/'.length)))
  if (spec.startsWith('@/')) return tryResolve(join(SRC, 'renderer', 'src', spec.slice(2)))
  if (spec.startsWith('.')) {
    const base = resolve(dirname(fromFile), spec)
    const direct = tryResolve(base)
    if (direct) return direct
    // TS 的 ESM 风格 .js → .ts/.tsx 映射
    if (base.endsWith('.js')) return tryResolve(base.slice(0, -3))
    return null
  }
  return null
}

const IMPORT_RE = /(?:from\s*|import\s*\(\s*|require\s*\(\s*)['"]([^'"]+)['"]/g
const HTML_SCRIPT_RE = /<script[^>]*\bsrc=["']([^"']+)["']/g

function importsOf(file) {
  const source = readFileSync(file, 'utf8')
  const specs = []
  if (file.endsWith('.html')) {
    for (const m of source.matchAll(HTML_SCRIPT_RE)) specs.push(m[1])
    return specs
  }
  for (const m of source.matchAll(IMPORT_RE)) specs.push(m[1])
  return specs
}

const roots = []
for (const html of ['src/renderer/index.html', 'src/renderer/popout.html', 'src/renderer/web-index.html']) {
  if (existsSync(join(ROOT, html))) {
    for (const spec of importsOf(join(ROOT, html))) {
      const abs = tryResolve(join(SRC, 'renderer', spec.replace(/^\//, '')))
      if (abs) roots.push(abs)
    }
  }
}
roots.push(...walk(join(SRC, 'main')), ...walk(join(SRC, 'preload')), ...walk(join(SRC, 'bridge')), ...walk(join(SRC, 'types')))

const reachable = new Set()
const queue = [...roots]
while (queue.length > 0) {
  const file = queue.pop()
  if (reachable.has(file)) continue
  reachable.add(file)
  for (const spec of importsOf(file)) {
    const target = resolveSpecifier(file, spec)
    if (target && !reachable.has(target)) queue.push(target)
  }
}

const unreachable = universe
  .filter((f) => !reachable.has(f))
  .map((f) => f.slice(ROOT.length + 1))
  .sort()

const outIndex = process.argv.indexOf('--out')
if (outIndex !== -1 && process.argv[outIndex + 1]) {
  writeFileSync(process.argv[outIndex + 1], unreachable.join('\n') + (unreachable.length ? '\n' : ''))
}
console.log(`reachable=${reachable.size} unreachable=${unreachable.length}`)
console.log(unreachable.join('\n'))
```

- [ ] **Step 5: 创建删除脚本** `/tmp/orcinus-batch-rm.mjs`

```js
// Usage: node /tmp/orcinus-batch-rm.mjs [--chunk N] <pathspec...>
// 默认：一次性 git rm 全部匹配文件，然后跑一次 pnpm typecheck && pnpm build:web。
// --chunk N：每 N 个文件一次 git rm，逐批门禁（用于跨域混合批）。
// 门禁失败即停；按报错修引用后重跑本命令（git ls-files 不会列出已删文件，幂等）。
import { execFileSync } from 'node:child_process'

const args = process.argv.slice(2)
let chunk = 0
const chunkIndex = args.indexOf('--chunk')
if (chunkIndex !== -1) {
  chunk = Number(args[chunkIndex + 1])
  args.splice(chunkIndex, 2)
}
const paths = args
if (paths.length === 0) {
  console.error('usage: node /tmp/orcinus-batch-rm.mjs [--chunk N] <pathspec...>')
  process.exit(1)
}
const list = execFileSync('git', ['ls-files', '-z', '--', ...paths], { encoding: 'utf8' })
  .split('\0')
  .filter(Boolean)
if (list.length === 0) {
  console.error('no files matched:', paths.join(' '))
  process.exit(1)
}
console.log(`total files: ${list.length}`)

function gate(label) {
  try {
    execFileSync('pnpm', ['typecheck'], { stdio: 'inherit' })
    execFileSync('pnpm', ['build:web'], { stdio: 'inherit' })
  } catch {
    console.error(`GATE FAILED (${label}). 修复报错引用后重跑本命令。`)
    process.exit(1)
  }
}

if (chunk > 0) {
  for (let i = 0; i < list.length; i += chunk) {
    const batch = list.slice(i, i + chunk)
    execFileSync('git', ['rm', '-q', '--', ...batch], { stdio: 'inherit' })
    console.log(`chunk ${Math.floor(i / chunk) + 1}: removed ${batch.length}`)
    gate(`chunk ${Math.floor(i / chunk) + 1}`)
  }
} else {
  execFileSync('git', ['rm', '-q', '--', ...list], { stdio: 'inherit' })
  gate('all')
}
console.log('ALL OK')
```

- [ ] **Step 6: 创建 i18n 候选键收集脚本** `/tmp/orcinus-domain-keys.sh`

```bash
#!/bin/bash
# Usage: bash /tmp/orcinus-domain-keys.sh <out-keys.txt> <被删文件或目录...>
# 收集形如 a.b.c 的键字面量（覆盖 auto.* / components.* / settings.* 等命名空间）
set -euo pipefail
out="$1"; shift
grep -rhoE "'[a-zA-Z][a-zA-Z0-9_-]*(\.[a-zA-Z0-9_-]+){2,}'" "$@" \
  | tr -d "'" | sort -u > "$out"
echo "candidate keys: $(wc -l < "$out" | tr -d ' ')"
```

- [ ] **Step 7: 创建 i18n 安全删键脚本** `/tmp/orcinus-i18n-prune.mjs`

```js
// Usage: node /tmp/orcinus-i18n-prune.mjs <keys.txt> [--apply]
// 规则：候选键存在于 catalog；且剩余源码（排除 src/renderer/src/i18n/**）无完整键字面量；
//       且无 `父路径.${` 形式的模板拼接；才从 7 个 catalog 删除。
import { existsSync, readFileSync, writeFileSync } from 'node:fs'
import { execFileSync } from 'node:child_process'

const keysFile = process.argv[2]
const APPLY = process.argv.includes('--apply')
const CATALOGS = [
  'src/renderer/src/i18n/locales/en.json',
  'src/renderer/src/i18n/locales/es.json',
  'src/renderer/src/i18n/locales/fr.json',
  'src/renderer/src/i18n/locales/ja.json',
  'src/renderer/src/i18n/locales/ko.json',
  'src/renderer/src/i18n/locales/zh.json',
  'src/renderer/src/i18n/en-runtime-required.json'
]

const catalogs = new Map()
for (const path of CATALOGS) catalogs.set(path, JSON.parse(readFileSync(path, 'utf8')))

function lookup(root, key) {
  let node = root
  for (const part of key.split('.')) {
    if (!node || typeof node !== 'object') return undefined
    node = node[part]
  }
  return node
}

function deleteKey(root, key) {
  const parts = key.split('.')
  let node = root
  for (const part of parts.slice(0, -1)) {
    if (!node || typeof node !== 'object') return false
    node = node[part]
  }
  const last = parts.at(-1)
  if (node && typeof node === 'object' && last in node) {
    delete node[last]
    return true
  }
  return false
}

function sourceRefs(pattern) {
  try {
    const out = execFileSync(
      'bash',
      ['-c', `grep -rlF --include='*.ts' --include='*.tsx' --include='*.html' -- ${JSON.stringify(pattern)} src/renderer/src src/shared src/preload src/bridge src/main src/types | grep -v 'src/renderer/src/i18n/' || true`],
      { encoding: 'utf8' }
    )
    return out.trim() ? out.trim().split('\n') : []
  } catch {
    return []
  }
}

const candidates = [...new Set(readFileSync(keysFile, 'utf8').split('\n').map((k) => k.trim()).filter(Boolean))]
const kept = { notInCatalog: [], referenced: [], dynamic: [], deleted: [] }

for (const key of candidates) {
  const inCatalog = [...catalogs.values()].some((c) => typeof lookup(c, key) === 'string')
  if (!inCatalog) { kept.notInCatalog.push(key); continue }
  if (sourceRefs(key).length > 0) { kept.referenced.push(`${key}  <- ${sourceRefs(key)[0]}`); continue }
  const parent = key.split('.').slice(0, -1).join('.')
  if (parent && sourceRefs(`${parent}.$`).length > 0) { kept.dynamic.push(key); continue }
  let removed = 0
  for (const catalog of catalogs.values()) if (deleteKey(catalog, key)) removed++
  if (removed > 0) kept.deleted.push(key)
}

if (APPLY) {
  for (const [path, catalog] of catalogs) writeFileSync(path, JSON.stringify(catalog, null, 2))
}
for (const [reason, list] of Object.entries(kept)) {
  console.log(`${reason}: ${list.length}`)
}
console.log('--- kept (referenced) sample ---')
console.log(kept.referenced.slice(0, 10).join('\n'))
console.log('--- kept (dynamic) sample ---')
console.log(kept.dynamic.slice(0, 10).join('\n'))
```

- [ ] **Step 8: 采集可达性基线并自检**

```bash
node /tmp/orcinus-reach.mjs --out /tmp/orcinus-reach-baseline.txt | head -1
wc -l /tmp/orcinus-reach-baseline.txt
# 期望：unreachable 数量为已知死树规模（rpc-contract 等 Phase 1 backlog 项），
# 若出现大量意外不可达（>1600 或包含终端/worktree 文件），说明脚本解析有误，先修脚本。
```

- [ ] **Step 9: 提交**

无仓库改动（工具在 /tmp，基线在 /tmp），本任务不提交。

---

## Task 1: 移除 Pet 桌宠

**Files（锚点，最终以 reach diff 为准）:**
- Delete: `src/renderer/src/components/pet/**`（约 22 文件）、`src/renderer/src/components/status-bar/PetStatusSegment.tsx` 及其测试、`src/preload/api/pet-api.ts`、`src/preload/api/pet-bridge.ts`
- Modify: `src/renderer/src/components/status-bar/StatusBarSurface.tsx`、`src/renderer/src/components/status-bar/use-status-bar-controller.ts`、`src/renderer/src/app-shell/AppRootSurfaces.tsx`、`src/renderer/src/store/slices/ui/ui-slice-surface-actions.ts`、`src/renderer/src/store/slices/ui/ui-slice-hydration-actions.ts`、`src/renderer/src/store/slices/ui/ui-slice-contract-preferences.ts`、`src/renderer/src/components/settings/ExperimentalPane.tsx`（pet 开关）、`src/shared/global-settings-types.ts`（`experimentalPet` 与旧名迁移键）、`src/shared/persisted-ui-state-types.ts`、`src/shared/rpc-contract/client-ui-params.ts`、`src/preload/index.ts`、`src/preload/api-types.ts`

**Interfaces:**
- Consumes: Task 0 的 4 个工具与基线
- Produces: Pet 域删除提交；后续任务复用的 SOP 验证

- [ ] **Step 1: 删前可达性快照**

```bash
node /tmp/orcinus-reach.mjs --out /tmp/reach-before-pet.txt | head -1
```

- [ ] **Step 2: 收集 i18n 候选键**

```bash
bash /tmp/orcinus-domain-keys.sh /tmp/keys-pet.txt src/renderer/src/components/pet src/renderer/src/components/status-bar/PetStatusSegment.tsx
```

- [ ] **Step 3: 删除 + 门禁**

```bash
node /tmp/orcinus-batch-rm.mjs src/renderer/src/components/pet src/renderer/src/components/status-bar/PetStatusSegment.tsx
# 若 GATE FAILED：按报错删除/修改引用方（多为上表 Modify 文件），然后重跑本命令
```

- [ ] **Step 4: 解耦共享引用（编译器驱动）**

```bash
pnpm typecheck 2>&1 | grep -E "error TS" | head -30
# 逐项处理：删除 pet lazy import / petEnabled 分支 / PetOverlay 分支 / pet store 调用 /
# settings 字段与迁移键 / preload pet 命名空间（index.ts、api-types.ts）
```

- [ ] **Step 5: 清理设置与实验开关**

在 `ExperimentalPane.tsx` 删除 pet 开关后，确认无残留：

```bash
grep -rn "experimentalPet\|petEnabled\|petVisible\|api.pet" src --include='*.ts' --include='*.tsx' | grep -v '\.test\.' || echo "CLEAN"
```

- [ ] **Step 6: i18n 清键**

```bash
node /tmp/orcinus-i18n-prune.mjs /tmp/keys-pet.txt          # 先看报告
node /tmp/orcinus-i18n-prune.mjs /tmp/keys-pet.txt --apply  # 确认后应用
git diff --stat src/renderer/src/i18n
```

- [ ] **Step 7: 守卫测试 + 可达性 diff**

```bash
pnpm test src/renderer/src/lazy-use-ref-ratchet.test.ts src/renderer/src/lazy-modal-mount-state.test.ts src/renderer/src/renderer-node-builtin-boundary.test.ts src/bridge src/renderer/src/i18n
node /tmp/orcinus-reach.mjs --out /tmp/reach-after-pet.txt | head -1
comm -13 /tmp/reach-before-pet.txt /tmp/reach-after-pet.txt   # 新增孤儿：逐个判定删除或记录
```

- [ ] **Step 8: 提交**

```bash
git add -A && git commit -m "chore: 移除 Pet 桌宠（含契约与测试）"
```

---

## Task 2: 移除引导/营销面（Contextual Tours + Feature Tips + Feature Wall + Setup Guide）

**Files（锚点，最终以 reach diff 为准）:**
- Delete: `src/renderer/src/components/contextual-tours/**`（约 25）、`src/renderer/src/components/feature-tips/**`（约 21）、`src/renderer/src/components/feature-wall/**`（约 90）、`src/renderer/src/components/setup-guide/**`（约 11）、`src/shared/feature-wall-setup-steps.ts`、`src/renderer/src/components/settings/SettingsSetupGuidePane.tsx`、`src/renderer/src/components/settings/settings-setup-guide-progress.ts`、`src/renderer/src/components/sidebar/SetupGuideSidebarEntry.tsx`、`src/renderer/src/components/settings/setup-guide*.ts(x)` 相关搜索条目
- Modify: `src/renderer/src/app-shell/use-onboarding-and-feature-tips.ts`（只删 feature tips 部分，Onboarding 保留）、`src/renderer/src/app-shell/AppRootSurfaces.tsx`、`src/renderer/src/app-shell/AppBackgroundServices.tsx`、`src/renderer/src/lazy-modal-mount-state.ts`（去掉 `'setup-guide' | 'feature-wall' | 'feature-tips'`）、`vite.config.ts`（删 `ORCA_FEATURE_WALL_ENABLED` define）、设置导航/搜索注册（`settings-navigation-*`、`settings-search*`、`experimental-search.ts`）、`src/renderer/src/lib/feature-education-telemetry.ts` 的 tours/tips/wall 调用点

**Interfaces:**
- Consumes: Task 0 工具
- Produces: 引导面删除提交；`lazy-modal-mount-state` 保留 `quick-open`/`worktree-palette`/`workspace-cleanup` 三项

- [ ] **Step 1: 删前快照**

```bash
node /tmp/orcinus-reach.mjs --out /tmp/reach-before-guides.txt | head -1
```

- [ ] **Step 2: 收集键**

```bash
bash /tmp/orcinus-domain-keys.sh /tmp/keys-guides.txt \
  src/renderer/src/components/contextual-tours src/renderer/src/components/feature-tips \
  src/renderer/src/components/feature-wall src/renderer/src/components/setup-guide \
  src/renderer/src/components/settings/SettingsSetupGuidePane.tsx \
  src/renderer/src/components/settings/settings-setup-guide-progress.ts \
  src/renderer/src/components/sidebar/SetupGuideSidebarEntry.tsx
```

- [ ] **Step 3: 删除 + 门禁**

```bash
node /tmp/orcinus-batch-rm.mjs src/renderer/src/components/contextual-tours src/renderer/src/components/feature-tips src/renderer/src/components/setup-guide
node /tmp/orcinus-batch-rm.mjs src/renderer/src/components/feature-wall src/shared/feature-wall-setup-steps.ts
# 若 GATE FAILED：处理报错引用后重跑
```

- [ ] **Step 4: 解耦共享引用**

```bash
pnpm typecheck 2>&1 | grep -E "error TS" | head -30
# 处理：use-onboarding-and-feature-tips 拆分；lazy-modal-mount-state 常量表；
# setup-guide 的设置/侧栏注册；AppRootSurfaces/AppBackgroundServices 挂载分支
```

- [ ] **Step 5: 清理 vite define 与 Onboarding 交叉引用**

```bash
# vite.config.ts 删除 define 段：
#   define: { ORCA_FEATURE_WALL_ENABLED: 'true' }
grep -rn "ORCA_FEATURE_WALL_ENABLED\|feature-wall-setup\|SetupGuide" src vite.config.ts --include='*.ts' --include='*.tsx' | grep -v '\.test\.' | grep -v 'Onboarding' || echo "CLEAN"
```

- [ ] **Step 6: i18n 清键**

```bash
node /tmp/orcinus-i18n-prune.mjs /tmp/keys-guides.txt
node /tmp/orcinus-i18n-prune.mjs /tmp/keys-guides.txt --apply
```

- [ ] **Step 7: 守卫测试 + 可达性 diff**

```bash
pnpm test src/renderer/src/lazy-use-ref-ratchet.test.ts src/renderer/src/lazy-modal-mount-state.test.ts src/renderer/src/renderer-node-builtin-boundary.test.ts src/bridge src/renderer/src/i18n
node /tmp/orcinus-reach.mjs --out /tmp/reach-after-guides.txt | head -1
comm -13 /tmp/reach-before-guides.txt /tmp/reach-after-guides.txt
```

- [ ] **Step 8: 提交**

```bash
git add -A && git commit -m "chore: 移除引导/营销面 Tours/Tips/FeatureWall/SetupGuide（含契约与测试）"
```

---

## Task 3: 移除 Dictation 语音听写

**Files（锚点，最终以 reach diff 为准）:**
- Delete: `src/renderer/src/components/dictation/**`（约 21）、`src/preload/api/speech-api.ts`、`src/preload/api/speech-bridge.ts`、听写专用 hooks（`src/renderer/src/hooks/use-hold-dictation-gesture*`、dictation 相关 store/工具文件），以及仅听写使用的设置项（`OpenAiTranscriptionKeyDialog` 等，实施时判定）
- Modify: `src/renderer/src/app-shell/AppRootSurfaces.tsx`（`DictationController` 懒加载、`shouldMountDictationController`、`dictationState`/`voiceEnabled` 选择器）、`src/shared/keybindings/definitions-core-1.ts` 与 `src/shared/keybindings/types.ts`（`voice.dictation`）、`src/shared/feature-interaction-catalog.ts` 与 `feature-interaction-categories.ts`（`voice-dictation`）、voice 设置 pane 中听写分区、`src/preload/index.ts`、`src/preload/api-types.ts`

**Interfaces:**
- Consumes: Task 0 工具
- Produces: Dictation 删除提交；`window.api.speech` 命名空间移除；voice 设置 pane 保留非听写部分

- [ ] **Step 1: 删前快照**

```bash
node /tmp/orcinus-reach.mjs --out /tmp/reach-before-dictation.txt | head -1
grep -rn "dictationState\|voiceEnabled\|speech\." src/renderer/src/components/settings/voice-pane-search.ts src/renderer/src/components/settings/VoicePane.tsx 2>/dev/null | head -20
# 上一条用于确认 voice pane 中哪些项仅服务于听写；不确定的项保守保留
```

- [ ] **Step 2: 收集键**

```bash
bash /tmp/orcinus-domain-keys.sh /tmp/keys-dictation.txt src/renderer/src/components/dictation
```

- [ ] **Step 3: 删除 + 门禁**

```bash
node /tmp/orcinus-batch-rm.mjs src/renderer/src/components/dictation src/preload/api/speech-api.ts src/preload/api/speech-bridge.ts
# 若 GATE FAILED：处理报错引用后重跑
```

- [ ] **Step 4: 解耦共享引用**

```bash
pnpm typecheck 2>&1 | grep -E "error TS" | head -30
# 处理：AppRootSurfaces 挂载与选择器；keybindings 定义与类型联合；feature-interaction 条目；preload 命名空间
```

- [ ] **Step 5: 清理检查**

```bash
grep -rn "DictationController\|api.speech\|voice.dictation\|voice-dictation\|shouldMountDictation" src --include='*.ts' --include='*.tsx' | grep -v '\.test\.' || echo "CLEAN"
```

- [ ] **Step 6: i18n 清键**

```bash
node /tmp/orcinus-i18n-prune.mjs /tmp/keys-dictation.txt
node /tmp/orcinus-i18n-prune.mjs /tmp/keys-dictation.txt --apply
```

- [ ] **Step 7: 守卫测试 + 可达性 diff**

```bash
pnpm test src/renderer/src/lazy-use-ref-ratchet.test.ts src/renderer/src/lazy-modal-mount-state.test.ts src/renderer/src/renderer-node-builtin-boundary.test.ts src/bridge src/renderer/src/i18n
node /tmp/orcinus-reach.mjs --out /tmp/reach-after-dictation.txt | head -1
comm -13 /tmp/reach-before-dictation.txt /tmp/reach-after-dictation.txt
```

- [ ] **Step 8: 提交**

```bash
git add -A && git commit -m "chore: 移除 Dictation 语音听写（含契约与测试）"
```

---

## Task 4: 移除 Emulator Pane 移动端仿真

**Files（锚点，最终以 reach diff 为准）:**
- Delete: `src/renderer/src/components/emulator-pane/**`（约 54）、`src/renderer/src/components/settings/MobileEmulatorSettingsPane.tsx`、`MobileEmulatorAgentControlRow.tsx`、`MobileEmulatorAvailabilityDetails.tsx`、`src/renderer/src/components/settings/mobile-emulator-search.ts`、`src/preload/api/emulator-api.ts`、`src/preload/api/emulator-bridge.ts`、`src/bridge/mock/emulator-api.ts`
- Modify: `src/renderer/src/components/settings/settings-interface-primary-section-renderers.tsx`（删除 MobileEmulatorSettingsPane 渲染分支）、`src/bridge/create-api.ts`（移除 `emulator` 命名空间注册）、`src/preload/index.ts`、`src/preload/api-types.ts`、`src/preload/api/pty-api.ts`（emulator 专用方法）、store/ui 切片中 emulator 引用、`src/shared/default-global-settings.ts`、`src/shared/constants.ts`、`src/shared/keybindings/definitions-core-2.ts` 与 `types.ts`（emulator 键位）、`src/shared/feature-interaction-catalog.ts` 与 `feature-interaction-categories.ts`（`mobile-emulator-agent-setup`）

**Interfaces:**
- Consumes: Task 0 工具
- Produces: Emulator 删除提交；`window.api.emulator` 命名空间与 pty 契约中 emulator 方法移除

- [ ] **Step 1: 删前快照**

```bash
node /tmp/orcinus-reach.mjs --out /tmp/reach-before-emulator.txt | head -1
```

- [ ] **Step 2: 收集键**

```bash
bash /tmp/orcinus-domain-keys.sh /tmp/keys-emulator.txt src/renderer/src/components/emulator-pane src/renderer/src/components/settings/MobileEmulatorSettingsPane.tsx src/renderer/src/components/settings/mobile-emulator-search.ts
```

- [ ] **Step 3: 删除 + 门禁**

```bash
node /tmp/orcinus-batch-rm.mjs src/renderer/src/components/emulator-pane src/preload/api/emulator-api.ts src/preload/api/emulator-bridge.ts src/bridge/mock/emulator-api.ts
# 若 GATE FAILED：处理报错引用后重跑
```

- [ ] **Step 4: 解耦共享引用（含 pty 契约与 shared 默认值）**

```bash
pnpm typecheck 2>&1 | grep -E "error TS" | head -30
# 处理：settings 渲染分支；bridge/create-api 注册；pty-api emulator 方法；shared 默认设置/常量/键位/feature-interaction
```

- [ ] **Step 5: 清理检查**

```bash
grep -rn "EmulatorPane\|api.emulator\|mobile-emulator\|mobileEmulator" src --include='*.ts' --include='*.tsx' | grep -v '\.test\.' || echo "CLEAN"
```

- [ ] **Step 6: i18n 清键**

```bash
node /tmp/orcinus-i18n-prune.mjs /tmp/keys-emulator.txt
node /tmp/orcinus-i18n-prune.mjs /tmp/keys-emulator.txt --apply
```

- [ ] **Step 7: 守卫测试 + 可达性 diff**

```bash
pnpm test src/renderer/src/lazy-use-ref-ratchet.test.ts src/renderer/src/lazy-modal-mount-state.test.ts src/renderer/src/renderer-node-builtin-boundary.test.ts src/bridge src/renderer/src/i18n
node /tmp/orcinus-reach.mjs --out /tmp/reach-after-emulator.txt | head -1
comm -13 /tmp/reach-before-emulator.txt /tmp/reach-after-emulator.txt
```

- [ ] **Step 8: 提交**

```bash
git add -A && git commit -m "chore: 移除 Emulator Pane 移动端仿真（含契约与测试）"
```

---

## Task 5: 移除 Activity / Dashboard / Kanban / Agent Map / Popout

**Files（锚点，最终以 reach diff 为准）:**
- Delete: `src/renderer/src/components/activity/**`（约 78）、`src/renderer/src/components/dashboard/**`（约 61）、`src/renderer/src/components/dashboard-popout/**`（约 123）、`src/renderer/popout.html`、`src/renderer/src/popout.tsx`、sidebar Kanban 全套（`src/renderer/src/components/sidebar/WorkspaceKanban*`、`workspace-kanban-*`，约 50）、`src/renderer/src/components/sidebar/AgentDashboardSidebarEntry.tsx`、`src/renderer/src/components/settings/AgentDashboardExperimentalSetting.tsx`、`src/renderer/src/components/settings/experimental-search.ts` 中大屏相关条目、activity/dashboard/kanban 相关 store slice 与 hook、相关测试
- Modify: `src/renderer/src/app-shell/AppWorkspaceShell.tsx`（activity 分支）、`src/renderer/src/app-shell/TitlebarMainStrip.tsx`、`src/renderer/src/app-shell/use-app-chrome-layout.ts`、`src/shared/ui-chrome-types.ts`（`'activity'` 枚举项）、`src/renderer/src/components/sidebar/index.tsx`（`ActivityThreadCollapseContext`、Kanban drawer 引用）、`src/renderer/src/components/sidebar/SidebarToolbar.tsx`（Kanban 按钮）、worktree 卡片同步相关引用（`use-workspace-board-task-status-sync` 等，实施时判定）

**Interfaces:**
- Consumes: Task 0 工具
- Produces: Activity 域删除提交；`TopLevelView` 不再含 `'activity'`；`popout.html` 入口移除

- [ ] **Step 1: 删前快照**

```bash
node /tmp/orcinus-reach.mjs --out /tmp/reach-before-activity.txt | head -1
```

- [ ] **Step 2: 收集键**

```bash
bash /tmp/orcinus-domain-keys.sh /tmp/keys-activity.txt \
  src/renderer/src/components/activity src/renderer/src/components/dashboard \
  src/renderer/src/components/dashboard-popout src/renderer/src/components/sidebar/WorkspaceKanbanDrawer.tsx \
  src/renderer/src/components/sidebar/SidebarToolbar.tsx src/renderer/src/components/sidebar/AgentDashboardSidebarEntry.tsx
```

- [ ] **Step 3: 删除 + 门禁（整域一次删除；域内互引由后续 typecheck 引导解耦）**

```bash
node /tmp/orcinus-batch-rm.mjs src/renderer/src/components/activity src/renderer/src/components/dashboard src/renderer/src/components/dashboard-popout
node /tmp/orcinus-batch-rm.mjs 'src/renderer/src/components/sidebar/WorkspaceKanban*' 'src/renderer/src/components/sidebar/workspace-kanban-*' src/renderer/src/components/sidebar/AgentDashboardSidebarEntry.tsx src/renderer/popout.html src/renderer/src/popout.tsx
# 若 GATE FAILED：处理报错引用后重跑
```

- [ ] **Step 4: 解耦共享引用**

```bash
pnpm typecheck 2>&1 | grep -E "error TS" | head -30
# 处理：AppWorkspaceShell/TitlebarMainStrip/use-app-chrome-layout；ui-chrome-types 枚举；
# sidebar/index.tsx（ActivityThreadCollapseContext、Kanban drawer）；SidebarToolbar Kanban 按钮；
# 设置 pane 与搜索条目；store 注册
```

- [ ] **Step 5: 清理检查**

```bash
grep -rn "components/activity\|components/dashboard\|AgentMap\|WorkspaceKanban\|'activity'" src/renderer/src src/shared --include='*.ts' --include='*.tsx' | grep -v '\.test\.' | grep -viE "agent-status|activity-at|terminal-activity|worktrees-activity" || echo "CLEAN"
# 若仍有命中，逐个人工判定（保留域的同名概念不要误删）
```

- [ ] **Step 6: i18n 清键**

```bash
node /tmp/orcinus-i18n-prune.mjs /tmp/keys-activity.txt
node /tmp/orcinus-i18n-prune.mjs /tmp/keys-activity.txt --apply
```

- [ ] **Step 7: 守卫测试 + 可达性 diff**

```bash
pnpm test src/renderer/src/lazy-use-ref-ratchet.test.ts src/renderer/src/lazy-modal-mount-state.test.ts src/renderer/src/renderer-node-builtin-boundary.test.ts src/bridge src/renderer/src/i18n
node /tmp/orcinus-reach.mjs --out /tmp/reach-after-activity.txt | head -1
comm -13 /tmp/reach-before-activity.txt /tmp/reach-after-activity.txt
```

- [ ] **Step 8: 提交**

```bash
git add -A && git commit -m "chore: 移除 Activity/Dashboard/Kanban/Popout（含契约与测试）"
```

---

## Task 6: 移除 Native Chat

**Files（锚点，最终以 reach diff 为准）:**
- Delete: `src/renderer/src/components/native-chat/**`（327）、`src/renderer/src/runtime/structured-agent-session-*.ts`、`src/renderer/src/runtime/web-agent-session-handoff.ts`、`src/shared/native-chat-*.ts`、`src/renderer/src/lib/native-chat-telemetry.ts`、`src/preload/api/native-chat-api.ts`、`src/preload/api/native-chat-bridge.ts`、`src/bridge/mock/` 中 native chat 域、`src/renderer/src/web/preload-api/web-native-chat-api.ts`、`src/renderer/src/components/settings/NativeChatExperimentalSetting.tsx` 与 `native-chat-experimental-search-entry.ts`、`src/renderer/src/i18n/native-chat-locales.test.ts`、终端 portal 互嵌文件（`src/renderer/src/components/terminal-pane/TerminalPaneNativeChatPortal.tsx`、`native-chat-covered-pane.ts`、`native-chat-leaf-title-agent.ts`、`use-terminal-pane-chat-state.ts`、`StructuredAgentSessionTerminalReturnButton.tsx` 等，实施时按可达性判定）
- Modify: `src/preload/index.ts`、`src/preload/api-types.ts`、`src/preload/api/runtime-bridge.ts`（`onNativeChatLaunchDraftResolved`）、`src/preload/api/runtime-api.ts`、`src/renderer/src/components/terminal-pane/TerminalPaneRuntimePortals.tsx`、`TerminalPaneSurface.tsx`、`use-terminal-pane-projection.ts`、`use-terminal-pane-global-effects.ts`、`terminal-pane-paste-listeners.ts`、`TerminalPaneOverlayLayer.tsx`、`TerminalContextMenu.tsx`、`use-terminal-window-wake-recovery.ts`、`pty-connection-types.ts`、`terminal-pane-host-state.ts`、`pty-connection/agent-idle-working-handlers.ts`、`src/renderer/src/components/tab-bar/QuickLaunchButton.tsx`、`TabBarCreateEntry.tsx`、设置开关与实验搜索、`src/renderer/src/web/web-preload-api.ts`

**Interfaces:**
- Consumes: Task 0 工具；终端域的 portal 解耦必须先于删除
- Produces: Native Chat 删除提交；`window.api.nativeChat` 移除；终端不再渲染 native chat portal

- [ ] **Step 1: 删前快照 + 互嵌点清点**

```bash
node /tmp/orcinus-reach.mjs --out /tmp/reach-before-nativechat.txt | head -1
grep -rn "native-chat\|nativeChat\|NativeChat" src/renderer/src/components/terminal-pane src/renderer/src/components/tab-bar src/renderer/src/components/tab-group --include='*.ts' --include='*.tsx' | grep -v '\.test\.' | wc -l
```

- [ ] **Step 2: 收集键**

```bash
bash /tmp/orcinus-domain-keys.sh /tmp/keys-nativechat.txt src/renderer/src/components/native-chat
```

- [ ] **Step 3: 先解耦终端 portal（删除终端侧 native-chat 专属文件 + 修改宿主文件）**

```bash
# 1) 修改终端宿主文件（见 Files Modify 列表），移除 portal 挂载、覆盖状态、快捷键、
#    上下文菜单项、结构化会话返回按钮等 native chat 引用；每改一组跑 pnpm typecheck
# 2) 删除终端侧 native-chat 专属文件（只被 native chat 使用）：
node /tmp/orcinus-batch-rm.mjs \
  src/renderer/src/components/terminal-pane/TerminalPaneNativeChatPortal.tsx \
  src/renderer/src/components/terminal-pane/native-chat-covered-pane.ts \
  src/renderer/src/components/terminal-pane/native-chat-leaf-title-agent.ts \
  src/renderer/src/components/terminal-pane/use-terminal-pane-chat-state.ts \
  src/renderer/src/components/terminal-pane/StructuredAgentSessionTerminalReturnButton.tsx
# 若 GATE FAILED：处理报错引用后重跑
pnpm typecheck
```

- [ ] **Step 4: 删除域内文件 + 门禁**

```bash
node /tmp/orcinus-batch-rm.mjs src/renderer/src/components/native-chat
node /tmp/orcinus-batch-rm.mjs src/renderer/src/runtime/structured-agent-session-*.ts src/renderer/src/runtime/web-agent-session-handoff.ts 'src/shared/native-chat-*.ts' src/renderer/src/lib/native-chat-telemetry.ts src/preload/api/native-chat-api.ts src/preload/api/native-chat-bridge.ts src/renderer/src/web/preload-api/web-native-chat-api.ts src/renderer/src/components/settings/NativeChatExperimentalSetting.tsx src/renderer/src/components/settings/native-chat-experimental-search-entry.ts src/renderer/src/i18n/native-chat-locales.test.ts
# 若 GATE FAILED：处理报错引用后重跑
```

- [ ] **Step 5: 解耦剩余共享引用**

```bash
pnpm typecheck 2>&1 | grep -E "error TS" | head -30
# 处理：preload runtime-bridge/runtime-api；web-preload-api；设置实验开关；遗留 terminal/tab 引用
```

- [ ] **Step 6: 清理检查**

```bash
grep -rn "api.nativeChat\|nativeChat\|native-chat" src --include='*.ts' --include='*.tsx' | grep -v '\.test\.' | grep -viE "mobile|relay|rpc-contract" || echo "CLEAN"
```

- [ ] **Step 7: i18n 清键**

```bash
node /tmp/orcinus-i18n-prune.mjs /tmp/keys-nativechat.txt
node /tmp/orcinus-i18n-prune.mjs /tmp/keys-nativechat.txt --apply
```

- [ ] **Step 8: 守卫测试 + 可达性 diff**

```bash
pnpm test src/renderer/src/lazy-use-ref-ratchet.test.ts src/renderer/src/lazy-modal-mount-state.test.ts src/renderer/src/renderer-node-builtin-boundary.test.ts src/bridge src/renderer/src/i18n
node /tmp/orcinus-reach.mjs --out /tmp/reach-after-nativechat.txt | head -1
comm -13 /tmp/reach-before-nativechat.txt /tmp/reach-after-nativechat.txt
```

- [ ] **Step 9: 提交**

```bash
git add -A && git commit -m "chore: 移除 Native Chat（含契约与测试）"
```

---

## Task 7: 移除 Telemetry 全链（含崩溃上报与 Feedback）

**Files（锚点，最终以 reach diff 为准）:**
- Delete: `src/preload/api/telemetry-api.ts`、`src/renderer/src/lib/telemetry.ts`、`src/shared/telemetry-*.ts`（events/consent/registry/classification/property-schemas/feature-education/onboarding/repository/app/daemon 等约 15）、`src/renderer/src/components/TelemetryFirstLaunchSurface.tsx`、`src/renderer/src/components/sidebar/SidebarFeedbackDialog.tsx`、分别的 feedback preload/bridge/mock 文件、`src/shared/crash-reporting.ts`、renderer crash diagnostics 模块、`src/renderer/src/lib/feature-education-telemetry.ts`（此时其 tours/tips/wall/setup-guide 调用方已删，只剩 terminal 等将被本步清掉）、`src/bridge/mock/crash-reports-api.ts` 及 `settings-api.ts`/`create-api.ts` 中 telemetry 相关、`src/renderer/src/components/PrivacyPane`/`PrivacyDiagnosticsSection`/`privacy-search` 中遥测项
- Modify: `src/preload/index.ts`、`src/preload/api-types.ts`、`src/renderer/src/main.tsx`（crash diagnostics 安装）、`src/renderer/src/components/FirstLaunchBanner.tsx`、`src/shared/global-settings-types.ts`（`telemetry` 字段）、`src/shared/persisted-state-types.ts`、`src/shared/onboarding-state-types.ts`（如有 telemetry 关联）、约 80 个保留域文件的 `track()` 调用与 import、29 处 breadcrumb 调用、`src/main` 中 telemetry/PostHog 模块（休眠 Electron 代码，一并删）

**Interfaces:**
- Consumes: Tasks 1–6 已带走各自域内的埋点
- Produces: Telemetry 删除提交；`window.api.telemetry*` 与 `window.api.crashReports`/feedback 契约移除；仓库内无任何 `track(`/breadcrumb 调用

- [ ] **Step 1: 删前快照与埋点清点**

```bash
node /tmp/orcinus-reach.mjs --out /tmp/reach-before-telemetry.txt | head -1
grep -rl "lib/telemetry\|track(" src/renderer/src src/shared src/preload src/bridge --include='*.ts' --include='*.tsx' | grep -v '\.test\.' | wc -l
grep -rl "recordRendererCrashBreadcrumb\|installRendererCrashDiagnostics" src/renderer/src --include='*.ts' --include='*.tsx' | grep -v '\.test\.' | wc -l
```

- [ ] **Step 2: 收集键**

```bash
bash /tmp/orcinus-domain-keys.sh /tmp/keys-telemetry.txt \
  src/renderer/src/components/TelemetryFirstLaunchSurface.tsx \
  src/renderer/src/components/sidebar/SidebarFeedbackDialog.tsx
```

- [ ] **Step 3: 删除 telemetry 核心层（契约、库、shared schemas）**

```bash
node /tmp/orcinus-batch-rm.mjs src/preload/api/telemetry-api.ts src/renderer/src/lib/telemetry.ts 'src/shared/telemetry-*.ts' src/renderer/src/components/TelemetryFirstLaunchSurface.tsx
# 若 GATE FAILED：处理报错引用后重跑（这一批会暴露全部埋点调用点）
```

- [ ] **Step 4: 逐批清埋点（编译器驱动，约 80 文件）**

```bash
pnpm typecheck 2>&1 | grep -E "error TS" | head -40
# 规则：删除 import { track } 与 track(...) 调用；若调用所在函数仅为此存在，一并简化；
# 若发现某文件只被 telemetry 链引用，交给 reach diff 判定后续删除
# 每修完一批（≤20 文件）跑：pnpm typecheck && pnpm build:web
```

- [ ] **Step 5: 删除崩溃上报与 breadcrumb**

```bash
node /tmp/orcinus-batch-rm.mjs src/shared/crash-reporting.ts src/bridge/mock/crash-reports-api.ts
pnpm typecheck 2>&1 | grep -E "error TS" | head -30
# 删除 main.tsx 等处的 installRendererCrashDiagnostics/recordRendererCrashBreadcrumb 调用；
# 删除 renderer crash diagnostics 模块与 src/main 中 telemetry/PostHog 模块
```

- [ ] **Step 6: 删除 Feedback 与 Privacy 遥测项**

```bash
node /tmp/orcinus-batch-rm.mjs src/renderer/src/components/sidebar/SidebarFeedbackDialog.tsx
pnpm typecheck 2>&1 | grep -E "error TS" | head -20
# 删除 feedback preload/bridge/mock、PrivacyPane/PrivacyDiagnosticsSection/privacy-search 中遥测分区、
# shared 设置类型中 telemetry 字段、FirstLaunchBanner 遥测部分
```

- [ ] **Step 7: 清理检查**

```bash
grep -rn "api.telemetry\|telemetryTrack\|setOptIn\|getConsentState\|crashReports\|recordRendererCrashBreadcrumb" src --include='*.ts' --include='*.tsx' | grep -v '\.test\.' || echo "CLEAN"
grep -rn "privacy-search\|PrivacyPane" src --include='*.ts' --include='*.tsx' | grep -v '\.test\.' | head
```

- [ ] **Step 8: i18n 清键**

```bash
node /tmp/orcinus-i18n-prune.mjs /tmp/keys-telemetry.txt
node /tmp/orcinus-i18n-prune.mjs /tmp/keys-telemetry.txt --apply
```

- [ ] **Step 9: 守卫测试 + 可达性 diff**

```bash
pnpm test src/renderer/src/lazy-use-ref-ratchet.test.ts src/renderer/src/lazy-modal-mount-state.test.ts src/renderer/src/renderer-node-builtin-boundary.test.ts src/bridge src/renderer/src/i18n
node /tmp/orcinus-reach.mjs --out /tmp/reach-after-telemetry.txt | head -1
comm -13 /tmp/reach-before-telemetry.txt /tmp/reach-after-telemetry.txt
```

- [ ] **Step 10: 提交**

```bash
git add -A && git commit -m "chore: 移除 Telemetry 全链与崩溃上报/Feedback（含契约与测试）"
```

---

## Task 8: 全局收尾（兜底扫描、依赖、ratchet、文档、人工冒烟）

**Files:**
- Create: `docs/phase1-feature-trim-record.md`
- Modify: `docs/superpowers/specs/2026-09-14-ade-design.md`
- Modify: `src/shared/child-process/__fixtures__/*-allowlist.txt`、`src/shared/child-process/child-process-import-boundary.test.ts`、其他 ratchet 测试 pin（若失败）
- Modify: `package.json`（仅删被删域独占的依赖）

**Interfaces:**
- Consumes: Tasks 1–7 全部提交；Task 0 的 `/tmp/orcinus-reach-baseline.txt` 与 `/tmp/orcinus-test-baseline-failures.txt`
- Produces: 收尾提交；更新后的 spec 与删减记录

- [ ] **Step 1: 全局可达性兜底扫描**

```bash
node /tmp/orcinus-reach.mjs --out /tmp/reach-final.txt | head -1
comm -13 /tmp/orcinus-reach-baseline.txt /tmp/reach-final.txt > /tmp/reach-new-orphans.txt
wc -l /tmp/reach-new-orphans.txt
# 逐个判定：仅剩测试引用或自身引用 → git rm；仍被 live 引用或属已知保留死树 → 追加到
# docs/phase1-feature-trim-record.md 的「残留」表并写明原因
```

- [ ] **Step 2: i18n 残留核查**

```bash
pnpm test src/renderer/src/i18n
# 若 zh/ja/ko technical-literal 或 semantic 测试断言了已删域的键：删除对应断言（仅限已删域）
# 若 runtime-required-catalog 失败：同步 en-runtime-required.json 中被误删/应保留的键
```

- [ ] **Step 3: 依赖清理（逐项 grep，只删独占项）**

```bash
for dep in @streamparser/json tldts react-grab html-to-image @dnd-kit/core @dnd-kit/sortable @tanstack/react-virtual @sanity/diff-match-patch emoji-picker-react emojibase-data react-colorful cmdk; do
  hits=$(grep -rl "$dep" src --include='*.ts' --include='*.tsx' | grep -v '\.test\.' | wc -l | tr -d ' ')
  echo "$dep -> $hits"
done
# 仅当 0 命中时从 package.json 移除并跑 pnpm install --lockfile-only 更新锁文件；
# 有命中则保留并在删减记录中列出保留原因
pnpm typecheck && pnpm build:web
```

- [ ] **Step 4: ratchet 重基线**

```bash
pnpm test src/shared/child-process src/renderer/src/lazy-use-ref-ratchet.test.ts src/renderer/src/lazy-modal-mount-state.test.ts src/renderer/src/renderer-node-builtin-boundary.test.ts
# 按失败信息处理：删除已删文件的 allowlist 行、更新 pin 计数（仅当失败由本任务删除引起；
# 既有 stale 问题保持原状并在记录中注明）
```

- [ ] **Step 5: 全量测试对比基线**

```bash
pnpm test 2>&1 | tee /tmp/orcinus-test-final.log; echo "exit=${PIPESTATUS[0]}"
grep -E "^ *(Test Files|Tests) " /tmp/orcinus-test-final.log | tail -4
grep -E "FAIL " /tmp/orcinus-test-final.log | sed 's/.*FAIL //' | sort -u > /tmp/orcinus-test-final-failures.txt
comm -13 /tmp/orcinus-test-baseline-failures.txt /tmp/orcinus-test-final-failures.txt
# 期望：新增失败为空（既有失败允许保留）
```

- [ ] **Step 6: 人工冒烟（由人执行）**

```bash
pnpm dev
```

检查清单：应用启动无报错；终端可开 tab 并执行命令；侧栏 worktree 列表可切换；设置页各分组可达；以下入口不存在：Pet 状态栏项、引导/功能墙弹窗、听写、移动仿真设置、Activity/看板入口、原生聊天入口、遥测/隐私遥测项、反馈入口；浏览器 tab 与 source control 面板仍可用。

- [ ] **Step 7: 更新设计规格**

修改 `docs/superpowers/specs/2026-09-14-ade-design.md`：

1. §2.1 C 行：删除「原生聊天界面」字样（保留 CLI 启动与 resume、agent 状态/通知/未读、automations、AI Vault）
2. §2.1 H 行：删除「崩溃上报/遥测」「语音」字样（保留多窗口、托盘、自动更新、全局快捷键、computer-use、通知）
3. §2.2 删除清单追加：Pet、Contextual Tours、Feature Tips、Feature Wall、Setup Guide、Dictation、Emulator Pane、Activity/Dashboard（含 Kanban/Agent Map/Popout）、Native Chat、Telemetry（含崩溃上报与 Feedback），并指向 `docs/phase1-feature-trim-record.md`
4. §6.5 追加被删 UI 面（同第 3 条清单）
5. Phase 2 交付行：删除「native chat」字样；Phase 4 交付行：删除「崩溃上报/遥测」
6. §10 风险清单：删除第 6 条中「语音」字样（computer-use 保留）

- [ ] **Step 8: 编写删减记录**

创建 `docs/phase1-feature-trim-record.md`，沿 `docs/phase0-dead-code-inventory.md` 格式，必须包含：

- 分支与各域提交哈希（`git log --oneline phase1-trim-features ^main`）
- 方法与证据链（可达性脚本行为、工具路径、判定规则）
- 每域「删除文件数 + 批次 + 门禁结果」表
- i18n 键删除统计（各 catalog 删除数、kept 原因统计）
- 依赖清理结果（删除项与保留项及原因）
- ratchet 重基线结果
- 残留表（未删的孤儿与原因）
- **推翻 Phase 0 判定的说明**：emulator-pane 与 MobileEmulatorSettingsPane 从「保留」改为「删除」的决策记录
- `pnpm test` 基线对比结果（既有失败 vs 新增失败）

- [ ] **Step 9: 收尾提交**

```bash
git add -A && git commit -m "chore: 功能删减收尾（全局扫描、依赖与 ratchet、spec 同步、删减记录）"
git log --oneline main..phase1-trim-features
# 期望：9 个提交（spec + Tasks 1-7 + 收尾）
```

---

## Self-Review

**Spec coverage:**

| Spec 章节 | 覆盖任务 |
|---|---|
| §2.1 Pet | Task 1 |
| §2.2 引导/营销面 | Task 2 |
| §2.3 Dictation | Task 3 |
| §2.4 Emulator（含推翻判定记录） | Task 4、Task 8 Step 8 |
| §2.5 Activity/Dashboard | Task 5 |
| §2.6 Native Chat | Task 6 |
| §2.7 Telemetry 全链 | Task 7 |
| §2.8 全局收尾 | Task 8 |
| §3 顺序与提交策略 | Tasks 1–8、Task 8 Step 9 |
| §4 SOP | Task 0 工具 + 各任务步骤 |
| §5 验收门禁 | 各任务 Step 7/8 + Task 8 Steps 4–6 |
| §6 风险缓解 | Task 0 基线、各域独立提交、Task 8 对比 |

**Placeholder scan:** 无 TBD/TODO；所有命令与脚本为完整可执行内容；「实施时判定」处均给出了判定命令与规则，不依赖隐含知识。

**Type consistency:** 工具名 `/tmp/orcinus-reach.mjs`、`/tmp/orcinus-batch-rm.mjs`、`/tmp/orcinus-domain-keys.sh`、`/tmp/orcinus-i18n-prune.mjs` 在 Task 0 定义、Tasks 1–8 引用一致；提交消息格式与 spec §4 一致；分支名 `phase1-trim-features` 一致。
