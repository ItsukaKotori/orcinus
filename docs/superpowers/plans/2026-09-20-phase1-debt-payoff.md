# Phase 1 债务清偿实施计划（还债后再跑闭环）

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 清掉 Phase 1 backlog 第 1–7 项与第 8 项可做部分（xterm 补丁、测试重基线、mock 加固、fork electron 清理、PTY minors、死树与残留、文档回写），使 `pnpm test` 全绿、`typecheck/build` 全绿，为 Phase 1 核心工作流闭环提供干净基线。

**Architecture:** 11 个任务按依赖排序：先移植 xterm/node-pty 补丁族（消除大批被误判为环境噪声的红项），再删共享层与 UI 层死树/残留，随后加固 mock、抽取 preload 类型并清 electron，最后做测试重基线、PTY minors 与文档收尾。每个删除批 `git rm` + 冷门禁（`rm -f tsconfig.tsbuildinfo && pnpm typecheck && pnpm build:web`），每个任务一个提交；不做功能改动，不做端到端 VM/多 profile 删除。

**Tech Stack:** TypeScript 7（`tsc --noEmit`）、Vite（rolldown-vite）、React 19、Vitest 4、pnpm 12 patch 机制、Rust（`orcinus-pty` crate，portable-pty 0.9）。

**Spec:** 需求来自 `docs/phase0-dead-code-inventory.md`（Phase 1 backlog 1–8 与「Phase 1 建议」）、`docs/phase1-feature-trim-record.md`（残留表）；本计划的 Task 0–10 即上述两项的落地拆分。设计决策已经用户批准（2026-09-20）：CEF 按 no-go 回写、VM 运行目标保留、tiptap 先修后隔离、i18n 移动全子树一并清、branch+SDD+收尾合并。

## Global Constraints

- 分支 `phase1-debt-payoff`（Task 0 创建），base `main@7b6606d`；全程不 push；完成后由 finishing-a-development-branch 交用户决定合并。
- orca 参照仓库根为 `/Users/itsuka/CodeSpace/orca`（**不是** `../orca-main`；AGENTS.md/文档里的旧路径在 Task 10 勘误）。
- 冷门禁（每次删除批次后必须）：`rm -f tsconfig.tsbuildinfo && pnpm typecheck && pnpm build:web`，exit 0。
- 已跟踪文件删除一律 `git rm -q --`；一次性脚本放 `.superpowers/sdd/2026-09-20-phase1-debt-payoff/tools/`（已 gitignore），不进仓库。
- 测试基线：当前套件为红（2026-09-20 实测 173 项 test 级失败 / 41 失败文件，见 Task 0 采样）；本计划结束时 `pnpm test` 必须 0 失败。任何「修不动的环境项」必须记录在 Task 10 的收尾文档并给出证据，不允许静默 exclude。
- 不删（本计划明确保留）：`orca-profiles` 整链、`ephemeral-vm`/run-target、renderer remote-runtime live 15 模块与 17 个 live 测试、preload 运行期树（`*-bridge.ts`、`index.ts` 等，Phase 2 随 Tauri bridge 删除）、host-compat 字段（viewMode/telemetrySource/launch telemetry）、`activity-terminal-portal`、agent-hook-listener、`data-workspace-board-preserve-open`、mock 的 fake-success 插件流程（仅动 doc-preview）。
- 每任务提交信息用 Conventional Commits 中文（沿用仓库风格）。
- `serve-desktop-*`（G/I 已删面）与 `src/main` type-chain 残留：本计划删除，不在 Phase 2 复活。
- Windows 构建/签名/CI 验证不在本计划（macOS 环境）：Task 10 单列跟踪。

## 文件结构

**一次性工具（Task 0 创建，均不进仓库）**

- `.superpowers/sdd/2026-09-20-phase1-debt-payoff/tools/prune-allowlist.mjs` — 删除 allowlist 中源文件已不存在的条目
- `.superpowers/sdd/2026-09-20-phase1-debt-payoff/tools/i18n-prune.mjs` — 按路径前缀删 i18n 键并清空壳
- `.superpowers/sdd/2026-09-20-phase1-debt-payoff/baseline-test.log`、`baseline-failures.txt` — Task 0 基线

**本计划新增（进仓库）**

- `config/patches/node-pty@1.1.0.patch`、`@xterm__xterm@6.1.0-beta.303.patch`、`@xterm__addon-webgl@0.20.0-beta.299.patch`、`@xterm__addon-search@0.17.0-beta.300.patch`、`@xterm__addon-serialize@0.15.0-beta.300.patch`（自 orca 复制）
- `config/patches/xterm-src/*.src.patch`（4 个）、`config/patches/xterm-upstream.json`
- `config/scripts/regenerate-xterm-patches.mjs`、`xterm-patch-text.mjs`、`regenerate-xterm-patches.test.mjs`
- `docs/reference/xterm-patch-regeneration.md`
- `src/shared/preload-api/`（api-types.ts + api/*-api.ts，自 `src/preload` 迁入）
- `src/shared/preload-api/api/terminal-preview-api.ts`（自 bridge 抽出类型）
- `src/shared/electron-import-boundary.test.ts`（新 ratchet）
- `src/bridge/mock/clone-mock-value.ts`
- `examples/plugins/hello-orca/*`、`examples/plugins/hostile-panel/*`（自 orca 复制 fixture）
- `docs/phase1-debt-payoff-record.md`（Task 10）

---

## Task 0: 工作区、基线与一次性工具

**Files:**
- Create: `docs/superpowers/plans/2026-09-20-phase1-debt-payoff.md`（本文件，先提交）
- Create: `.superpowers/sdd/2026-09-20-phase1-debt-payoff/tools/prune-allowlist.mjs`
- Create: `.superpowers/sdd/2026-09-20-phase1-debt-payoff/tools/i18n-prune.mjs`
- Create: `.superpowers/sdd/2026-09-20-phase1-debt-payoff/baseline-test.log`（gitignored）

- [ ] **Step 1: 建分支并提交本计划**

```bash
git checkout main && git pull --ff-only || true
git checkout -b phase1-debt-payoff
git add docs/superpowers/plans/2026-09-20-phase1-debt-payoff.md
git commit -m "docs: 新增 Phase 1 债务清偿实施计划（11 步）"
```

- [ ] **Step 2: 建 SDD 工作区**

```bash
mkdir -p .superpowers/sdd/2026-09-20-phase1-debt-payoff/tools
```

（`.superpowers` 已在 .gitignore，无需再改。）

- [ ] **Step 3: 写 allowlist 裁剪脚本**

写入 `.superpowers/sdd/2026-09-20-phase1-debt-payoff/tools/prune-allowlist.mjs`：

```js
#!/usr/bin/env node
import { existsSync, readFileSync, writeFileSync } from 'node:fs'
import { join } from 'node:path'

const [file] = process.argv.slice(2)
if (!file) {
  console.error('usage: node prune-allowlist.mjs <allowlist-file>')
  process.exit(2)
}

// Some allowlists are repo-relative (src/...), others are src/-relative (cli/...).
const exists = (entry) => existsSync(entry) || existsSync(join('src', entry))

const lines = readFileSync(file, 'utf8').split('\n')
const kept = []
let removed = 0
for (const line of lines) {
  const entry = line.trim()
  if (!entry || entry.startsWith('#')) {
    kept.push(line)
    continue
  }
  if (exists(entry)) kept.push(line)
  else removed += 1
}

const entries = kept.filter((line) => line.trim() && !line.trim().startsWith('#')).length
writeFileSync(file, kept.join('\n'))
console.log(JSON.stringify({ file, removed, entries }))
```

- [ ] **Step 4: 写 i18n 裁剪脚本**

写入 `.superpowers/sdd/2026-09-20-phase1-debt-payoff/tools/i18n-prune.mjs`（前缀与精确键两种目标；嵌套与扁平键都按点分路径命中；删后自底向上清空对象）：

```js
#!/usr/bin/env node
import { readFileSync, writeFileSync } from 'node:fs'

const TARGETS = [
  'auto.components.skills.SkillsPage',
  'auto.components.mobile',
  'auto.components.settings.SshPassphraseDialog',
  'auto.components.settings.RemoteServerUpdateDialog',
  'auto.components.settings.AppearancePane.9da1020447',
  'auto.components.settings.AppearancePane.61d842eca0'
]

const FILES = [
  'src/renderer/src/i18n/locales/en.json',
  'src/renderer/src/i18n/locales/es.json',
  'src/renderer/src/i18n/locales/fr.json',
  'src/renderer/src/i18n/locales/ja.json',
  'src/renderer/src/i18n/locales/ko.json',
  'src/renderer/src/i18n/locales/zh.json',
  'src/renderer/src/i18n/en-runtime-required.json'
]

const isTarget = (path) => TARGETS.some((t) => path === t || path.startsWith(`${t}.`))

const countLeaves = (value) => {
  if (value === null || typeof value !== 'object') return 1
  if (Array.isArray(value)) return value.length
  return Object.values(value).reduce((sum, child) => sum + countLeaves(child), 0)
}

const prune = (node, path) => {
  if (node === null || typeof node !== 'object' || Array.isArray(node)) {
    return { value: node, removed: 0 }
  }
  const out = {}
  let removed = 0
  for (const [key, child] of Object.entries(node)) {
    const childPath = path ? `${path}.${key}` : key
    if (isTarget(childPath)) {
      removed += countLeaves(child)
      continue
    }
    const pruned = prune(child, childPath)
    removed += pruned.removed
    if (
      pruned.value !== null &&
      typeof pruned.value === 'object' &&
      !Array.isArray(pruned.value) &&
      Object.keys(pruned.value).length === 0
    ) {
      continue
    }
    out[key] = pruned.value
  }
  return { value: out, removed }
}

let totalRemoved = 0
for (const file of FILES) {
  const catalog = JSON.parse(readFileSync(file, 'utf8'))
  const { value, removed } = prune(catalog, '')
  totalRemoved += removed
  writeFileSync(file, `${JSON.stringify(value, null, 2)}\n`)
  console.log(JSON.stringify({ file, removed }))
}
console.log(JSON.stringify({ totalRemoved }))
```

- [ ] **Step 5: 记录构建门禁基线**

```bash
rm -f tsconfig.tsbuildinfo && pnpm typecheck && pnpm build:web
```

预期：exit 0（若失败即停，先查原因）。

- [ ] **Step 6: 记录测试基线（耗时约 9 分钟）**

```bash
pnpm test > .superpowers/sdd/2026-09-20-phase1-debt-payoff/baseline-test.log 2>&1
grep -E '^[[:space:]]*FAIL' .superpowers/sdd/2026-09-20-phase1-debt-payoff/baseline-test.log | sed 's/.*FAIL *//' | sort -u > .superpowers/sdd/2026-09-20-phase1-debt-payoff/baseline-failures.txt
wc -l .superpowers/sdd/2026-09-20-phase1-debt-payoff/baseline-failures.txt
tail -5 .superpowers/sdd/2026-09-20-phase1-debt-payoff/baseline-test.log
```

预期：记录失败清单（2026-09-20 实测 173 行 test 级失败；`sed` 必须先经 `grep -E '^\s*FAIL'` 过滤，否则非 FAIL 行会原样输出）。此文件是 Task 10 改红为绿的证据基线。

---

## Task 1: 移植 xterm/node-pty 补丁族与再生维护链

**Files:**
- Create: `config/patches/node-pty@1.1.0.patch` 等 5 个补丁 + `config/patches/xterm-src/` 4 个源补丁 + `config/patches/xterm-upstream.json`
- Create: `config/scripts/regenerate-xterm-patches.mjs`、`config/scripts/xterm-patch-text.mjs`、`config/scripts/regenerate-xterm-patches.test.mjs`
- Create: `docs/reference/xterm-patch-regeneration.md`
- Modify: `pnpm-workspace.yaml`（patchedDependencies 追加 5 条）
- Test: `src/renderer/src/components/terminal-search-decoration-leak.test.ts`、`terminal-search-long-wrapped-line.test.ts`、`src/renderer/src/components/terminal-pane/terminal-ime-xterm-transaction-events.test.ts`、`src/renderer/src/components/terminal-scrollback-decoration-eviction.test.ts`

**Interfaces:**
- Consumes: orca 仓库 `/Users/itsuka/CodeSpace/orca` 的补丁与脚本（版本与 ade 依赖逐字一致）。
- Produces: ade 本地 patched node_modules；后续 Task 8 依赖补丁后的 xterm 行为跑终端测试。

- [ ] **Step 1: 复制补丁与维护链**

```bash
cp /Users/itsuka/CodeSpace/orca/config/patches/node-pty@1.1.0.patch config/patches/
cp /Users/itsuka/CodeSpace/orca/config/patches/@xterm__xterm@6.1.0-beta.303.patch config/patches/
cp /Users/itsuka/CodeSpace/orca/config/patches/@xterm__addon-webgl@0.20.0-beta.299.patch config/patches/
cp /Users/itsuka/CodeSpace/orca/config/patches/@xterm__addon-search@0.17.0-beta.300.patch config/patches/
cp /Users/itsuka/CodeSpace/orca/config/patches/@xterm__addon-serialize@0.15.0-beta.300.patch config/patches/
mkdir -p config/patches/xterm-src docs/reference
cp /Users/itsuka/CodeSpace/orca/config/patches/xterm-src/*.src.patch config/patches/xterm-src/
cp /Users/itsuka/CodeSpace/orca/config/patches/xterm-upstream.json config/patches/
cp /Users/itsuka/CodeSpace/orca/config/scripts/regenerate-xterm-patches.mjs config/scripts/
cp /Users/itsuka/CodeSpace/orca/config/scripts/xterm-patch-text.mjs config/scripts/
cp /Users/itsuka/CodeSpace/orca/config/scripts/regenerate-xterm-patches.test.mjs config/scripts/
cp /Users/itsuka/CodeSpace/orca/docs/reference/xterm-patch-regeneration.md docs/reference/
```

不复制 `lint-staged@16.4.0.patch`、`@vscode__windows-process-tree@0.8.0.patch`（ade 无对应依赖）。

- [ ] **Step 2: 声明 patchedDependencies**

编辑 `pnpm-workspace.yaml`，在 `patchedDependencies:` 块内 ligatures 行之后追加（保留现有注释）：

```yaml
  node-pty@1.1.0: config/patches/node-pty@1.1.0.patch
  '@xterm/xterm@6.1.0-beta.303': config/patches/@xterm__xterm@6.1.0-beta.303.patch
  '@xterm/addon-webgl@0.20.0-beta.299': config/patches/@xterm__addon-webgl@0.20.0-beta.299.patch
  '@xterm/addon-search@0.17.0-beta.300': config/patches/@xterm__addon-search@0.17.0-beta.300.patch
  '@xterm/addon-serialize@0.15.0-beta.300': config/patches/@xterm__addon-serialize@0.15.0-beta.300.patch
```

- [ ] **Step 3: 安装并校验补丁生效**

```bash
pnpm install
grep -n "patchedDependencies" -A 7 pnpm-lock.yaml | head -12
grep -c "compositionTransaction" node_modules/@xterm/xterm/lib/xterm.js
grep -c "outColor = vec4(0,0,0,0)" node_modules/@xterm/addon-webgl/lib/addon-webgl.js
grep -c "src-prefix" config/patches/xterm-src/@xterm__xterm@6.1.0-beta.303.src.patch
```

预期：lockfile 出现 6 条 patchedDependencies（含原 ligatures）；xterm 补丁标记 >0；webgl 补丁标记 >0。

- [ ] **Step 4: 跑补丁再生维护链的自测**

```bash
node --test config/scripts/regenerate-xterm-patches.test.mjs
```

预期：PASS（纯文本测试，不联网）。若该文件不是 node:test 形态，改为读文件头确认其运行方式并在记录中注明命令。

- [ ] **Step 5: 跑补丁依赖的 4 个终端测试**

```bash
pnpm test src/renderer/src/components/terminal-search-decoration-leak.test.ts \
  src/renderer/src/components/terminal-search-long-wrapped-line.test.ts \
  src/renderer/src/components/terminal-pane/terminal-ime-xterm-transaction-events.test.ts \
  src/renderer/src/components/terminal-scrollback-decoration-eviction.test.ts
```

预期：全部 PASS（这些文件在基线中因缺补丁而红）。

- [ ] **Step 6: 冷门禁 + 提交**

```bash
rm -f tsconfig.tsbuildinfo && pnpm typecheck && pnpm build:web
git add config/patches config/scripts docs/reference pnpm-workspace.yaml pnpm-lock.yaml
git commit -m "chore: 移植 xterm/node-pty 补丁族与再生维护链"
```

> 注：不在 `vite.config.ts` 加 `@xterm/headless`/`addon-serialize` alias——orca 的别名是给 electron-vite 主进程打包用的，ade 渲染层经 `main` 字段已能解析（现有 22 处 import 的构建通过）。如后续构建报解析失败，再按 orca `electron.vite.config.ts:279-289` 补。

---

## Task 2: 删除 shared 层死树（rpc-contract / CLI 链 / remote-server-updates）

**Files:**
- Delete: `src/shared/rpc-contract/` 整树（76 文件）
- Delete: `src/shared/diff-comment-schema.ts`、`src/shared/orchestration-run-pagination.ts`、`src/shared/workspace-linked-item-schema.ts`
- Delete: `src/shared/mobile-push-contract.ts` + 其测试、`src/shared/mobile-relay-credential-contract.ts`、`src/shared/agent-skill-sharing-contract.ts` + 其测试、`src/shared/skill-upload-session-contract.ts`
- Delete: `src/shared/node-cli-command-resolution.ts`、`src/shared/system-cli-install-dirs.ts`、`src/shared/posix-version-manager-bin-dirs.ts`、`src/shared/local-agent-install-dir-detection.ts`、`src/shared/agent-cli-install-dir-fallback.test.ts`、`src/shared/nvm-default-alias.test.ts`、`src/shared/cli-runtime-pairing-boundary.test.ts`、`src/shared/__fixtures__/cli-runtime-pairing-allowlist.txt`、`src/shared/posix-command-path-lookup.ts` + `posix-command-path-lookup.test.ts`、`src/shared/plugins/plugin-vm-recipe-artifact.ts` + 其测试
- Delete: `src/renderer/src/runtime/remote-server-*`（install-failure-probe / restart-wait / update-batch / update-coordinator / updater-polling / update-errors 及各自测试）、`src/renderer/src/store/slices/remote-server-updates.ts` + `remote-server-updates.integration.test.ts`
- Modify: `src/renderer/src/store/index.ts:45,109`、`src/renderer/src/store/types.ts:43,87`、`src/renderer/src/store/slices/store-test-helpers.ts:49,104`

- [ ] **Step 1: rpc-contract 整树 + 连带模块**

```bash
git rm -rq -- src/shared/rpc-contract
git rm -q -- src/shared/diff-comment-schema.ts src/shared/orchestration-run-pagination.ts src/shared/workspace-linked-item-schema.ts
git rm -q -- src/shared/mobile-push-contract.ts src/shared/mobile-push-contract.test.ts src/shared/mobile-relay-credential-contract.ts src/shared/agent-skill-sharing-contract.ts src/shared/agent-skill-sharing-contract.test.ts src/shared/skill-upload-session-contract.ts
grep -rn "rpc-contract\|mobile-push-contract\|mobile-relay-credential-contract\|agent-skill-sharing-contract\|skill-upload-session-contract\|diff-comment-schema\|orchestration-run-pagination\|workspace-linked-item-schema" src --include='*.ts' --include='*.tsx' | grep -v "^src/shared/orchestration-rpc-contract"
rm -f tsconfig.tsbuildinfo && pnpm typecheck && pnpm build:web
git add -A && git commit -m "chore: 删除 rpc-contract 死树与连带契约模块"
```

预期：grep 无输出（除 `orchestration-rpc-contract.ts`，它是另一个 live 文件）；门禁 exit 0。

- [ ] **Step 2: CLI 死链与 test-only 孤儿**

```bash
git rm -q -- src/shared/node-cli-command-resolution.ts src/shared/system-cli-install-dirs.ts \
  src/shared/posix-version-manager-bin-dirs.ts src/shared/local-agent-install-dir-detection.ts \
  src/shared/agent-cli-install-dir-fallback.test.ts src/shared/nvm-default-alias.test.ts \
  src/shared/cli-runtime-pairing-boundary.test.ts src/shared/__fixtures__/cli-runtime-pairing-allowlist.txt \
  src/shared/posix-command-path-lookup.ts src/shared/posix-command-path-lookup.test.ts \
  src/shared/plugins/plugin-vm-recipe-artifact.ts src/shared/plugins/plugin-vm-recipe-artifact.test.ts
rm -f tsconfig.tsbuildinfo && pnpm typecheck && pnpm build:web
git add -A && git commit -m "chore: 删除 CLI 解析死链与 test-only 孤儿模块"
```

- [ ] **Step 3: remote-server-updates 整链 + store 注册**

```bash
git rm -q -- src/renderer/src/runtime/remote-server-*
git rm -q -- src/renderer/src/store/slices/remote-server-updates.ts src/renderer/src/store/slices/remote-server-updates.integration.test.ts
```

编辑三处注册（删除 import 行与 slice 组合行）：
- `src/renderer/src/store/index.ts`：删 `:45` import 与 `:109` `...createRemoteServerUpdatesSlice(...a),`
- `src/renderer/src/store/types.ts`：删 `:43` import 与 `:87` 的 `RemoteServerUpdatesSlice &`
- `src/renderer/src/store/slices/store-test-helpers.ts`：删 `:49` import 与 `:104` `...createRemoteServerUpdatesSlice(...a),`

保留 `src/shared/remote-server-update.ts`（live，被 `protocol-version.ts` 等引用）。

```bash
grep -rn "remoteServerUpdates\|RemoteServerUpdates" src --include='*.ts' --include='*.tsx'
rm -f tsconfig.tsbuildinfo && pnpm typecheck && pnpm build:web
git add -A && git commit -m "chore: 删除 remote-server-updates 整链（UI 面已移除）"
```

预期：grep 仅剩 `remote-server-update.ts` 自身与其 live 消费（如有），无 slice 痕迹；门禁 exit 0。

---

## Task 3: i18n 清键与设置面残留

**Files:**
- Modify: `src/renderer/src/i18n/locales/{en,es,fr,ja,ko,zh}.json`、`src/renderer/src/i18n/en-runtime-required.json`（脚本批量）
- Modify: `src/renderer/src/store/slices/ui/ui-slice-contract-preferences.ts:75-76`、`ui-slice-preference-actions.ts:214-218`、测试 `ui-hydration-workspace-preferences.test.ts:187-190`
- Modify: `src/shared/global-settings-types.ts:240`、`src/shared/default-global-settings.ts:139`、`src/renderer/src/components/settings/AppearanceWindowSidebarSection.tsx:230-233,246-248`
- Modify: `src/renderer/src/components/settings/provider-account-scope.ts:52,60,79,88`
- Test: `provider-account-scope.test.ts`、`cli-source-control-integration-cards.test.tsx`、`task-tracker-integration-cards.test.tsx`、`provider-rate-limit-scope-panels.test.tsx`、`src/renderer/src/i18n/` 全目录

- [ ] **Step 1: 核实相邻 remote-server-update 键无生产引用**

```bash
grep -rn "GeneralRemoteServerUpdates\|RemoteServerUpdateStatus" src --include='*.ts' --include='*.tsx' | grep -v i18n
```

预期：无输出。若有输出则保留这些键并在记录中说明；无输出则把它们追加进 `i18n-prune.mjs` 的 `TARGETS`：
`auto.components.settings.GeneralRemoteServerUpdates`、`auto.components.settings.RemoteServerUpdateStatus`。

- [ ] **Step 2: 运行 i18n 裁剪脚本**

```bash
node .superpowers/sdd/2026-09-20-phase1-debt-payoff/tools/i18n-prune.mjs
```

预期：totalRemoved ≈ 1447（四组 + 移动全子树 + showMobileButton 2；若 Step 1 追加两个键则 ≈1605，以脚本输出为准）；7 个 catalog 写出后 JSON 合法。

```bash
pnpm test src/renderer/src/i18n
```

预期：全 PASS（`runtime-required-catalog.test.ts` 的 en 与 en-runtime-required 同步缩小）。

- [ ] **Step 3: 删 `setUsagePercentageDisplay` setter**

删除 `ui-slice-contract-preferences.ts:75-76` 的类型声明与 `ui-slice-preference-actions.ts:214-218` 的实现；把 `ui-hydration-workspace-preferences.test.ts:187-190` 的 setter 调用断言改为「hydration 后该值只读」（断言现有状态值，不再调用 setter）。

```bash
pnpm test src/renderer/src/store/slices/ui-hydration-workspace-preferences.test.ts
```

- [ ] **Step 4: 删 `settings.showMobileButton`**

删除 `global-settings-types.ts:240` 字段、`default-global-settings.ts:139` 默认值、`AppearanceWindowSidebarSection.tsx:246-248` 的 SearchableSetting 块与 `:230-233` 搜索索引条目（`sidebarEntries[2]`，删后重排后续索引）。

```bash
grep -rn "showMobileButton" src
pnpm test src/renderer/src/components/settings/AppearanceWindowSidebarSection.test.tsx 2>/dev/null || pnpm test src/renderer/src/components/settings
```

预期：grep 无输出；设置面测试 PASS。

- [ ] **Step 5: 修 stale copy（4 条 fallback）**

把 `provider-account-scope.ts` 的 4 条 fallback 文案替换为不指向已删设置页的表述（translate 的 key 不变）：
- `:52` remoteServerCredentials → `'Credentials and account checks for this provider are owned by this remote server.'`
- `:60` localCredentials → `'Credentials and account checks for this provider are owned by this desktop client.'`
- `:79` remoteServerRateLimit → `'{{value0}} API budget is fetched from the CLI on this remote server.'`
- `:88` localRateLimit → `'{{value0}} API budget is fetched from the CLI on this desktop client.'`

同步更新 4 个断言旧文案的测试（`provider-account-scope.test.ts:16-37`、`cli-source-control-integration-cards.test.tsx:94,111`、`task-tracker-integration-cards.test.tsx:132,152`、`provider-rate-limit-scope-panels.test.tsx:41,56`），改为断言新文案。

```bash
pnpm test src/renderer/src/components/settings/provider-account-scope.test.ts
rm -f tsconfig.tsbuildinfo && pnpm typecheck && pnpm build:web
git add -A && git commit -m "chore: 清理 i18n 死键与设置面残留（setter、移动按钮、陈旧文案）"
```

---

## Task 4: 契约、目录与 ai-vault 残留

**Files:**
- Modify: `src/preload/api/ui-command-event-api.ts:50`、`src/preload/api/ui-bridge-state-and-menu-commands.ts:24-28`、`src/bridge/mock/ui-api.ts:24-25`、`src/renderer/src/web/preload-api/web-ui-api.ts:151-152`、`src/renderer/src/hooks/ipc-events-test-harness.ts:149`、`src/renderer/src/hooks/useIpcEvents-lifecycle.test.ts:59,110,378-387`
- Modify: `src/shared/feature-interaction-catalog.ts`（type union + 数组，删 7 id）、`src/shared/feature-interactions.test.ts:29-80`
- Delete: `src/renderer/src/components/right-sidebar/ai-vault-session-resume-in-chat.ts` + `.test.ts`
- Modify: `AiVaultPanel.tsx:33,291-297,379`、`AiVaultSessionVirtualList.tsx:22,43,72,210`、`AiVaultVirtualRow.tsx:25,46,76,108,181-182`

- [ ] **Step 1: 删 skill-share 契约与期望**

删除 preload 两处 `onOpenSkillShare` / `consumePendingSkillShare`（类型 + 实现）、mock 与 web stub 对应方法、harness stub；从 `useIpcEvents-lifecycle.test.ts` 移除 `:59` inventory 项、`:110` 注册顺序项、`:378-387` 分组顺序中的两处 skill-share 断言（按实际结构重排索引）。

```bash
pnpm test src/renderer/src/hooks/useIpcEvents
```

预期：30 文件 / 148 用例全 PASS（基线唯一失败即此缺口）。

- [ ] **Step 2: feature-interaction catalog 删 7 个无 writer 且功能已删的 id**

从 `src/shared/feature-interaction-catalog.ts` 的 `FeatureInteractionId` union 与 `FEATURE_INTERACTIONS` 数组删除：
`workspace-agent-sessions`、`client-hosted-browser`、`agent-browser-use`、`agent-orchestration`、`ephemeral-vm-setup`、`computer-use`、`mobile-pairing`。
同步删 `feature-interactions.test.ts` 的 `expectedIds` 同名 7 项。（保留 `*-setup` 项：其 writer 仍在。）

```bash
pnpm test src/shared/feature-interactions.test.ts
```

预期：PASS。理由：runtime 侧 RPC 映射未 fork，交互无法记录；Phase 1 端口后随实现补回（Task 10 记录）。

- [ ] **Step 3: 拆 ai-vault resume-in-chat plumbing**

删除 `ai-vault-session-resume-in-chat.ts` 与其测试；解开 `AiVaultPanel.tsx`、`AiVaultSessionVirtualList.tsx`、`AiVaultVirtualRow.tsx` 的 `getSessionResumeInChat`/`resumeInChat` 传参与 `onResumeInNewChat` 分支（动作恒不可用、UI 已隐藏）。

```bash
pnpm test src/renderer/src/components/right-sidebar
rm -f tsconfig.tsbuildinfo && pnpm typecheck && pnpm build:web
git add -A && git commit -m "chore: 清理 skill-share 契约、feature-interaction 死项与 ai-vault 残留"
```

---

## Task 5: 删除 relay/remote-runtime 死簇（先解耦 3 处 live 测试）

**Files:**
- Delete: `src/shared/e2ee-crypto.ts`、`src/shared/remote-runtime-shared-control-test-server.ts`、`src/shared/remote-runtime-shared-control-boundary.test.ts` 与其余 dead 生产模块 + 测试（见 Step 4 规则）
- Modify: `src/renderer/src/web/web-runtime-client.test.ts:4,6-12,86-88,295,657,689-705`、`src/renderer/src/web/web-runtime-client-file-watch-replay.test.ts:6,248`、`src/renderer/src/web/web-runtime-status-owner.test.ts:3-6,29-34`、`src/renderer/src/worktree/github-pr-suppression.test.ts:7,34-36`
- Modify: `src/shared/agent-hook-listener-relay-dependency.test.ts`（删 relay 消费者 it）

**保留（勿删）**：`src/shared/remote-runtime-client-error-classification.ts`、`-client-error.ts`、`-memory-limits.ts`、`-prepared-request-admission.ts`、`-pty-id.ts`、`-shared-control-types.ts`、`-socket-liveness.ts`、`-tailscale-hint.ts`；`src/shared/pairing.ts`；renderer live 15 个 `remote-runtime*` 生产文件与 `remote-runtime-pty-transport-test-harness.ts`/`-stream-fixtures.ts`（17 个 live 测试依赖）。

- [ ] **Step 1: e2ee 测试符号切到 web-e2ee**

把两个 web 测试对 `e2ee-crypto` 的 `encrypt/decrypt/encryptBytes` 引用改为 `./web-e2ee` 同名导出（`src/renderer/src/web/web-e2ee.ts` 已实现且 API 对齐；如测试用到 `MAX_E2EE_ENCRYPTED_BASE64_CHARACTERS` 常量而 live 侧没有，删掉该常量断言并记录）。

```bash
pnpm test src/renderer/src/web/web-runtime-client.test.ts src/renderer/src/web/web-runtime-client-file-watch-replay.test.ts
```

- [ ] **Step 2: status-owner 测试去 test-server**

`web-runtime-status-owner.test.ts` 不再 import `remote-runtime-shared-control-test-server`；改用本地 `FakeWebSocket`（照 `web-runtime-client.test.ts` 的写法：手动 `e2ee_hello/e2ee_ready/e2ee_auth/e2ee_authenticated` 握手）覆盖原用例意图。

```bash
pnpm test src/renderer/src/web/web-runtime-status-owner.test.ts
```

- [ ] **Step 3: github-pr-suppression 删死断言**

删除 `github-pr-suppression.test.ts:34-36` 对 `remoteRuntimeClientCapabilities()` 的断言；保留 `:31` live 的 `NATIVE_REMOTE_RUNTIME_CLIENT_CAPABILITIES` 断言与 `:7` import 中 live 部分。

```bash
pnpm test src/renderer/src/worktree/github-pr-suppression.test.ts
```

- [ ] **Step 4: 删死簇（生产 + 测试）**

规则：删除 `src/shared` 下 `remote-runtime-*`、`relay-*`、`e2ee-crypto.ts` 的全部文件，**排除** Step 「保留」列出的 8 个 live 模块；同时删除 `remote-runtime-shared-control-test-server.ts`、`remote-runtime-shared-control-boundary.test.ts`。参考（非穷尽）受影响测试：`relay-optional-artifacts.test.ts`、`remote-runtime-request-*.test.ts`、`remote-runtime-shared-control-*.test.ts`、`remote-runtime-subscription-*.test.ts`、`remote-runtime-client*.test.ts`、`remote-runtime-outbound-admission.test.ts`、`runtime-client-export-parity.test.ts`、`remote-runtime-transport-error-agreement.test.ts`。

```bash
ls src/shared | grep -E '^(remote-runtime-|relay-)' | grep -vE '^(remote-runtime-client-error-classification|remote-runtime-client-error|remote-runtime-memory-limits|remote-runtime-prepared-request-admission|remote-runtime-pty-id|remote-runtime-shared-control-types|remote-runtime-socket-liveness|remote-runtime-tailscale-hint)\.ts$'
```

用该清单逐个 `git rm -q --`（生产与测试一起）。再删 `src/shared/e2ee-crypto.ts`。

- [ ] **Step 5: 删 relay 边界测试的消费者 it**

`agent-hook-listener-relay-dependency.test.ts`：保留解析器单测（`:51-74`），删除读 `src/relay/*` 的第二个 `it`（`:76` 起）。

```bash
pnpm test src/shared/agent-hook-listener-relay-dependency.test.ts src/shared/agent-hook-listener
rm -f tsconfig.tsbuildinfo && pnpm typecheck && pnpm build:web
git add -A && git commit -m "chore: 删除 relay/remote-runtime 死簇并解耦 live 测试"
```

---

## Task 6: Mock 加固

**Files:**
- Create: `src/bridge/mock/clone-mock-value.ts`
- Modify: `onboarding-api.ts:8`、`cli-api.ts:22`、`repos-api.ts:6`、`runtime-environments-api.ts:6`、`workspace-session-api.ts:7,14`（fallback 层级）
- Modify（按引用返回）：`agent-awake-api.ts:13`、`preflight-api.ts:23,24`、`runtime-events-api.ts:20,21`、`onboarding-api.ts:9,17`、`cli-api.ts:23`、`worktrees-api.ts:64,70,72`、`plugins-api.ts:12,14,64`、`settings-api.ts:15,16`、`ui-api.ts:13`
- Modify: `src/bridge/mock/doc-preview-api.ts:5-10`
- Test: `src/bridge/unimplemented-fallback.test.ts`、`src/bridge/mock/*-api.test.ts`

- [ ] **Step 1: 加 clone helper**

创建 `src/bridge/mock/clone-mock-value.ts`：

```ts
/** Mock 返回值一律深拷贝，防止调用方修改污染模块级常量或闭包状态。 */
export const cloneMockValue = <T>(value: T): T => structuredClone(value)
```

- [ ] **Step 2: 修 6 处命名空间级 fallback 误用**

对 5 个文件把 `withUnimplementedFallback(partial)` 改成方法级：
- `onboarding-api.ts:8` → `withMethodFallback<PreloadApi['onboarding']>('onboarding', { ... })`
- `cli-api.ts:22` → `withMethodFallback<PreloadApi['cli']>('cli', { ... })`
- `repos-api.ts:6` → `withMethodFallback<PreloadApi['repos']>('repos', { ... })`
- `runtime-environments-api.ts:6` → `withMethodFallback<PreloadApi['runtimeEnvironments']>('runtimeEnvironments', { ... })`
- `workspace-session-api.ts:7` → `withMethodFallback('session', {...})`；`:14` → `withMethodFallback('remoteWorkspace', {...})`

（对照正确写法：`src/bridge/mock/browser-api.ts:9`。）改后缺失方法必须产生 `UnimplementedBridgeError` rejection，而非同步 TypeError。

- [ ] **Step 3: 返回值深拷贝**

按 Files 列表在返回处包 `cloneMockValue(...)`；`memory-api.ts:11` 的 `app.main/renderer/other` 三字段改为各自独立对象（不再共享同一 `emptyUsage` 引用）。`onboarding`/`settings`/`ui`/`plugins` 的闭包 `state` 一律 `cloneMockValue(state)` 返回。

- [ ] **Step 4: doc-preview mintGrant 改响亮失败**

`doc-preview-api.ts`：`mintGrant` 改为 `async () => { throw new UnimplementedBridgeError('docPreview.mintGrant') }`（从 `../unimplemented-fallback` 导入）；注释改为「mock 不发放 grant，预览能力未实现前一律拒绝」。

- [ ] **Step 5: 补回归测试并跑 bridge 套件**

在 `unimplemented-fallback.test.ts` 补：方法级 partial 传错包装时 `api.cli.<missing>()` reject `UnimplementedBridgeError`（按修好的 5 域各断言一次）。
在对应 `*-api.test.ts` 补：修改返回值后再次 `get/list` 不变化（agent-awake、preflight、runtime-events、onboarding、settings、plugins、ui、worktrees 各一例）。

```bash
pnpm test src/bridge
rm -f tsconfig.tsbuildinfo && pnpm typecheck && pnpm build:web
git add -A && git commit -m "chore: mock 加固（方法级 fallback、返回值拷贝、doc-preview 响亮失败）"
```

---

## Task 7: Fork electron 清理（preload 类型抽取 + main 残留 + ratchet）

**Files:**
- Create: `src/shared/preload-api/api-types.ts`、`src/shared/preload-api/api/*-api.ts`（41 个，`git mv` 自 `src/preload`）
- Create: `src/shared/preload-api/api/terminal-preview-api.ts`
- Create: 旧路径 re-export 桶：`src/preload/api-types.ts`、`src/preload/api/*-api.ts`
- Create: `src/shared/electron-import-boundary.test.ts`
- Modify: `tsconfig.json`（include 7-19、23-49 行）、`config/tsconfig.web.json`（include 6-7）
- Delete: `src/main/startup/single-instance-lock.ts`、`src/main/window/foreground-activation-policy.ts`、`src/main/window/focus-existing-window.ts`、`src/main/window/macos-app-activation.ts`、`src/renderer/src/lib/serve-desktop-promotion-session-continuity.test.ts`
- Delete（收尾）: `src/types/electron-type-shim.d.ts`（vitest 的 `electron-vitest-stub.ts` 保留，preload 运行期测试仍用）

- [ ] **Step 1: 抽 TerminalPreviewApi**

在 `src/preload/api/terminal-preview-bridge.ts` 中把 `TerminalPreviewApi` 类型（`:7-18`）移入新建 `src/preload/api/terminal-preview-api.ts`；bridge 文件改为 `import type { TerminalPreviewApi } from './terminal-preview-api'` 并 `satisfies`；`api-types.ts:22` 的 import 指向新文件。

```bash
pnpm typecheck
```

- [ ] **Step 2: 搬迁 42 个类型文件**

```bash
mkdir -p src/shared/preload-api/api
git mv src/preload/api-types.ts src/shared/preload-api/api-types.ts
for f in src/preload/api/*-api.ts; do git mv "$f" "src/shared/preload-api/api/$(basename "$f")"; done
```

修正搬迁文件中跨目录相对导入（`../../shared/*` → `../../*` 等），以 `pnpm typecheck` 报错为准逐个修。

- [ ] **Step 3: 建旧路径 re-export 桶**

`src/preload/api-types.ts`：

```ts
export type * from '../shared/preload-api/api-types'
```

每个 `src/preload/api/<name>-api.ts`：

```ts
export type * from '../../../shared/preload-api/api/<name>-api'
```

（`install.ts`/`create-api.ts` 等消费者无需改动，`import type` 会被构建擦除。）

```bash
rm -f tsconfig.tsbuildinfo && pnpm typecheck && pnpm build:web
pnpm test src/bridge src/renderer/src/web
```

- [ ] **Step 4: 收窄 typecheck program 并删 shim**

- `config/tsconfig.web.json:6-7`：`../src/preload/api-types.ts`、`../src/preload/api/**/*` → `../src/shared/preload-api/**/*`、`../src/preload/api-types.ts`、`../src/preload/api/*-api.ts`
- `tsconfig.json:7-19`：改为 `src/shared/preload-api/**/*`、`src/preload/api-types.ts`、`src/preload/api/*-api.ts`；删除 11 个顶层 preload 运行期 include
- `tsconfig.json:45-49`：删除 4 个 main electron 文件 include
- `git rm -q -- src/main/startup/single-instance-lock.ts src/main/window/foreground-activation-policy.ts src/main/window/focus-existing-window.ts src/main/window/macos-app-activation.ts`
- `git rm -q -- src/renderer/src/lib/serve-desktop-promotion-session-continuity.test.ts`
- `git rm -q -- src/types/electron-type-shim.d.ts`

```bash
grep -rn "from 'electron'" src/renderer src/shared src/bridge src/preload/api-types.ts src/preload/api/*-api.ts
rm -f tsconfig.tsbuildinfo && pnpm typecheck && pnpm build:web
```

预期：grep 无输出；门禁 exit 0。若 `Electron.*` 命名空间在剩余 program 文件中仍有引用（如 preload 测试被 include），把这些文件从 include 移出而非恢复 shim。

- [ ] **Step 5: 加 electron 回潮 ratchet**

创建 `src/shared/electron-import-boundary.test.ts`（对照 `src/shared/child-process/child-process-import-boundary.test.ts` 的模式）：

```ts
import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import { glob } from 'tinyglobby'

const ROOTS = [
  'src/renderer/**/*.{ts,tsx}',
  'src/shared/**/*.{ts,tsx}',
  'src/bridge/**/*.{ts,tsx}',
  'src/preload/api-types.ts',
  'src/preload/api/*-api.ts'
]

const ELECTRON_SPECIFIER =
  /(?:from\s*'electron'|from\s*"electron"|require\(\s*'electron'\s*\)|require\(\s*"electron"\s*\)|import\(\s*'electron'\s*\)|import\(\s*"electron"\s*\))/

it('keeps the renderer/shared/bridge type graph free of electron', async () => {
  const offenders = (await glob(ROOTS, { cwd: resolve('.'), absolute: false }))
    .filter((file) => !/\.test\.tsx?$/.test(file))
    .filter((file) => ELECTRON_SPECIFIER.test(readFileSync(file, 'utf8')))
  expect(offenders).toEqual([])
})
```

```bash
pnpm test src/shared/electron-import-boundary.test.ts
git add -A && git commit -m "chore: 抽取 preload 类型至 shared 并清除 electron 残留"
```

---

## Task 8: PTY minors 与 node-pty 构建启用

**Files:**
- Modify: `src-tauri/crates/orcinus-pty/src/lib.rs`（补单测）
- Modify: `docs/spikes/2026-09-14-pty-throughput.md`（新增「Phase 1 宿主实现要求」）
- Modify: `pnpm-workspace.yaml:10`（`node-pty: false` → `true`）

- [ ] **Step 1: 补 CPR 跨块单测**

在 `lib.rs` 末尾追加（`reply_to_cursor_query` 已实现跨块 tail 保留，单测固化行为）：

```rust
#[cfg(test)]
mod tests {
    use super::{reply_to_cursor_query, CPR_REPLY};

    #[derive(Default)]
    struct Sink(Vec<u8>);

    impl std::io::Write for Sink {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0.extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn replies_when_query_splits_across_chunks() {
        let mut tail = Vec::new();
        let mut sink = Sink::default();
        reply_to_cursor_query(&mut tail, b"\x1b[6", &mut sink).unwrap();
        assert!(sink.0.is_empty());
        reply_to_cursor_query(&mut tail, b"n", &mut sink).unwrap();
        assert_eq!(sink.0, CPR_REPLY);
    }

    #[test]
    fn keeps_only_partial_query_tail_without_replying() {
        let mut tail = Vec::new();
        let mut sink = Sink::default();
        reply_to_cursor_query(&mut tail, b"output\x1b[", &mut sink).unwrap();
        assert!(sink.0.is_empty());
        // CPR 查询是 4 字节；实现保留末尾至多 3 字节覆盖分块边界。
        assert_eq!(tail, b"t\x1b[");
    }
}
```

```bash
cargo test -p orcinus-pty
```

预期：2 个新单测 + 既有 throughput 测试 PASS。

- [ ] **Step 2: 启用 node-pty 构建并验证**

`pnpm-workspace.yaml`：`node-pty: false` → `node-pty: true`（注释同步为「补丁已移植，渲染层 parity 测试需要本地构建」）。

上游 prebuilt tarball 的 `spawn-helper` 权限为 644，直接安装后 spawn 会失败且补丁源码不会编译；需要强制源码构建。在 `package.json` 的 scripts 增加（对齐 orca 的 `rebuild:node` 命名）：

```json
"rebuild:node": "npm_config_build_from_source=true pnpm rebuild node-pty"
```

```bash
pnpm install
pnpm run rebuild:node
pnpm test src/renderer/src/components/terminal-pane/fish-color-scheme-child-stdin.node-pty.test.ts src/shared/fish-query-reply-child-stdin.node-pty.test.ts src/shared/pty-reply-echo-shapes.node-pty.test.ts
```

预期：三文件全部 PASS（基线中 `pty-reply-echo-shapes.node-pty` 4 项红；2026-09-20 实测源码构建后转绿）。若 `pnpm run rebuild:node` 失败，报告 BLOCKED 并附完整错误。

- [ ] **Step 3: 固化 Phase 1 PTY 宿主实现要求**

在 `docs/spikes/2026-09-14-pty-throughput.md` 末尾追加：

```markdown
## Phase 1 宿主实现要求（2026-09-20 还债批次固化）

1. **CPR 应答必须跨读块边界扫描**：`ESC[6n` 可能被切在两个 read 块之间；宿主需保留至多 3 字节尾部缓冲（spike 已实现并有单测，见 `orcinus-pty/src/lib.rs`）。
2. **读取循环不得以目标字节数为停止条件而不排空管道**：ConPTY 输出相对输入有 ~10% 放大（重绘/换行/重定位），达到阈值即停会把剩余放大数据留在管道；生产宿主必须显式排空并做背压分片，否则尾部字节丢失。
3. **独立 reader 线程必须可回收**：spike 的 reader thread 是故意 detach 的（killed child 后 ConPTY 管道可能保持打开，join 会挂起）；生产宿主需用 supervisor 生命周期管理（关闭时先断管道再 join，或进程级回收），不得让线程/句柄泄漏。
4. 通道决策不变：终端数据走本地 socket（Windows 命名管道 / macOS unix socket），Tauri Channel 仅承载控制/状态消息。
```

```bash
git add -A && git commit -m "chore: 固化 PTY 宿主实现要求并启用 node-pty 构建"
```

---

## Task 9: 测试重基线（ratchet、fixture、ENOENT、环境簇）

**Files:**
- Modify: `src/shared/child-process/__fixtures__/child-process-import-allowlist.txt`、`child-process-import-boundary.test.ts:32`
- Modify: `src/shared/child-process/__fixtures__/windows-console-visibility-allowlist.txt`、`windows-console-visibility.test.ts:37`
- Create: `config/scripts/locale-ko-key-overrides.json`（自 orca 复制 + 品牌替换）
- Create: `examples/plugins/hello-orca/{main.mjs,orca-plugin.json,panel.html}`、`examples/plugins/hostile-panel/{panel.html,orca-plugin.json}`
- Modify: `src/renderer/src/runtime/__fixtures__/web-session-terminal-host-finalization.ts`（重写）
- Delete: `src/renderer/src/components/workspace-cleanup/workspace-cleanup-scanned-host-confirmation-removal.test.tsx`、`src/shared/windows-lane-tree-removal-boundary.test.ts`
- Modify: `src/renderer/src/components/hover-reveal-touch-action-visibility.test.ts:26`
- Modify: `src/shared/pane-agent-identity-inventory.test.ts`、`src/shared/pane-agent-identity-surface-inventory.test.ts`
- Modify: `src/renderer/src/app-shell/workspace-view-cross-client-sync.test.tsx:184,191`、`vitest.config.ts:30-34`
- Modify: tiptap 环境失败对应文件

- [ ] **Step 1: ratchet 数据裁剪**

```bash
node .superpowers/sdd/2026-09-20-phase1-debt-payoff/tools/prune-allowlist.mjs src/shared/child-process/__fixtures__/child-process-import-allowlist.txt
node .superpowers/sdd/2026-09-20-phase1-debt-payoff/tools/prune-allowlist.mjs src/shared/child-process/__fixtures__/windows-console-visibility-allowlist.txt
```

各预期剩 7 条（工具已兼容 repo-relative 与 src/-relative 两种条目；实测 child-process 删 146 留 7、windows-console 删 58 留 7）。把 `child-process-import-boundary.test.ts:32` 的 `DIRECT_IMPORTER_PIN = 155` 改为 `7`；`windows-console-visibility.test.ts:37` 的 `UNHIDDEN_SPAWNER_PIN = 65` 改为 `7`。

```bash
pnpm test src/shared/child-process
```

预期：8 个用例全 PASS。

- [ ] **Step 2: ko overrides 移植**

```bash
cp /Users/itsuka/CodeSpace/orca/config/scripts/locale-ko-key-overrides.json config/scripts/
node -e "const fs=require('fs');const p='config/scripts/locale-ko-key-overrides.json';const s=fs.readFileSync(p,'utf8');fs.writeFileSync(p,s.replaceAll('Orca','Orcinus'))"
pnpm test src/renderer/src/i18n/ko-ui-semantic-mistranslations.test.ts
```

预期：2 用例 PASS（12 个目标键含 3 个品牌词经替换后匹配）。

- [ ] **Step 3: 移植插件 fixtures**

```bash
mkdir -p examples/plugins
cp -R /Users/itsuka/CodeSpace/orca/examples/plugins/hello-orca examples/plugins/
cp -R /Users/itsuka/CodeSpace/orca/examples/plugins/hostile-panel examples/plugins/
pnpm test src/shared/plugins/plugin-demo-fixture.test.ts src/shared/plugins/plugin-hostile-fixture.test.ts
```

预期：PASS。

- [ ] **Step 4: 重写 web-session finalization fixture**

用以下内容整体替换 `src/renderer/src/runtime/__fixtures__/web-session-terminal-host-finalization.ts`（去掉 `vi.importActual` 对已删 main 模块的依赖，本地实现无分组子集，行为与 orca `runtime-mobile-session-result-finalization.ts` 等价）：

```ts
import type { RuntimeMobileSessionTabsResult } from '../../../../shared/runtime-types'
import { dropRetirementProofsForLiveSurfaces } from '../../../../shared/terminal-retirement-proof-ledger'

/** Run terminal fixtures through the host's retirement filter before the renderer consumes them. */
export function finalizeHostTerminalSnapshot(
  snapshot: RuntimeMobileSessionTabsResult
): RuntimeMobileSessionTabsResult {
  if (snapshot.tabGroups !== undefined || snapshot.tabGroupLayout !== undefined) {
    throw new Error('This fixture only supports ungrouped terminal snapshot finalization')
  }
  const tabs = snapshot.tabs
  const active =
    tabs.find((tab) => tab.isActive && tab.id === snapshot.activeTabId) ??
    tabs.find((tab) => tab.isActive) ??
    (snapshot.activeTabId ? (tabs[0] ?? null) : null)
  const normalizedTabs =
    active && !tabs.some((tab) => tab.isActive)
      ? tabs.map((tab) => (tab.id === active.id ? { ...tab, isActive: true } : tab))
      : tabs
  return {
    worktree: snapshot.worktree,
    publicationEpoch: snapshot.publicationEpoch,
    snapshotVersion: snapshot.snapshotVersion,
    activeGroupId: null,
    activeTabId: active?.id ?? null,
    activeTabType: active?.type ?? null,
    ...(snapshot.retiredTerminalSurfaces
      ? {
          retiredTerminalSurfaces: dropRetirementProofsForLiveSurfaces(
            snapshot.retiredTerminalSurfaces,
            snapshot.tabs
          )
        }
      : {}),
    tabs: normalizedTabs
  }
}
```

```bash
pnpm test src/renderer/src/runtime/web-session-tabs-sync-terminal-mirroring.test.ts src/renderer/src/runtime/web-session-terminal-orphan-recovery-prior-removal.test.ts src/renderer/src/runtime/web-session-terminal-orphan-recovery-adoption-regressions.test.ts
```

预期：3 文件不再收集失败，用例 PASS。

- [ ] **Step 5: 删无对象的守卫测试 + hover 条目**

```bash
git rm -q -- src/renderer/src/components/workspace-cleanup/workspace-cleanup-scanned-host-confirmation-removal.test.tsx
git rm -q -- src/shared/windows-lane-tree-removal-boundary.test.ts
```

理由：前者审计的 `main/ipc/workspace-cleanup-scan` 分支在 ade 不存在，语义已被 `workspace-cleanup-removal-preflight.test.ts` 与 `workspace-cleanup-host-qualified-list-state.test.ts` 覆盖；后者审计的 `.github/workflows/pr.yml`（39 条 spec 中 33 条缺失）在 ade 不存在。删除 `hover-reveal-touch-action-visibility.test.ts:26` 的 `settings/MobilePairingQrSection.tsx` 条目（组件随移动面删除，其余 26 个文件与 CSS 断言保留）。

```bash
pnpm test src/renderer/src/components/hover-reveal-touch-action-visibility.test.ts
```

- [ ] **Step 6: pane-agent-identity inventory 重基线**

```bash
pnpm test src/shared/pane-agent-identity-inventory.test.ts src/shared/pane-agent-identity-surface-inventory.test.ts
```

按失败输出逐个删除 `INVENTORY` / `DIRECT_SINGLE_SOURCE_SURFACES` / `SURFACE_ROWS` / `EXPECTED_REBIND_SITES` 中指向 ade 不存在路径的条目（预计 17 + 5 + 7 + 3 行），**不新增 `src/main`/`mobile` 文件**；循环直到 PASS。记录删除条目数到 Task 10 收尾文档。

- [ ] **Step 7: excluded test 重指向并去掉 exclude**

`workspace-view-cross-client-sync.test.tsx`：删除 `:184`、`:191` 对 `mobile/` 源码的 `readFileSync` pin 及其依赖断言（保留其余 14 个用例的行为断言）；从 `vitest.config.ts` 的 exclude 中删除该文件条目（`:30-34`）与注释。

```bash
pnpm test src/renderer/src/app-shell/workspace-view-cross-client-sync.test.tsx
```

- [ ] **Step 8: tiptap/环境簇处置**

```bash
pnpm test src/renderer/src/components/editor 2>&1 | tail -30
```

对报 `window is not defined`/`no window object` 的文件：若文件头缺 `// @vitest-environment happy-dom` 则补上并复跑；若补后仍失败，追根因（setupFiles 顺序/happy-dom 版本）；仅当确认需要真实浏览器能力时，才在 `vitest.config.ts` exclude 加注释并记入 Task 10 收尾文档（禁止无证据隔离）。

- [ ] **Step 9: 全量跑一次并分类剩余失败**

```bash
pnpm test > .superpowers/sdd/2026-09-20-phase1-debt-payoff/after-rebaseline.log 2>&1
grep -E '^[[:space:]]*FAIL' .superpowers/sdd/2026-09-20-phase1-debt-payoff/after-rebaseline.log | sed 's/.*FAIL *//' | sort -u > .superpowers/sdd/2026-09-20-phase1-debt-payoff/after-rebaseline-failures.txt
wc -l .superpowers/sdd/2026-09-20-phase1-debt-payoff/after-rebaseline-failures.txt
```

与 Task 0 基线比对；剩余失败只允许两类：等待 Task 10 收尾记录的环境项（须有证据），或本任务引入的回归（必须修）。修完提交：

```bash
rm -f tsconfig.tsbuildinfo && pnpm typecheck && pnpm build:web
git add -A && git commit -m "chore: 测试套件重基线（ratchet、fixture、ENOENT、环境簇）"
```

---

## Task 10: 全绿门禁、文档回写与收尾

**Files:**
- Modify: `docs/superpowers/specs/2026-09-14-ade-design.md`（风险 1 + 新增 §10.1）
- Modify: `docs/phase0-dead-code-inventory.md`（勘误）
- Modify: `docs/phase0-acceptance.md:269`（悬空引用）
- Create: `docs/phase1-debt-payoff-record.md`

- [ ] **Step 1: 冷门禁 + 全量测试（必须 0 失败）**

```bash
rm -f tsconfig.tsbuildinfo && pnpm typecheck && pnpm build:web
pnpm test > .superpowers/sdd/2026-09-20-phase1-debt-payoff/final-test.log 2>&1
grep -E "Test Files|Tests " .superpowers/sdd/2026-09-20-phase1-debt-payoff/final-test.log | tail -4
```

预期：`Tests ... passed`，0 failed（Task 9 允许的隔离项须在文档列明；否则必须修）。

- [ ] **Step 2: GUI 复核（用户或执行者，记录证据）**

```bash
pnpm dev
```

清单：窗口启动无报错；打开一个 `.ts` 文件（CombinedDiff/DiffViewer）确认 `ts.worker-*.js` 实际加载、无 worker 报错；终端 pane 正常；设置页可开；`git status` 无异常。结果写入收尾文档。

- [ ] **Step 3: CEF no-go 回写 spec**

`docs/superpowers/specs/2026-09-14-ade-design.md` 风险清单第 1 条改为：

```markdown
1. **CEF 集成与打包**（体积、签名、公证、崩溃隔离）→ Phase 0 spike 结论（2026-09-14，macOS arm64）：按阈值判定 **no-go**；Phase 3 采用系统 WebView 降级方案（§4 决策 3）。Windows 签名/公证未验证，见 §10.1。
```

并在 `## 10. 风险清单` 之后追加：

```markdown
### 10.1 CEF no-go 披露

CEF spike 判定 no-go（体积/启动/双平台构建未达标；Windows 侧签名、公证与打包未验证）。Phase 3 浏览器能力按系统 WebView + 注入式元素拾取降级（Design Mode 仅 HTML/CSS，无录像/完整 cookie 能力），UI 标注能力差异。若未来重启 CEF 方案，需先补 Windows 构建/签名验证。
```

- [ ] **Step 4: 文档勘误**

- `docs/phase0-dead-code-inventory.md:94`：`88 个 preload` → `80（基线 88，Phase 1 删减 -8）`，并注明 `src/preload` 运行期树保留至 Phase 2。
- `docs/phase0-dead-code-inventory.md:20`：`remote-server-updates` 条目改为「已整链删除（原『启动链路仍在用』前提失效）」。
- `docs/phase0-dead-code-inventory.md:26`：`orca-profiles` 改为「整链 live，不删；仅多 profile 管理死动作留待后续」。
- `docs/phase0-dead-code-inventory.md:47`：`MobileEmulatorSettingsPane` 标注「已被 Phase 1 功能删减 §2.4 推翻（59e7ee2 整域删除）」。
- `docs/phase0-dead-code-inventory.md:83`：composer 运行目标条目改为「已决策：VM run-target 保留，端到端移除另立产品决策」。
- `docs/phase0-acceptance.md:269`：删除对不存在文件 `task-12-remediation-report.md` 的引用（改为指向 `docs/phase0-dead-code-inventory.md`）。
- 全仓查 orca 路径引用，把 `../orca-main/orca-main` 统一为 `/Users/itsuka/CodeSpace/orca`（如有）。

- [ ] **Step 5: 写收尾记录**

创建 `docs/phase1-debt-payoff-record.md`，结构：批次与提交序（Task 0–10）、删减统计（文件数/行数/i18n 叶子数）、测试基线→全绿对比（173 → 0）、保留与延迟项（preload 运行期树、orca-profiles 死动作、mock fake-success 流程、renderer relay harness、活动终端 portal、host-compat 字段）、Windows 验证单列跟踪（补丁构建、Windows lane、CEF 签名）、GUI 复核结果、隔离项（如有）。

```bash
git add -A && git commit -m "docs: 债务清偿收尾（CEF 回写、勘误、记录）"
```

- [ ] **Step 6: 交付**

调用 superpowers:finishing-a-development-branch：确认 base `main`、工作树干净、全绿证据，向用户给出合并/PR/保留选项（不做未批准的 push/merge）。

---

## 自检记录

- **Spec 覆盖**：backlog 1（Task 1）、2（Task 9）、3（Task 6）、4（Task 7）、5（Task 1 Step 3-6 + Task 10 Step 2）、6（Task 8）、7（Task 2/3/4/5）、8（Task 5 Step 5、Task 9、Task 10 Step 3-5）全部有任务落点；「先还债再跑闭环」的闭环部分不在本计划（下一轮 brainstorming）。
- **占位符扫描**：无 TBD/TODO；所有删除步骤带文件清单或可执行筛选命令；所有新增代码给全文。
- **类型一致性**：`withMethodFallback(prefix, partial)` 签名与 `unimplemented-fallback.ts` 一致；`cloneMockValue<T>` 与使用处一致；`finalizeHostTerminalSnapshot` 输入输出 `RuntimeMobileSessionTabsResult` 与两处 importer 一致；ratchet 用的是仓库既有 `tinyglobby` 依赖。
