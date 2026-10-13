# Orcinus 功能删减实施计划(Phase 3B 瘦身:聚焦本地编码)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 从 Orcinus 彻底删除 7 个功能域(无后端死面、Web 远程客户端、GitHub 全链、Automations、Computer-use、Onboarding、Artifacts)+ AI Vault 死子面与杂项清理,含测试、mock bridge 域、preload 契约、Rust crate/命令、i18n 键与独占依赖;删除后应用可构建、可启动、保留功能(终端/worktree/编辑器/agent/插件/AI Vault)可用。

**Architecture:** 按风险与依赖排序 8 步执行;每域先做生产可达性分析确定「独占文件」与「共享引用点」,整域 `git rm` 后跑冷门禁(删除 `tsconfig.tsbuildinfo` 后 typecheck + build:web),编译器驱动解耦共享文件;Rust 域另跑 cargo check/test + bindings 重生成 + parity 测试;收尾做 reach 兜底、i18n 全量清键、依赖 grep、ratchet 重基线、全量测试对比与文档。

**Tech Stack:** TypeScript 7(`tsc --noEmit`)、Vite(rolldown-vite)、React 19、Vitest 4、pnpm、git;Rust 2021(cargo workspace `src-tauri/`,tauri-specta bindings)。

**Spec:** `docs/superpowers/specs/2026-10-13-local-focus-trim-design.md`

## Global Constraints

- 分支 `phase3b-trim-local-focus`(已创建,spec 提交 `639e6de`)。全程不切分支、不 merge、不 push。
- TS 门禁(每批删除后):`rm -f tsconfig.tsbuildinfo && pnpm typecheck && pnpm build:web` 必须 exit 0。**必须冷检**(Phase 1 教训:增量缓存曾掩盖 10 个错误)。
- Rust 门禁(动 Rust 的任务):`(cd src-tauri && cargo check)` 必须 exit 0;删 crate 后跑受影响 crate 测试;命令增删后 `(cd src-tauri && cargo run -p ade-bridge --bin export-bindings)` 重生成 bindings 并 `git add src/bridge/real/generated/tauri-bindings.ts`(仓库根),parity 守卫 `pnpm test src/bridge` 必须绿。
- 删除一律 `git rm -q --`(工具已封装),禁止裸 `rm`(唯一例外:Task 8 的 `spikes/cef-embed/target/` 若未被 git 追踪,用本地 `rm -rf`)。
- 删除节奏:域目录内部互引,默认整域一次 `git rm` + 一次门禁(`orcinus-batch-rm.mjs` 默认行为);跨域混合批用 `--chunk 20`。门禁失败按报错修引用后重跑(幂等)。
- 删除 reach 新增孤儿前必须 grep 生产+测试导入者(`grep -rn <module> src --include='*.ts' --include='*.tsx'`),仅「无生产/测试导入者」才删。
- 每域收尾守卫测试:`pnpm test src/renderer/src/lazy-use-ref-ratchet.test.ts src/renderer/src/lazy-modal-mount-state.test.ts src/renderer/src/renderer-node-builtin-boundary.test.ts src/bridge src/renderer/src/i18n`。
- `pnpm test` 基线存在既有失败。Task 0 重采基线,Task 8 对比:**不得新增失败**;collection 错误数只允许减少(删掉既有失败 collection 文件)不允许新增。
- **不得误伤**:终端/PTY 本地部分、worktree/项目模型、agent hooks/状态(`ade-hooks`)、编辑器 + diff 批注、git 基础(source control/commit/sparse)、Cmd+J、通知、会话持久化(尤其 `agent_sessions.rs` 与 Phase 2A `providerSession` 链)、插件系统(`ade-plugins`)、AI Vault 核心(扫描/列表/resume/续聊/tab 标题)、`shared/pairing.ts` 本体(`parsePairingCode` 被活的 `ephemeral-vm-recipes.ts` 引用)、ephemeral-vm composer 运行目标 UI(Phase 0 决策)、Landing、浮动终端、workspace 清理。
- 一次性工具脚本放 `/tmp`,不进仓库;不新增产品代码(守卫断言与文档除外)。
- 每域一个提交,消息格式 `chore: 移除 <域>(含契约与测试)`;Task 8 收尾单独提交;文档提交单独。
- 人工 GUI 冒烟合并为 Task 8 一次执行(自动门禁逐域 + 收尾一次人工冒烟,沿 Phase 1)。

## 文件结构

**一次性工具(Task 0 创建,均不进仓库,沿 Phase 1 验证过的版本)**

- `/tmp/orcinus-reach.mjs` — 生产可达性分析(入口自适应:html 入口存在才纳入;`src/main`/`src/preload` 删除后自动退出根集)
- `/tmp/orcinus-batch-rm.mjs` — `git rm` + 冷门禁(默认整域一次;`--chunk N` 分批)
- `/tmp/orcinus-domain-keys.sh` — 收集 i18n 候选键
- `/tmp/orcinus-i18n-prune.mjs` — 安全删键(剩余源码无引用才删)

**仓库内改动**

- Create(Task 8): `docs/phase3b-feature-trim-record.md`
- Modify(Task 8): `docs/superpowers/specs/2026-09-14-ade-design.md`
- Delete/Modify(Tasks 1–8): 见各任务「Files」;每域「独占文件清单」以 reach before/after diff 与编译器报错为准

---

## Task 0: 基线与一次性工具

**Files:**
- Create: `/tmp/orcinus-reach.mjs`、`/tmp/orcinus-batch-rm.mjs`、`/tmp/orcinus-domain-keys.sh`、`/tmp/orcinus-i18n-prune.mjs`
- Create: `/tmp/orcinus-reach-baseline.txt`、`/tmp/orcinus-test-baseline.log`、`/tmp/orcinus-test-baseline-failures.txt`、`/tmp/orcinus-test-baseline-collections.txt`

**Interfaces:**
- Consumes: 无
- Produces: 4 个工具(Tasks 1–8 调用);reach 基线与测试基线清单(Task 8 用)

- [ ] **Step 1: 确认分支与工作区**

```bash
git status -sb   # 期望:## phase3b-trim-local-focus,无未提交改动
```

- [ ] **Step 2: 记录构建门禁基线**

```bash
rm -f tsconfig.tsbuildinfo && pnpm typecheck   # 期望 exit 0
pnpm build:web                                  # 期望 exit 0
```

- [ ] **Step 3: 记录测试基线(约 8–10 分钟)**

```bash
pnpm test 2>&1 | tee /tmp/orcinus-test-baseline.log; echo "exit=${PIPESTATUS[0]}"
grep -E "^ *(Test Files|Tests|Errors) " /tmp/orcinus-test-baseline.log | tail -4
grep -E "FAIL " /tmp/orcinus-test-baseline.log | sed 's/.*FAIL //' | LC_ALL=C sort -u > /tmp/orcinus-test-baseline-failures.txt
grep -E "^ FAIL \[" /tmp/orcinus-test-baseline.log | sed 's/^ FAIL \[//; s/\].*//' | LC_ALL=C sort -u > /tmp/orcinus-test-baseline-collections.txt
wc -l /tmp/orcinus-test-baseline-failures.txt /tmp/orcinus-test-baseline-collections.txt   # 期望均非 0(既有失败,Phase 1 记录为 172+4)
```

- [ ] **Step 4: 创建可达性分析脚本** `/tmp/orcinus-reach.mjs`

```js
// 生产可达性分析:入口 = renderer html(存在才纳入)+ src/main、src/preload、src/bridge、src/types 全量
// 解析 from '...' / import('...') / require('...'),支持相对路径、@/、@renderer/ 别名(仅字面量)。
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
// 默认:一次性 git rm 全部匹配文件,然后跑一次冷门禁(rm tsbuildinfo + typecheck + build:web)。
// --chunk N:每 N 个文件一次 git rm,逐批门禁(用于跨域混合批)。
// 门禁失败即停;按报错修引用后重跑本命令(git ls-files 不会列出已删文件,幂等)。
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
    execFileSync('rm', ['-f', 'tsconfig.tsbuildinfo'], { stdio: 'inherit' })
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
# 收集形如 a.b.c 的键字面量(覆盖 auto.* / components.* / settings.* 等命名空间)
set -euo pipefail
out="$1"; shift
grep -rhoE "'[a-zA-Z][a-zA-Z0-9_-]*(\.[a-zA-Z0-9_-]+){2,}'" "$@" \
  | tr -d "'" | sort -u > "$out"
echo "candidate keys: $(wc -l < "$out" | tr -d ' ')"
```

- [ ] **Step 7: 创建 i18n 安全删键脚本** `/tmp/orcinus-i18n-prune.mjs`

```js
// Usage: node /tmp/orcinus-i18n-prune.mjs <keys.txt> [--apply]
// 规则:候选键存在于 catalog;且剩余源码(排除 src/renderer/src/i18n/**)无完整键字面量;
//       且无 `父路径.${` 形式的模板拼接;才从 7 个 catalog 删除。
import { readFileSync, writeFileSync } from 'node:fs'
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
# 期望:unreachable 在 600–900 区间(Phase 1 收尾为 623,其后 phase2a–3a 新增少量死代码)。
# 若 >1200 或清单含 terminal-pane/worktree/editor 的生产文件,说明脚本解析有误,先修脚本再继续。
grep -cE "^src/(main|preload)/" /tmp/orcinus-reach-baseline.txt || true
# 注:src/main 与 src/preload 全量在根集里,正常应几乎不在基线中;Task 1 的删除靠显式 pathspec,不依赖基线。
```

- [ ] **Step 9: 提交**

无仓库改动(工具与基线在 /tmp),本任务不提交。

---

## Task 1: 移除无后端死面(browser / Linear / Jira / GitLab / SSH / 远程主机死树 / 杂项孤儿 / src-main / src-preload)

**Files(锚点,最终以 reach diff 与编译器为准):**
- Delete(分 6 批,见 Steps):
  - 批 A 内嵌浏览器:`src/renderer/src/components/browser-pane/**`(约 291)、`src/shared/browser-*/**`(约 45)、`src/renderer/src/store/slices/browser*`、`src/renderer/src/components/settings/BrowserPane.tsx`、`BrowserUsePane.tsx` 及 `browser-*` 搜索条目、`src/preload/api/browser-*.ts`
  - 批 B 集成提供方:`src/renderer/src/components/task-page/**`(约 47)、task* 散文件(linear* 约 37、jira* 约 8)、`components/gitlab/**`、`gitlab-item-dialog*`、`components/settings/LinearAgentSkillPane.tsx`、IntegrationsPane 的 Task providers 卡
  - 批 C SSH:`src/renderer/src/ssh/**`(8)+ sidebar `ssh-*`、设置 ssh 面、terminal-pane 内远程 PTY 传输文件(**本地 PTY 链路绝不删**,以 typecheck 报错与 reach 为边界)
  - 批 D 远程主机死树:`src/renderer/src/runtime/` 内 `remote-runtime-*`、`host-session-mirror*` 等生产不可达文件(以 reach 基线 + 逐文件 grep 导入者判定;`web-session-*` 测试若因缺文件 collection 失败,允许随批删除→collection 错误减少是允许的)
  - 批 E 杂项孤儿:`components/maintenance/**`(7,updater 卡)、star-nag(4)、ports(3)、`network/AddressPicker*`(3)
  - 批 F Electron 遗留:`src/main/**`(20)、`src/preload/**` 除 `api-types.ts` 外全部
- Modify: `src/shared/ui-chrome-types.ts`(tab 类型 `'browser'` 枚举与持久化校验)、`src/bridge/create-api.ts`(browser/ssh/jira/linear/runtime-environments/workspace-ports/workspace-session/updater/star-nag 等 mock 命名空间接线)、`src/bridge/mock/` 对应域文件、设置导航/搜索注册(`settings-navigation-remote-sections.ts` 等)、Cmd+J 注册、`src/shared/feature-interaction-*` 对应条目、`src/shared/default-global-settings.ts`(对应设置字段)

**Interfaces:**
- Consumes: Task 0 的 4 个工具与基线
- Produces: 死面删除提交;`src/preload` 仅剩 `api-types.ts`、`src/main` 消失(Task 2–8 的锚点)

- [ ] **Step 1: 删前快照**

```bash
node /tmp/orcinus-reach.mjs --out /tmp/reach-before-deadface.txt | head -1
```

- [ ] **Step 2: 批 A 内嵌浏览器(收集键 → 删除 → 门禁)**

```bash
# 先列出 shared 下 browser 独占目录/文件与 store slices,以实际名称为准:
ls src/shared | grep '^browser' ; ls src/renderer/src/store/slices | grep browser
bash /tmp/orcinus-domain-keys.sh /tmp/keys-deadface-a.txt \
  src/renderer/src/components/browser-pane src/renderer/src/components/settings/BrowserPane.tsx \
  src/renderer/src/components/settings/BrowserUsePane.tsx
node /tmp/orcinus-batch-rm.mjs src/renderer/src/components/browser-pane
node /tmp/orcinus-batch-rm.mjs <上一步 ls 出的 src/shared/browser* 实际路径>
node /tmp/orcinus-batch-rm.mjs <ls 出的 store/slices browser 实际文件>
node /tmp/orcinus-batch-rm.mjs src/renderer/src/components/settings/BrowserPane.tsx src/renderer/src/components/settings/BrowserUsePane.tsx
```

- [ ] **Step 3: 批 B Linear/Jira/GitLab + task-page**

```bash
bash /tmp/orcinus-domain-keys.sh /tmp/keys-deadface-b.txt \
  src/renderer/src/components/task-page src/renderer/src/components/gitlab
node /tmp/orcinus-batch-rm.mjs src/renderer/src/components/task-page src/renderer/src/components/gitlab
# task* / linear* / jira* / gitlab* 散文件:
git ls-files 'src/renderer/src/*linear*' 'src/renderer/src/*jira*' 'src/renderer/src/*gitlab*' | head -40   # 先看清单,确认均为 provider 独占
node /tmp/orcinus-batch-rm.mjs 'src/renderer/src/*linear*' 'src/renderer/src/*jira*' 'src/renderer/src/*gitlab*'
node /tmp/orcinus-batch-rm.mjs src/renderer/src/components/settings/LinearAgentSkillPane.tsx
```

- [ ] **Step 4: 批 C SSH(本地 PTY 不许伤)**

```bash
bash /tmp/orcinus-domain-keys.sh /tmp/keys-deadface-c.txt src/renderer/src/ssh
node /tmp/orcinus-batch-rm.mjs src/renderer/src/ssh
# sidebar ssh-* / 设置 ssh 面 / terminal-pane 远程传输:
git ls-files 'src/renderer/src/*ssh*' | grep -v '^src/renderer/src/ssh/'   # 看散落文件
# 逐个判定(仅 ssh 独占、无生产/测试导入者的才删),写入清单后删除:
node /tmp/orcinus-batch-rm.mjs <判定后的 ssh 独占文件清单>
rm -f tsconfig.tsbuildinfo && pnpm typecheck   # terminal-pane 本地链路必须无报错
```

- [ ] **Step 5: 批 D 远程主机死树(reach 驱动)**

```bash
grep -E "remote-runtime|host-session-mirror|runtime-environment" /tmp/orcinus-reach-baseline.txt | head -60
# 逐文件 grep 导入者(生产+测试),仅「无导入者」的加入删除清单 /tmp/deadtree.txt:
# grep -rn "<模块名>" src --include='*.ts' --include='*.tsx' | grep -v "被删文件自身"
xargs -a /tmp/deadtree.txt node /tmp/orcinus-batch-rm.mjs
```

- [ ] **Step 6: 批 E 杂项孤儿 + 批 F Electron 遗留**

```bash
node /tmp/orcinus-batch-rm.mjs src/renderer/src/components/maintenance
git ls-files | grep -E "star-nag|workspace-ports|AddressPicker" | head -20   # 先看
node /tmp/orcinus-batch-rm.mjs <star-nag/ports/network 判定后的文件>
# Electron 遗留:src/main 全删;src/preload 除 api-types.ts 全删
git ls-files src/main > /tmp/preload-batch.txt
git ls-files src/preload | grep -v 'api-types.ts' >> /tmp/preload-batch.txt
xargs -a /tmp/preload-batch.txt node /tmp/orcinus-batch-rm.mjs
```

- [ ] **Step 7: 编译器驱动解耦共享引用**

```bash
rm -f tsconfig.tsbuildinfo && pnpm typecheck 2>&1 | grep -E "error TS" | head -40
# 逐项处理(预计):
# - ui-chrome-types.ts 删 'browser' tab 类型 → tab-bar/tab-group 相关分支
# - create-api.ts 删对应 mock 命名空间接线;src/bridge/mock/ 删对应域文件
# - settings-navigation-remote-sections.ts 删条目;settings-search*/experimental-search 删索引
# - Cmd+J 注册表删对应条目;feature-interaction-* 删条目
# - default-global-settings.ts / settings store slices 删字段(browser/ssh/linear/updater/mobilePairing 相关;
#   mobilePairingCustomAddress 留到 Task 2 一并处理)
```

- [ ] **Step 8: i18n 清键**

```bash
cat /tmp/keys-deadface*.txt 2>/dev/null | sort -u > /tmp/keys-deadface-all.txt
node /tmp/orcinus-i18n-prune.mjs /tmp/keys-deadface-all.txt          # 先看报告
node /tmp/orcinus-i18n-prune.mjs /tmp/keys-deadface-all.txt --apply  # 确认后应用
rm -f tsconfig.tsbuildinfo && pnpm typecheck && pnpm build:web
```

- [ ] **Step 9: 守卫测试 + 可达性 diff**

```bash
pnpm test src/renderer/src/lazy-use-ref-ratchet.test.ts src/renderer/src/lazy-modal-mount-state.test.ts src/renderer/src/renderer-node-builtin-boundary.test.ts src/bridge src/renderer/src/i18n
node /tmp/orcinus-reach.mjs --out /tmp/reach-after-deadface.txt | head -1
comm -13 /tmp/reach-before-deadface.txt /tmp/reach-after-deadface.txt   # 新增孤儿:逐个判定删除或记录
```

- [ ] **Step 10: 提交**

```bash
git add -A && git commit -m "chore: 移除无后端死面(browser/Linear/Jira/GitLab/SSH/远程主机死树/Electron 遗留,含契约与测试)"
```

---

## Task 2: 移除 Web 远程客户端

**Files(锚点):**
- Delete: `src/renderer/src/web/**`(100 文件)、`src/renderer/web-index.html`、shared 配角:`pairing-local-ui-fields.ts`(+test)、`runtime-environment-store.ts`、`mobile-relay-pairing-fixtures.ts`、`runtime-pairing-reach.ts`、`mobile-relay-status.ts`、`mobile-relay-mint-failure.ts`(+test)、`mobile-pairing-connection-mode.ts`(+test)、`mobile-pairing-custom-address.ts`(+test)、`remote-pairing-address.ts`、`remote-pairing-verification.ts`、`network/pairing-url.ts`、`serve-option-validation.ts`;连带孤儿:`src/renderer/src/lib/remote-pairing-copy.ts`、`src/renderer/src/components/settings/RuntimeAccessGrantList.tsx`、`runtime-access-grants.ts`、`src/renderer/src/lib/web-client-location.ts` 及其约 12 处桌面消费分支
- Modify: `src/renderer/src/store/slices/settings.ts`(删 `mobilePairingCustomAddress(es)` normalize,约 L27-129)、`src/shared/default-global-settings.ts`(约 L214-216 对应默认值)
- 依赖:删除 `qrcode`、`tweetnacl`、`ws`(逐个 grep 确认 0 非测试命中后 `pnpm remove`)

**Interfaces:**
- Consumes: Task 0 工具;Task 1 完成后的干净基线
- Produces: web 客户端删除提交;`build:web` 门禁不受影响(vite 单入口,`web-index.html` 本就不进产物)

- [ ] **Step 1: 删前快照 + 收集键**

```bash
node /tmp/orcinus-reach.mjs --out /tmp/reach-before-web.txt | head -1
bash /tmp/orcinus-domain-keys.sh /tmp/keys-web.txt src/renderer/src/web
```

- [ ] **Step 2: 主目录删除 + 门禁**

```bash
node /tmp/orcinus-batch-rm.mjs src/renderer/src/web src/renderer/web-index.html
```

- [ ] **Step 3: shared 配角与连带孤儿(先逐个 grep 导入者)**

```bash
# 逐个确认仅 web/测试/孤儿引用后:
git ls-files | grep -E "pairing-local-ui-fields|runtime-environment-store|mobile-relay-pairing-fixtures|runtime-pairing-reach|mobile-relay-status|mobile-relay-mint-failure|mobile-pairing-connection-mode|mobile-pairing-custom-address|remote-pairing-address|remote-pairing-verification|pairing-url|serve-option-validation|remote-pairing-copy|RuntimeAccessGrantList|runtime-access-grants|web-client-location" > /tmp/web-deps.txt
cat /tmp/web-deps.txt
# 注意:shared/pairing.ts 不得出现在清单里(parsePairingCode 被 ephemeral-vm-recipes.ts 引用)
xargs -a /tmp/web-deps.txt node /tmp/orcinus-batch-rm.mjs
```

- [ ] **Step 4: 编译器驱动清理残留分支**

```bash
rm -f tsconfig.tsbuildinfo && pnpm typecheck 2>&1 | grep -E "error TS" | head -30
# 逐项处理:
# - lib/web-client-location.ts 的 isWebClientLocation 消费分支(settings/AppearancePane、TerminalAppearanceSection、
#   AgentSettingsDialog、terminal-clipboard-event-paste 等约 12 处)→ 删 web 分支保桌面分支
# - store/slices/settings.ts 删 mobilePairingCustomAddress(es) normalize;default-global-settings.ts 删默认值
```

- [ ] **Step 5: 依赖清理**

```bash
grep -rl "qrcode\|tweetnacl\|ws" src --include='*.ts' --include='*.tsx' | grep -v '\.test\.' || echo "CLEAN"
# 注意 'ws' 是高频子串,grep 用词边界:grep -rlE "\bfrom ['\"]ws['\"]|\brequire\(['\"]ws['\"]\)" src
pnpm remove qrcode tweetnacl ws   # 仅确认 0 命中后执行;有命中则保留该项并记录
rm -f tsconfig.tsbuildinfo && pnpm typecheck && pnpm build:web
```

- [ ] **Step 6: i18n 清键 + 守卫 + reach diff**

```bash
node /tmp/orcinus-i18n-prune.mjs /tmp/keys-web.txt
node /tmp/orcinus-i18n-prune.mjs /tmp/keys-web.txt --apply
pnpm test src/renderer/src/lazy-use-ref-ratchet.test.ts src/renderer/src/lazy-modal-mount-state.test.ts src/renderer/src/renderer-node-builtin-boundary.test.ts src/bridge src/renderer/src/i18n
node /tmp/orcinus-reach.mjs --out /tmp/reach-after-web.txt | head -1
comm -13 /tmp/reach-before-web.txt /tmp/reach-after-web.txt   # 新增孤儿逐个判定
```

- [ ] **Step 7: 提交**

```bash
git add -A && git commit -m "chore: 移除 Web 远程客户端(死面,含 pairing/relay 配角与依赖)"
```

---

## Task 3: 移除 GitHub 全链(含只读 checks)

**Files(锚点):**
- Delete(TS): `src/renderer/src/components/pull-request-page/**`(约 54)、`components/github-item-dialog/**`(52)、`components/github-project/**`(53)、`components/github/**`(44)、github* 散文件、task-page 内 github 部分(Task 1 已删 task-page 宿主,此处核对残留)、right-sidebar `checks*`/`pr-checks*`、editor 内 `check-run*`/`CheckRun*` 面板、`components/settings/IntegrationsPane.tsx` 的 Review providers 卡、`GitProviderApiBudgetPane*`、`githubCache` 相关
- Delete(桥): `src/bridge/real/gh.ts`、`src/bridge/mock/` gh 域、`create-api.ts` gh 接线;hosted-review 域(先 `grep -rn "hostedReview\|hosted-review" src/bridge` 判定是否纯 PR 审查服务,是则整域删)
- Delete(Rust): `src-tauri/crates/ade-bridge/src/commands/gh*.rs`(定位:`grep -rn "gh_" src-tauri/crates/ade-bridge/src/commands/`)+ `commands/mod.rs` 注册 + AppState gh 字段 + specta 类型清单
- Modify: 设置导航/搜索条目、`GitPane`/`CommitMessageAiPane` 中 provider 面(**git 基础面保留**)

**Interfaces:**
- Consumes: Task 0 工具
- Produces: GitHub 删除提交;bindings 重生成(gh 命名空间消失)

- [ ] **Step 1: 删前快照 + 收集键**

```bash
node /tmp/orcinus-reach.mjs --out /tmp/reach-before-gh.txt | head -1
bash /tmp/orcinus-domain-keys.sh /tmp/keys-gh.txt src/renderer/src/components/pull-request-page src/renderer/src/components/github src/renderer/src/components/github-item-dialog src/renderer/src/components/github-project src/renderer/src/components/settings/IntegrationsPane.tsx
```

- [ ] **Step 2: TS 侧删除 + 门禁**

```bash
node /tmp/orcinus-batch-rm.mjs src/renderer/src/components/pull-request-page src/renderer/src/components/github-item-dialog src/renderer/src/components/github-project src/renderer/src/components/github
git ls-files 'src/renderer/src/*github*' | grep -v components/   # 散文件逐个判定后删
node /tmp/orcinus-batch-rm.mjs <判定的 github 散文件>
node /tmp/orcinus-batch-rm.mjs src/bridge/real/gh.ts
```

- [ ] **Step 3: 编译器驱动解耦(right-sidebar/editor/settings/桥)**

```bash
rm -f tsconfig.tsbuildinfo && pnpm typecheck 2>&1 | grep -E "error TS" | head -40
# 逐项:右栏 checks/pr-checks 挂载、editor CheckRun 引用、IntegrationsPane Review 卡、
# GitProviderApiBudgetPane 注册、create-api.ts gh 接线、mock gh 域、设置搜索条目
```

- [ ] **Step 4: Rust 侧 gh 域删除 + bindings**

```bash
grep -rn "gh" src-tauri/crates/ade-bridge/src/commands/mod.rs | head   # 定位 gh 命令模块
node /tmp/orcinus-batch-rm.mjs src-tauri/crates/ade-bridge/src/commands/gh.rs   # 文件名以定位为准
# 编辑 mod.rs 删注册;lib.rs 删 AppState.gh 字段与 specta 清单项(编译器指路)
(cd src-tauri && cargo check)
(cd src-tauri && cargo run -p ade-bridge --bin export-bindings)
pnpm test src/bridge   # parity 必须绿
```

- [ ] **Step 5: i18n + 守卫 + reach diff + 提交**

```bash
node /tmp/orcinus-i18n-prune.mjs /tmp/keys-gh.txt && node /tmp/orcinus-i18n-prune.mjs /tmp/keys-gh.txt --apply
pnpm test src/renderer/src/lazy-use-ref-ratchet.test.ts src/renderer/src/lazy-modal-mount-state.test.ts src/renderer/src/renderer-node-builtin-boundary.test.ts src/bridge src/renderer/src/i18n
node /tmp/orcinus-reach.mjs --out /tmp/reach-after-gh.txt | head -1
comm -13 /tmp/reach-before-gh.txt /tmp/reach-after-gh.txt
git add -A && git commit -m "chore: 移除 GitHub 集成全链(含只读 checks 与 gh 后端,含契约与测试)"
```

---

## Task 4: 移除 Automations

**Files(锚点):**
- Delete(TS): `src/renderer/src/components/automations/**`(约 283)、store automations 切片、sidebar Automations 按钮(默认显示的入口 + 右键隐藏设置 `showAutomationsButton*`)、`components/settings/Automations*`、设置导航 `automations` 条目、Cmd+J 条目
- Delete(Rust): `src-tauri/crates/ade-automations/**`(5 文件 5048 行)+ workspace 成员、`src-tauri/crates/ade-bridge/src/commands/automations.rs`(497 行)与 mod.rs 注册、AppState、specta 清单
- Modify: SQLite automations 表**只删生产者不迁移**(记录注明);`src/bridge/create-api.ts` automations 接线

**Interfaces:**
- Consumes: Task 0 工具
- Produces: Automations 删除提交;bindings 重生成(automations 命名空间消失)

- [ ] **Step 1: 快照 + 收集键**

```bash
node /tmp/orcinus-reach.mjs --out /tmp/reach-before-auto.txt | head -1
bash /tmp/orcinus-domain-keys.sh /tmp/keys-auto.txt src/renderer/src/components/automations
```

- [ ] **Step 2: TS 侧删除 + 门禁**

```bash
node /tmp/orcinus-batch-rm.mjs src/renderer/src/components/automations
rm -f tsconfig.tsbuildinfo && pnpm typecheck 2>&1 | grep -E "error TS" | head -30
# 逐项:sidebar 按钮与设置、settings pane 与导航、store 切片注册、create-api.ts 接线、Cmd+J 条目
```

- [ ] **Step 3: Rust 侧删除 + workspace + bindings**

```bash
node /tmp/orcinus-batch-rm.mjs src-tauri/crates/ade-automations src-tauri/crates/ade-bridge/src/commands/automations.rs
# src-tauri/Cargo.toml:若 [workspace] members 显式列出 ade-automations 则移除该行(glob 写法则无需改)
grep -n "ade-automations" src-tauri/Cargo.toml src-tauri/crates/*/Cargo.toml || echo "无残留引用"
# mod.rs 删注册;lib.rs 删 AppState/specta 项(编译器指路)
(cd src-tauri && cargo check)
(cd src-tauri && cargo test -p ade-bridge)
(cd src-tauri && cargo run -p ade-bridge --bin export-bindings)
pnpm test src/bridge
```

- [ ] **Step 4: i18n + 守卫 + reach diff + 提交**

```bash
node /tmp/orcinus-i18n-prune.mjs /tmp/keys-auto.txt && node /tmp/orcinus-i18n-prune.mjs /tmp/keys-auto.txt --apply
pnpm test src/renderer/src/lazy-use-ref-ratchet.test.ts src/renderer/src/lazy-modal-mount-state.test.ts src/renderer/src/renderer-node-builtin-boundary.test.ts src/bridge src/renderer/src/i18n
node /tmp/orcinus-reach.mjs --out /tmp/reach-after-auto.txt | head -1
comm -13 /tmp/reach-before-auto.txt /tmp/reach-after-auto.txt
git add -A && git commit -m "chore: 移除 Automations(含 ade-automations crate 与命令,含契约与测试)"
```

---

## Task 5: 移除 Computer-use

**Files(锚点):**
- Delete(Rust): `src-tauri/crates/ade-computer/**`(12 文件 3630 行)+ workspace 成员、`ade-bridge/src/commands/computer_use.rs`(294 行)+ `computer_service.rs` + mod.rs 注册、AppState、specta 清单;`src-tauri/native/computer-use-macos/**`(28 个 Swift 源文件)及构建配置(`grep -rn "computer-use-macos\|computer_use" src-tauri/tauri.conf.json config/scripts/ src-tauri/Cargo.toml` 定位)
- Delete(CLI/技能): `crates/orcinus-cli/src/computer.rs`(子命令 mod + match 分支)、`skills/computer-use/**`、`skill-guides/computer-use.md`
- Delete(渲染层): `components/settings/ComputerUsePane.tsx`、`ComputerUseSkillSetupPanel*`、`computer-use*` 搜索条目、设置导航 `computer-use` 条目、`src/bridge/real/computer-use-permissions.ts` + mock 域 + create-api 接线
- Modify: `package.json` 删 `build:computer-macos` 脚本与 `config/scripts/build-computer-macos.mjs`

**Interfaces:**
- Consumes: Task 0 工具
- Produces: Computer-use 删除提交;bindings 重生成(computer 命名空间消失)

- [ ] **Step 1: 快照 + 收集键**

```bash
node /tmp/orcinus-reach.mjs --out /tmp/reach-before-cu.txt | head -1
bash /tmp/orcinus-domain-keys.sh /tmp/keys-cu.txt src/renderer/src/components/settings/ComputerUsePane.tsx
```

- [ ] **Step 2: 渲染层删除 + 解耦 + 门禁**

```bash
node /tmp/orcinus-batch-rm.mjs src/renderer/src/components/settings/ComputerUsePane.tsx src/bridge/real/computer-use-permissions.ts
rm -f tsconfig.tsbuildinfo && pnpm typecheck 2>&1 | grep -E "error TS" | head -30
# 逐项:ComputerUseSkillSetupPanel、设置导航 computer-use 条目、搜索条目、create-api/mock 接线
```

- [ ] **Step 3: Rust + Swift + CLI + 技能删除**

```bash
node /tmp/orcinus-batch-rm.mjs src-tauri/crates/ade-computer src-tauri/crates/ade-bridge/src/commands/computer_use.rs src-tauri/crates/ade-bridge/src/computer_service.rs
git rm -q -- src-tauri/native/computer-use-macos   # Swift 包整目录(构建产物若在其中一并走)
grep -rn "computer" src-tauri/tauri.conf.json config/scripts/*.mjs package.json | head   # 构建配置定位
# 编辑:tauri.conf.json / 构建脚本 / package.json(build:computer-macos)删对应项;删 config/scripts/build-computer-macos.mjs
# orcinus-cli:删 src/computer.rs + main.rs mod 与 match 分支(编译器指路)
node /tmp/orcinus-batch-rm.mjs src-tauri/crates/orcinus-cli/src/computer.rs
node /tmp/orcinus-batch-rm.mjs skills/computer-use skill-guides/computer-use.md
# grep 确认技能清单/文档无残留引用:
grep -rn "computer-use" src config docs/README.md README.md 2>/dev/null | grep -v "docs/superpowers\|docs/phase" | head || echo "CLEAN"
(cd src-tauri && cargo check)
(cd src-tauri && cargo test -p ade-bridge -p orcinus-cli)
(cd src-tauri && cargo run -p ade-bridge --bin export-bindings)
pnpm test src/bridge
```

- [ ] **Step 4: i18n + 守卫 + reach diff + 提交**

```bash
node /tmp/orcinus-i18n-prune.mjs /tmp/keys-cu.txt && node /tmp/orcinus-i18n-prune.mjs /tmp/keys-cu.txt --apply
pnpm test src/renderer/src/lazy-use-ref-ratchet.test.ts src/renderer/src/lazy-modal-mount-state.test.ts src/renderer/src/renderer-node-builtin-boundary.test.ts src/bridge src/renderer/src/i18n
node /tmp/orcinus-reach.mjs --out /tmp/reach-after-cu.txt | head -1
comm -13 /tmp/reach-before-cu.txt /tmp/reach-after-cu.txt
git add -A && git commit -m "chore: 移除 Computer-use(含 ade-computer crate、Swift 包与技能,含契约与测试)"
```

---

## Task 6: 移除 Onboarding

**Files(锚点):**
- Delete: `src/renderer/src/components/onboarding/**`(32 文件)、`use-onboarding*` 首启链挂载(app-shell 内,Phase 1 已拆 feature-tips,剩余部分整删)
- Modify: 首启路由落点(删除后直接进主界面,Landing 兜底);仅服务向导的 shared onboarding 契约(`grep -rn "onboarding" src/shared src/bridge --include='*.ts' -l` 逐个判定消费者)

**Interfaces:**
- Consumes: Task 0 工具
- Produces: Onboarding 删除提交

- [ ] **Step 1: 快照 + 收集键 + 删除**

```bash
node /tmp/orcinus-reach.mjs --out /tmp/reach-before-onb.txt | head -1
bash /tmp/orcinus-domain-keys.sh /tmp/keys-onb.txt src/renderer/src/components/onboarding
node /tmp/orcinus-batch-rm.mjs src/renderer/src/components/onboarding
```

- [ ] **Step 2: 编译器驱动解耦首启链**

```bash
rm -f tsconfig.tsbuildinfo && pnpm typecheck 2>&1 | grep -E "error TS" | head -30
# 逐项:app-shell 挂载点、use-onboarding hook、设置导航/搜索中 onboarding 条目、mock/契约中 onboarding 域
# 注意:mock 域清单中 onboarding 曾在 Phase 1 backlog 记为误用命名空间 fallback——整域删
```

- [ ] **Step 3: i18n + 守卫 + reach diff + 提交**

```bash
node /tmp/orcinus-i18n-prune.mjs /tmp/keys-onb.txt && node /tmp/orcinus-i18n-prune.mjs /tmp/keys-onb.txt --apply
pnpm test src/renderer/src/lazy-use-ref-ratchet.test.ts src/renderer/src/lazy-modal-mount-state.test.ts src/renderer/src/renderer-node-builtin-boundary.test.ts src/bridge src/renderer/src/i18n
node /tmp/orcinus-reach.mjs --out /tmp/reach-after-onb.txt | head -1
comm -13 /tmp/reach-before-onb.txt /tmp/reach-after-onb.txt
git add -A && git commit -m "chore: 移除 Onboarding 向导(含契约与测试)"
```

---

## Task 7: 移除 Artifacts

**Files(锚点):**
- Delete: `src/renderer/src/components/artifacts/**`(29 文件)、`components/settings/Artifacts*` pane、设置导航 `artifacts` 条目
- Modify: 侧栏 `showArtifactsButton` 显示逻辑与设置字段(`grep -rn "showArtifactsButton" src --include='*.ts' --include='*.tsx'` 定位全部消费点)、`src/bridge` artifacts mock 域(若存在)

**Interfaces:**
- Consumes: Task 0 工具
- Produces: Artifacts 删除提交

- [ ] **Step 1: 快照 + 收集键 + 删除**

```bash
node /tmp/orcinus-reach.mjs --out /tmp/reach-before-art.txt | head -1
bash /tmp/orcinus-domain-keys.sh /tmp/keys-art.txt src/renderer/src/components/artifacts
node /tmp/orcinus-batch-rm.mjs src/renderer/src/components/artifacts
```

- [ ] **Step 2: 解耦 + i18n + 守卫 + reach diff**

```bash
rm -f tsconfig.tsbuildinfo && pnpm typecheck 2>&1 | grep -E "error TS" | head -20
grep -rn "showArtifactsButton\|artifacts" src/renderer/src/components/sidebar src/shared --include='*.ts' --include='*.tsx' | head   # 清显示逻辑与设置字段
node /tmp/orcinus-i18n-prune.mjs /tmp/keys-art.txt && node /tmp/orcinus-i18n-prune.mjs /tmp/keys-art.txt --apply
pnpm test src/renderer/src/lazy-use-ref-ratchet.test.ts src/renderer/src/lazy-modal-mount-state.test.ts src/renderer/src/renderer-node-builtin-boundary.test.ts src/bridge src/renderer/src/i18n
node /tmp/orcinus-reach.mjs --out /tmp/reach-after-art.txt | head -1
comm -13 /tmp/reach-before-art.txt /tmp/reach-after-art.txt
```

- [ ] **Step 3: 提交**

```bash
git add -A && git commit -m "chore: 移除 Artifacts(含设置项与测试)"
```

---

## Task 8: 杂项 + 全局收尾

**Files:**
- Delete: AI Vault 死子面(`src/shared/ai-vault-search-client.ts`、`-contract.ts`、`-query-operators.ts`、`-transport.ts`、`-types.ts`、`-limit.ts`、`-test-fixture.ts` + 测试,共 9 文件)、`AiVaultSessionSubagents.tsx`、`ai-vault-first-prompt-card.tsx`
- Modify: `src/bridge/real/ai-vault.ts`(删 `searchSessions`/`searchStatus`/`listSubagentSessions`/`getFirstUserPrompt` rejecting 方法,parity 锁定数 4→同步)、`AiVaultSessionDetails.tsx`(删子代理/首条提示引用)
- Create: `docs/phase3b-feature-trim-record.md`
- Modify: `docs/superpowers/specs/2026-09-14-ade-design.md`(§2 保留/删除清单更新)

**Interfaces:**
- Consumes: Task 0 基线(reach + 测试);Tasks 1–7 全部完成
- Produces: 收尾提交 + 文档提交;最终记录

- [ ] **Step 1: AI Vault 死子面清理**

```bash
bash /tmp/orcinus-domain-keys.sh /tmp/keys-av.txt src/shared/ai-vault-search-client.ts
git ls-files | grep -E "ai-vault-search" > /tmp/av-dead.txt
cat /tmp/av-dead.txt   # 应为 9 文件(含测试);不含 ai-vault-session-filters.ts(活的本地过滤)
xargs -a /tmp/av-dead.txt node /tmp/orcinus-batch-rm.mjs
# 子代理与首条提示组件删除后,typecheck 指路删 real/ai-vault.ts 4 个 rejecting 方法与 Details 引用
rm -f tsconfig.tsbuildinfo && pnpm typecheck && pnpm test src/bridge src/renderer/src/i18n
```

- [ ] **Step 2: spikes 构建产物**

```bash
git check-ignore spikes/cef-embed/target/ && echo "IGNORED(仅本地 rm -rf,不算提交)" || git ls-files spikes/cef-embed/target/ | head -3
# IGNORED: rm -rf spikes/cef-embed/target(不进提交);被追踪: xargs -a <(git ls-files spikes/cef-embed/target/) git rm -q --
```

- [ ] **Step 3: i18n 全量残留清键 + 守卫**

```bash
# 汇总 8 份键清单去重后复查 notInCatalog/referenced 分布,对已删域做最后一轮 sweep:
cat /tmp/keys-*.txt 2>/dev/null | sort -u > /tmp/keys-final.txt
node /tmp/orcinus-i18n-prune.mjs /tmp/keys-final.txt --apply
rm -f tsconfig.tsbuildinfo && pnpm typecheck && pnpm build:web
pnpm test src/renderer/src/i18n
```

- [ ] **Step 4: 依赖最终清理**

```bash
# 对每个 candidate 逐个 grep(非测试 0 命中才删):
for dep in qrcode tweetnacl ws; do echo "== $dep =="; grep -rlE "from ['\"]${dep}['\"]|require\(['\"]${dep}['\"]\)" src --include='*.ts' --include='*.tsx' | grep -v '\.test\.' || echo "0 命中"; done
# Task 2 已删的应全为 0;若 Task 2 保守保留了任一项,此处复核后决定
```

- [ ] **Step 5: ratchet 重基线**

```bash
pnpm test src/renderer/src/terminal-pane-store-subscription-budget.test.ts src/renderer/src/child-process-import-boundary.test.ts 2>/dev/null || true
# 逐个 ratchet 跑,与 /tmp/orcinus-test-baseline.log 中的同名失败逐字对比:
# - 失败消失(preload/src-main 删除可能使 child-process/windows-console stale 转绿)→ 记录改进
# - 失败但原因变化(如 pane-agent-identity-inventory 缺已删文件 pin)→ 更新 pin 清单后重跑
# - 既有红且原因不变 → 不动
```

- [ ] **Step 6: 全局可达性兜底**

```bash
node /tmp/orcinus-reach.mjs --out /tmp/orcinus-reach-final.txt | head -1
comm -13 /tmp/orcinus-reach-baseline.txt /tmp/orcinus-reach-final.txt   # 新增孤儿:必须逐个判定(删或记录理由)
# 期望最终新增孤儿 = 0;基线既有不可达不扫(非目标)
```

- [ ] **Step 7: 全量测试对比基线**

```bash
pnpm test 2>&1 | tee /tmp/orcinus-test-final.log; echo "exit=${PIPESTATUS[0]}"
grep -E "FAIL " /tmp/orcinus-test-final.log | sed 's/.*FAIL //' | LC_ALL=C sort -u > /tmp/orcinus-test-final-failures.txt
comm -13 /tmp/orcinus-test-baseline-failures.txt /tmp/orcinus-test-final-failures.txt   # 期望输出为空(新增失败 = 0)
grep -E "^ FAIL \[" /tmp/orcinus-test-final.log | sed 's/^ FAIL \[//; s/\].*//' | LC_ALL=C sort -u > /tmp/orcinus-test-final-collections.txt
comm -13 /tmp/orcinus-test-baseline-collections.txt /tmp/orcinus-test-final-collections.txt   # 期望为空(collection 只许减不许增)
```

- [ ] **Step 8: 自动启动冒烟**

```bash
pnpm dev > /tmp/orcinus-dev.log 2>&1 &
# 等 60s:确认 Vite ready、应用进程存活、日志无编译错误/panic;然后 kill 并确认端口 1420 释放
kill %1
```

- [ ] **Step 9: 收尾提交(代码与 ratchet 部分)**

```bash
git add -A && git commit -m "chore: 收尾清理(AI Vault 死子面/spikes 产物/i18n 残留/ratchet 重基线)"
```

- [ ] **Step 10: 文档 + 人工冒烟清单(交用户)**

写 `docs/phase3b-feature-trim-record.md`(沿 phase1-feature-trim-record 格式:提交与门禁表、reach/i18n/依赖/ratchet/测试对比、残留表、人工冒烟清单);更新 `2026-09-14-ade-design.md` §2。人工冒烟清单:

1. `pnpm dev` 启动无报错;2. 终端开 tab 执行命令;3. 侧栏 worktree 列表切换;4. 编辑器打开文件、diff 批注可用;5. agent 在终端启动、右栏 Agents(Vault)列表与 resume 可用;6. 插件中心可达;7. 设置页各分组可达;8. 以下入口不存在:browser tab 类型、Linear/Jira/GitLab/GitHub 设置与面板、Automations 侧栏按钮、ComputerUse 设置项、Onboarding 首启向导、Artifacts 按钮、Web 客户端入口。

```bash
git add docs && git commit -m "docs: Phase 3B 功能删减记录与规格更新"
git log --oneline main..HEAD   # 最终核对提交序列
```
