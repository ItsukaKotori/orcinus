# Git Log 插件（增量 1：fixture UI 先行）Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 交付一个 IDEA VcsLog 风格的独立主区 Git Log 大视图**插件**（图谱 + 分支面板 + 过滤条 + 提交详情），增量 1 以 **fixture 驱动**、零改动现有 git 功能与插件宿主能力表。

**Architecture:** 插件包 `plugins/git-log/`（React 19 + Tailwind v4，vite 单入口构建产出**全内联** `dist/tab.html`；主区 tab 由 hand-authored `launcher.html` panel 调 `workspace.openTab` 打开）。UI 只依赖 `GitLogRepository` 接口；`FixtureRepository` 返回对齐 `GitHistoryResult` 的快照；图谱复用 electron-free 的 `src/shared/git-history-graph.ts` 纯逻辑（只读、不修改）。增量 2 才换真实数据（worker 通道），本增量 `ChannelRepository` 仅留类型占位。

**Tech Stack:** react/react-dom `^19.2.8`、@tanstack/react-virtual `^3.14.10`、tailwindcss `^4.2.4`（`@tailwindcss/vite`）、`@vitejs/plugin-react` `^5.2.0`、vite（`rolldown-vite@7.3.1`）、vitest `^4.1.11`、happy-dom `^20.11.8`、typescript `^7.0.2`。

**Spec:** `docs/superpowers/specs/2026-10-18-git-log-plugin-design.md`

## Global Constraints

- **不新增 capability、不改能力表**：仅用现有 `workspace:open-tab`。**禁止修改** `src/shared/plugins/plugin-capability-gates.ts`、`plugin-host-api.ts`、Rust `host/methods.rs`、`PLUGIN_CAPABILITY_KINDS`（避让并行中的 `phase3c-ui-worker-channel`）。
- **零 Rust 改动** → `cargo test --workspace` 不需重跑、不受影响。
- **不改现有 git 功能 / Source Control / `src/shared/git-history*.ts`**（只读复用）。
- **CSP 硬约束**（`panel_shell.rs`）：panel/tab 只能内联 `<script>/<style>`，禁外链 / `fetch` / `import` / `@import url()`；字体必须内联为 `data:` URI。产物 tab 用 IIFE 打包以杜绝运行时 `import()`。
- **视觉一致**：`@theme inline` 块与宿主 `src/renderer/src/assets/main.css:43` **逐字相同**，由 vitest parity 测试锁死；字体栈与宿主一致（Geist）。
- **面板 token 现实**：shell 仅注入 20 个 design-token（**不含** `--git-graph-*`）。图谱所需的 `--git-graph-ref/remote-ref/base-ref/lane-1..5` 由插件自带（`:root` 浅色 + `.dark` 深色），取值抄自宿主。
- **manifest 文件名**：`orca-plugin.json`；`pluginApi: 1`；`engines.orca` 沿用 `">=1.4.0"`。
- **文案 v1 英文内嵌**（不做 i18n 本地化）。
- **产物 `dist` 不入库**（`.gitignore` 排除 `plugins/**/dist`）。
- **测试**：DOM 测试文件顶部 `// @vitest-environment happy-dom`；纯逻辑测试用默认 `node` 环境。别名 `@shared/*` → `<root>/src/shared/*`。

---

## File Structure

新建（`plugins/git-log/`）：
- `orca-plugin.json` — manifest（tabs + launcher panel + `workspace:open-tab`）
- `build.mjs` — vite 单入口构建 → 全内联 `dist/tab.html`
- `tsconfig.json` — UI 层类型（web lib，禁 node types，`@shared/*` paths）
- `launcher.html` — **手写**内联 panel（按钮 → postMessage `workspace.openTab`），提交入库
- `src/ui/theme.css` — tailwind 入口 + 逐字 `@theme inline` + git-graph tokens + Geist `@font-face`
- `src/ui/tab-main.tsx` — tab 挂载入口
- `src/ui/GitLogView.tsx` — 视图组合（工具条 / 分支面板 / 图谱列表 / 详情）
- `src/ui/Toolbar.tsx`、`src/ui/RefPanel.tsx`、`src/ui/GraphList.tsx`、`src/ui/GraphLane.tsx`、`src/ui/CommitDetail.tsx`
- `src/ui/host-bridge.ts` — panel 侧 `call(action, params)` postMessage 助手（launcher 与 tab 共用逻辑）
- `src/domain/types.ts` — `GitLogSnapshot` / `GitLogRepository` / `ChannelRepository`(占位)
- `src/fixtures/fixture-snapshot.ts`、`src/fixtures/fixture-repository.ts`
- `src/graph/git-graph-model.ts` — 复用 shared 泳道、色 id→CSS var 映射
- 测试：`src/ui/theme-parity.test.ts`、`src/domain/snapshot-shape.test.ts`、`src/graph/git-graph-model.test.ts`、`src/ui/git-log-view.test.tsx`、`src/launcher.test.ts`

修改（根接线，均为行级新增）：
- `package.json` — 新增 `build:git-log`；`typecheck` 追加插件 tsconfig
- `vitest.config.ts` — `include` 增 `plugins/**/*.test.ts`、`plugins/**/*.test.tsx`；`resolve.alias` 增 `@shared`
- `.gitignore` — 增 `plugins/**/dist`
- `tsconfig.json` — 无需改（插件用独立 tsconfig）

---

## Task 1: 插件包骨架 + 内联构建 + 根接线

**Files:**
- Create: `plugins/git-log/orca-plugin.json`
- Create: `plugins/git-log/build.mjs`
- Create: `plugins/git-log/tsconfig.json`
- Create: `plugins/git-log/src/ui/tab-main.tsx`（占位渲染，Task 5 填充）
- Create: `plugins/git-log/src/ui/theme.css`（占位 tailwind 入口，Task 2 填充 tokens）
- Create: `plugins/git-log/src/build-inline.test.ts`
- Modify: `package.json`、`vitest.config.ts`、`.gitignore`

**Interfaces:**
- Consumes: 无（起点）
- Produces: `pnpm build:git-log` → `plugins/git-log/dist/tab.html`（全内联单文件）；`inlineSingleFile(html, {js, css})` 纯函数（供测试与 build 复用）

- [ ] **Step 1: 写失败测试——内联自检纯函数**

Create `plugins/git-log/src/build-inline.test.ts`:

```ts
import { describe, expect, it } from 'vitest'
import { assertInlinedSingleFile, inlineSingleFile } from './build-inline'

describe('inlineSingleFile', () => {
  it('在单个 HTML 内联 style 与 script，且无任何外部引用', () => {
    const html = inlineSingleFile({ js: 'console.log(1)', css: ':root{--x:1}' })
    expect(html).toContain('<style>:root{--x:1}</style>')
    expect(html).toContain('<script>console.log(1)</script>')
    expect(html).toContain('<div id="git-log-root"></div>')
    assertInlinedSingleFile(html) // 不抛异常
  })

  it('拒绝外链 script / import / @import url / http 引用', () => {
    expect(() => assertInlinedSingleFile('<script src="a.js"></script>')).toThrow(/external/)
    expect(() => assertInlinedSingleFile('<script>import("x")</script>')).toThrow(/dynamic-import/)
    expect(() => assertInlinedSingleFile('<style>@import url(http://a)</style>')).toThrow(/external/)
  })
})
```

- [ ] **Step 2: 运行测试确认失败**

Run: `pnpm test plugins/git-log/src/build-inline.test.ts`
Expected: FAIL —`Cannot find module './build-inline'`

- [ ] **Step 3: 写内联助手 + build.mjs**

Create `plugins/git-log/src/build-inline.ts`:

```ts
const FORBIDDEN: Array<[RegExp, string]> = [
  [/<script[^>]+\ssrc=/i, 'external'],
  [/url\(\s*['"]?https?:/i, 'external'],
  [/@import\s+url\(/i, 'external'],
  [/\bimport\s*\(/, 'dynamic-import'],
  [/\bimport\s+[\w{*]/, 'dynamic-import']
]

export function inlineSingleFile({ js, css }: { js: string; css: string }): string {
  return `<!doctype html>
<html>
  <head>
    <meta charset="utf-8" />
    <style>${css}</style>
  </head>
  <body>
    <div id="git-log-root"></div>
    <script>${js}</script>
  </body>
</html>
`
}

export function assertInlinedSingleFile(html: string): void {
  for (const [pattern, label] of FORBIDDEN) {
    if (pattern.test(html)) {
      throw new Error(`inline violation (${label}): ${pattern}`)
    }
  }
}
```

Create `plugins/git-log/build.mjs`:

```js
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
```

> 说明：`import ... from './src/build-inline.ts'` 在 Node 26 `--experimental-strip-types` 下可读；若项目 Node 不支持 `.ts` 直引，改为把助手复制到 `build-inline.mjs` 并从 `.mjs` 引入（保持与测试同源即可）。

- [ ] **Step 4: 写占位 UI + 占位 theme.css（Task 2/5 会替换）**

Create `plugins/git-log/src/ui/theme.css`:

```css
@import 'tailwindcss';
@source "./";
@custom-variant dark (&:is(.dark *));
body { margin: 0; }
```

Create `plugins/git-log/src/ui/tab-main.tsx`:

```tsx
import { createRoot } from 'react-dom/client'

function Placeholder() {
  return <div className="p-4 text-foreground">Git Log (loading scaffold)</div>
}

const el = document.getElementById('git-log-root')
if (el) createRoot(el).render(<Placeholder />)
```

- [ ] **Step 5: 写 manifest**

Create `plugins/git-log/orca-plugin.json`:

```json
{
  "manifestVersion": 1,
  "id": "git-log",
  "publisher": "ade-local",
  "name": "Git Log",
  "version": "0.1.0",
  "description": "IDEA VcsLog-style commit graph for the active workspace (fixture UI preview, increment 1).",
  "engines": { "orca": ">=1.4.0" },
  "pluginApi": 1,
  "contributes": {
    "tabs": [{ "id": "git-log", "title": "Git Log", "icon": "plug", "entry": "dist/tab.html" }],
    "panels": [{ "id": "launcher", "title": "Git Log", "icon": "plug", "entry": "launcher.html" }]
  },
  "capabilities": [{ "kind": "workspace:open-tab" }]
}
```

- [ ] **Step 6: 写 launcher.html（占位可点，Task 6 完整化）**

Create `plugins/git-log/launcher.html`:

```html
<!doctype html>
<html>
  <head>
    <meta charset="utf-8" />
    <style>
      body { margin: 0; padding: 10px; font-family: system-ui, sans-serif; color: var(--foreground, #ddd); background: var(--background, transparent); }
      button { width: 100%; padding: 8px 10px; border: 1px solid var(--border, #555); border-radius: 6px; background: var(--secondary, #2a2a2a); color: var(--foreground, #ddd); cursor: pointer; font: inherit; }
    </style>
  </head>
  <body>
    <button id="open">Open Git Log</button>
    <script>
      'use strict'
      document.getElementById('open').addEventListener('click', function () {
        window.parent.postMessage({ type: 'orca-panel-action', requestId: 'gl-open-1', action: 'workspace.openTab', params: { tabId: 'git-log' } }, '*')
      })
    </script>
  </body>
</html>
```

- [ ] **Step 7: 写插件 tsconfig.json**

Create `plugins/git-log/tsconfig.json`:

```json
{
  "extends": "../../config/tsconfig.web.json",
  "compilerOptions": {
    "noEmit": true,
    "baseUrl": ".",
    "paths": { "@shared/*": ["../../src/shared/*"] },
    "types": []
  },
  "include": ["src/**/*", "src/**/*.tsx"]
}
```

- [ ] **Step 8: 根接线（package.json / vitest.config.ts / .gitignore）**

Modify `package.json` — 在 `scripts` 增：
```json
"build:git-log": "node --experimental-strip-types plugins/git-log/build.mjs",
```
并把 `typecheck` 改为：
```json
"typecheck": "tsc --noEmit -p tsconfig.json && tsc --noEmit -p plugins/git-log/tsconfig.json",
```

Modify `vitest.config.ts` — `resolve.alias` 数组追加（在 `@` 之前）：
```ts
{ find: '@shared', replacement: resolve('src/shared') },
```
`test.include` 追加：
```ts
'plugins/**/*.test.ts',
'plugins/**/*.test.tsx',
```

Modify `.gitignore` — 追加一行：
```
plugins/**/dist
```

- [ ] **Step 9: 运行测试 + 构建，确认通过**

Run: `pnpm test plugins/git-log/src/build-inline.test.ts`
Expected: PASS（2 用例）

Run: `pnpm build:git-log`
Expected: `built dist/tab.html (... KiB)`；无外部引用报错

Run: `pnpm typecheck`
Expected: PASS

- [ ] **Step 10: Commit**

```bash
git add plugins/git-log package.json vitest.config.ts .gitignore
git commit -m "feat(git-log): 插件包骨架 + 内联单文件构建 + 根接线（增量 1）"
```

---

## Task 2: 主题与视觉一致层（@theme inline parity + git-graph tokens）

**Files:**
- Create: `plugins/git-log/src/ui/theme.css`（替换 Task 1 占位为完整版）
- Create: `plugins/git-log/src/ui/theme-parity.test.ts`

**Interfaces:**
- Consumes: 宿主 `src/renderer/src/assets/main.css`（`@theme inline` 块 + git-graph base tokens 值）
- Produces: `theme.css`（tab 与后续组件依赖它提供 `--git-graph-*`、`--color-*` utilities）

- [ ] **Step 1: 写失败 parity 测试**

Create `plugins/git-log/src/ui/theme-parity.test.ts`:

```ts
import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { dirname } from 'node:path'
import { expect, it } from 'vitest'

const here = dirname(fileURLToPath(import.meta.url))

function extractThemeInline(css: string): string {
  const start = css.indexOf('@theme inline {')
  if (start === -1) throw new Error('no @theme inline block')
  let depth = 0
  let i = css.indexOf('{', start)
  for (; i < css.length; i += 1) {
    if (css[i] === '{') depth += 1
    else if (css[i] === '}') {
      depth -= 1
      if (depth === 0) break
    }
  }
  return css.slice(start, i + 1).replace(/\s+/g, ' ').trim()
}

it('插件 @theme inline 与宿主 main.css 逐字一致', () => {
  const host = readFileSync(resolve(here, '../../../../src/renderer/src/assets/main.css'), 'utf8')
  const plugin = readFileSync(resolve(here, './theme.css'), 'utf8')
  expect(extractThemeInline(plugin)).toBe(extractThemeInline(host))
})
```

- [ ] **Step 2: 运行确认失败**

Run: `pnpm test plugins/git-log/src/ui/theme-parity.test.ts`
Expected: FAIL（占位 theme.css 无 `@theme inline` → `no @theme inline block`）

- [ ] **Step 3: 从宿主抄 `@theme inline` 块**

打开 `src/renderer/src/assets/main.css`，定位 `@theme inline {`（line 43）到其**闭合大括号**（同缩进的 `}`）整块，**逐字复制**（不改空格/顺序）。

- [ ] **Step 4: 写完整 theme.css**

Create `plugins/git-log/src/ui/theme.css`（`@theme inline { … }` 处粘贴 Step 3 的整块）:

```css
@import 'tailwindcss';
@source "./";
@custom-variant dark (&:is(.dark *));

@font-face {
  font-family: 'Geist';
  src: url('../../../../src/renderer/src/assets/fonts/Geist-Variable.woff2') format('woff2');
  font-weight: 100 900;
  font-style: normal;
  font-display: swap;
}

/* 宿主仅注入 20 个 token，不含 git-graph 色；图谱所需基色由插件自带。
   取值抄自 main.css（:root ≈ line 236-243，.dark ≈ line 352-359）。 */
:root {
  --git-graph-ref: #007acc;
  --git-graph-remote-ref: #b66dff;
  --git-graph-base-ref: #ea5c00;
  --git-graph-lane-1: #ffb000;
  --git-graph-lane-2: #dc267f;
  --git-graph-lane-3: #994f00;
  --git-graph-lane-4: #40b0a6;
  --git-graph-lane-5: #b66dff;
  --font-sans: 'Geist', system-ui, -apple-system, sans-serif;
}
html.dark {
  --git-graph-ref: #3794ff;
  --git-graph-remote-ref: #b66dff;
  --git-graph-base-ref: #ea5c00;
  --git-graph-lane-1: #ffb000;
  --git-graph-lane-2: #dc267f;
  --git-graph-lane-3: #ce9178;
  --git-graph-lane-4: #40b0a6;
  --git-graph-lane-5: #b66dff;
}

/* ↓↓↓ 逐字复制自 src/renderer/src/assets/main.css 的 @theme inline 块 ↓↓↓ */
@theme inline {
  /* PASTE VERBATIM HERE */
}
/* ↑↑↑ 逐字块结束 ↑↑↑ */

html, body { margin: 0; font-family: var(--font-sans); background: var(--background); color: var(--foreground); }
```

> `@theme inline` 块内引用了 `var(--sidebar)`、`var(--git-graph-*)` 等宿主未注入 token；未映射的 utility 解析为空、无副作用。git-graph 基色已在上面 `:root`/`html.dark` 定义，故 `bg-git-graph-lane-1` / `text-git-graph-ref` 等可用。

- [ ] **Step 5: 运行 parity 测试确认通过**

Run: `pnpm test plugins/git-log/src/ui/theme-parity.test.ts`
Expected: PASS

Run: `pnpm build:git-log`
Expected: 构建成功；`tab.html` 内 CSS 含 `.git-graph`/`bg-` utilities，字体为 `data:` URI（`assertInlinedSingleFile` 不抛）

- [ ] **Step 6: Commit**

```bash
git add plugins/git-log/src/ui/theme.css plugins/git-log/src/ui/theme-parity.test.ts
git commit -m "feat(git-log): @theme inline 逐字 parity + git-graph 基色 tokens"
```

---

## Task 3: 领域类型 + fixture 数据 + FixtureRepository

**Files:**
- Create: `plugins/git-log/src/domain/types.ts`
- Create: `plugins/git-log/src/fixtures/fixture-snapshot.ts`
- Create: `plugins/git-log/src/fixtures/fixture-repository.ts`
- Create: `plugins/git-log/src/domain/snapshot-shape.test.ts`

**Interfaces:**
- Consumes: `@shared/git-history-types`（`GitHistoryItem`/`GitHistoryItemRef`/`GitHistoryResult`）、`@shared/git-diff-compare-types`（`GitBranchChangeEntry`）
- Produces:
  - `GitLogSnapshot = GitHistoryResult`
  - `GitLogRepository { loadSnapshot(): Promise<GitLogSnapshot>; loadCommitFiles(oid: string): Promise<GitBranchChangeEntry[]> }`
  - `buildFixtureSnapshot(): GitLogSnapshot`、`class FixtureRepository implements GitLogRepository`
  - `ChannelRepository`（类型占位，不实现、不导出实例）

- [ ] **Step 1: 写失败测试**

Create `plugins/git-log/src/domain/snapshot-shape.test.ts`:

```ts
import { describe, expect, it } from 'vitest'
import { buildFixtureSnapshot } from '../fixtures/fixture-snapshot'

const isOid = (s: string) => /^[0-9a-f]{40}$/.test(s)

describe('fixture snapshot', () => {
  const snap = buildFixtureSnapshot()

  it('至少 30 提交且 id 为 40-hex、parentIds 均存在或为 40-hex 根', () => {
    expect(snap.items.length).toBeGreaterThanOrEqual(30)
    const ids = new Set(snap.items.map((i) => i.id))
    for (const item of snap.items) {
      expect(isOid(item.id)).toBe(true)
      for (const pid of item.parentIds) expect(ids.has(pid)).toBe(true)
    }
  })

  it('包含一个 merge（2 parents）、一个 tag、remote ref、ahead/behind 标志', () => {
    expect(snap.items.some((i) => i.parentIds.length >= 2)).toBe(true)
    expect(snap.items.some((i) => (i.references ?? []).some((r) => r.category === 'tags'))).toBe(true)
    expect(snap.remoteRef?.category).toBe('remote branches')
    expect(typeof snap.hasIncomingChanges).toBe('boolean')
    expect(typeof snap.hasOutgoingChanges).toBe('boolean')
  })

  it('items 按 topo 序：每个 item 的 parent 出现在其后', () => {
    const order = new Map(snap.items.map((i, idx) => [i.id, idx]))
    for (const item of snap.items) {
      for (const pid of item.parentIds) expect(order.get(pid)!).toBeGreaterThan(order.get(item.id)!)
    }
  })
})
```

- [ ] **Step 2: 运行确认失败**

Run: `pnpm test plugins/git-log/src/domain/snapshot-shape.test.ts`
Expected: FAIL —`Cannot find module '../fixtures/fixture-snapshot'`

- [ ] **Step 3: 写领域类型**

Create `plugins/git-log/src/domain/types.ts`:

```ts
import type { GitHistoryResult } from '@shared/git-history-types'
import type { GitBranchChangeEntry } from '@shared/git-diff-compare-types'

export type GitLogSnapshot = GitHistoryResult

export interface GitLogRepository {
  loadSnapshot(): Promise<GitLogSnapshot>
  loadCommitFiles(oid: string): Promise<GitBranchChangeEntry[]>
}

/** 增量 2 数据缝占位：经 worker.invokeCommand/session.post 构造。
 *  本增量不实现、不实例化、不声明 worker:call capability。 */
export interface ChannelRepository extends GitLogRepository {
  readonly __reservedForIncrement2: true
}
```

- [ ] **Step 4: 写 fixture 生成器**

Create `plugins/git-log/src/fixtures/fixture-snapshot.ts`:

```ts
import type { GitHistoryItem, GitHistoryItemRef, GitHistoryResult } from '@shared/git-history-types'

const DAY = 86_400_000
const BASE_TS = Date.UTC(2026, 9, 18, 12, 0, 0) // 固定，确保确定性

function oid(label: string): string {
  let h = 0x811c9dc5
  const s = `git-log-fixture:${label}`
  for (let i = 0; i < s.length; i += 1) {
    h ^= s.charCodeAt(i)
    h = Math.imul(h, 0x01000193)
  }
  const seed = (h >>> 0).toString(16).padStart(8, '0')
  return (seed.repeat(5) + '0').slice(0, 40) // 稳定 40-hex
}

const MAIN_LEN = 24
const FEAT_LEN = 6

// 拓扑序（newest → oldest，parent 必在其后）：merge tip → main0..7 → feature 链 → main8..23
const ORDER: string[] = ['M']
for (let k = 0; k <= 7; k += 1) ORDER.push(`main${k}`)
for (let k = 0; k < FEAT_LEN; k += 1) ORDER.push(`feat${k}`)
for (let k = 8; k < MAIN_LEN; k += 1) ORDER.push(`main${k}`)

function parentsOf(key: string): string[] {
  if (key === 'M') return ['main0', `feat${FEAT_LEN - 1}`]
  if (key.startsWith('feat')) {
    const k = Number(key.slice(4))
    return k === FEAT_LEN - 1 ? ['main8'] : [`feat${k + 1}`]
  }
  const k = Number(key.slice(4)) // main<k>
  return k === MAIN_LEN - 1 ? [] : [`main${k + 1}`]
}

const AUTHORS = ['Ada', 'Lin', 'Grace']
function authorOf(key: string): string {
  if (key === 'M') return 'Ada'
  if (key.startsWith('feat')) return 'Grace'
  return AUTHORS[Number(key.slice(4)) % 3]!
}
function ageOf(key: string): number {
  if (key === 'M') return 0
  return key.startsWith('feat') ? 4 + Number(key.slice(4)) : Number(key.slice(4)) * 2
}
function subjectOf(key: string): string {
  if (key === 'M') return 'Merge branch feature into main'
  return key.startsWith('feat') ? `experiment: feat probe ${key.slice(4)}` : `feat(core): main change ${key.slice(4)}`
}

export function buildFixtureSnapshot(): GitHistoryResult {
  const items: GitHistoryItem[] = ORDER.map((key) => {
    const author = authorOf(key)
    return {
      id: oid(key),
      parentIds: parentsOf(key).map(oid),
      subject: subjectOf(key),
      message: subjectOf(key),
      author,
      authorEmail: `${author.toLowerCase()}@example.com`,
      timestamp: BASE_TS - ageOf(key) * DAY,
      statistics: { files: 1 + (key.length % 5), insertions: 10 + ((key.length * 3) % 40), deletions: 3 + (key.length % 7) }
    }
  })

  const at = (key: string): GitHistoryItem => items[ORDER.indexOf(key)]!
  const addRef = (item: GitHistoryItem, ref: GitHistoryItemRef): void => {
    item.references = [...(item.references ?? []), ref]
  }
  const tip = at('M')
  const featTip = at('feat0')
  const remoteTarget = at('main10')
  const tagA = at('main15')
  const tagB = at('main20')

  addRef(tip, { id: 'refs/heads/main', name: 'main', revision: tip.id, category: 'branches' })
  addRef(featTip, { id: 'refs/heads/feature', name: 'feature', revision: featTip.id, category: 'branches' })
  addRef(remoteTarget, { id: 'refs/remotes/origin/main', name: 'origin/main', revision: remoteTarget.id, category: 'remote branches' })
  addRef(tagA, { id: 'refs/tags/v1.0.0', name: 'v1.0.0', revision: tagA.id, category: 'tags' })
  addRef(tagB, { id: 'refs/tags/v0.9.0', name: 'v0.9.0', revision: tagB.id, category: 'tags' })

  return {
    items,
    currentRef: { id: 'refs/heads/main', name: 'main', revision: tip.id, category: 'branches' },
    remoteRef: { id: 'refs/remotes/origin/main', name: 'origin/main', revision: remoteTarget.id, category: 'remote branches' },
    baseRef: undefined,
    mergeBase: remoteTarget.id,
    hasIncomingChanges: true,
    hasOutgoingChanges: true,
    hasMore: false,
    limit: items.length
  }
}
```

> 确定性纯函数：`ORDER` 保证每个 commit 的 parent 都在其后（满足拓扑测试）；`oid()` 稳定 40-hex。拓扑：`M`(merge, 2 parents) → `main0..7` → `feat0..5`（`feat5` fork 自 `main8`）→ `main8..23`（`main23` 为根），共 31 提交。分支 `main`/`feature`、远端 `origin/main`、tag `v1.0.0`/`v0.9.0` 齐备，`hasIncoming/Outgoing=true` 触发 shared 边界行。

- [ ] **Step 5: 写 FixtureRepository**

Create `plugins/git-log/src/fixtures/fixture-repository.ts`:

```ts
import type { GitBranchChangeEntry } from '@shared/git-diff-compare-types'
import type { GitLogRepository, GitLogSnapshot } from '../domain/types'
import { buildFixtureSnapshot } from './fixture-snapshot'

const FILES: GitBranchChangeEntry[] = [
  { path: 'src/core/engine.ts', status: 'modified', added: 12, removed: 3 },
  { path: 'src/core/types.ts', status: 'added', added: 40, removed: 0 },
  { path: 'docs/old.md', status: 'deleted', added: 0, removed: 27 }
]

export class FixtureRepository implements GitLogRepository {
  async loadSnapshot(): Promise<GitLogSnapshot> {
    return buildFixtureSnapshot()
  }
  async loadCommitFiles(_oid: string): Promise<GitBranchChangeEntry[]> {
    return FILES
  }
}
```

- [ ] **Step 6: 运行测试确认通过**

Run: `pnpm test plugins/git-log/src/domain/snapshot-shape.test.ts`
Expected: PASS

- [ ] **Step 7: Commit**

```bash
git add plugins/git-log/src/domain plugins/git-log/src/fixtures
git commit -m "feat(git-log): 领域类型 + 确定性 fixture 快照 + FixtureRepository"
```

---

## Task 4: 图谱模型（复用 shared 泳道 + 色映射）

**Files:**
- Create: `plugins/git-log/src/graph/git-graph-model.ts`
- Create: `plugins/git-log/src/graph/git-graph-model.test.ts`

**Interfaces:**
- Consumes: `@shared/git-history-graph`（`buildGitHistoryViewModels`/`buildDefaultGitHistoryColorMap`/`getGitHistoryItemLaneIndex`）、`GitLogSnapshot`（Task 3）
- Produces:
  - `type GraphRow = { viewModel: GitHistoryItemViewModel; laneIndex: number }`
  - `buildGraphRows(snapshot: GitLogSnapshot): GraphRow[]`
  - `graphColorVar(colorId: GitHistoryGraphColorId): string` → `'var(--git-graph-*)'`

- [ ] **Step 1: 写失败测试**

Create `plugins/git-log/src/graph/git-graph-model.test.ts`:

```ts
import { describe, expect, it } from 'vitest'
import { buildGraphRows, graphColorVar } from './git-graph-model'
import { buildFixtureSnapshot } from '../fixtures/fixture-snapshot'

describe('buildGraphRows', () => {
  const rows = buildGraphRows(buildFixtureSnapshot())

  it('行数等于快照提交数', () => {
    expect(rows.length).toBeGreaterThanOrEqual(30)
  })

  it('merge 提交有两个 output 泳道', () => {
    const merge = rows.find((r) => r.viewModel.historyItem.subject.startsWith('Merge branch'))
    expect(merge?.viewModel.outputSwimlanes.length).toBeGreaterThanOrEqual(2)
  })

  it('laneIndex 落在 0..6（shared 5 lane + ref）', () => {
    for (const r of rows) expect(r.laneIndex).toBeGreaterThanOrEqual(0)
    expect(Math.max(...rows.map((r) => r.laneIndex))).toBeLessThanOrEqual(6)
  })

  it('色 id 映射到 --git-graph-* CSS 变量', () => {
    expect(graphColorVar('git-graph-lane-1')).toBe('var(--git-graph-lane-1)')
    expect(graphColorVar('git-graph-ref')).toBe('var(--git-graph-ref)')
  })
})
```

- [ ] **Step 2: 运行确认失败**

Run: `pnpm test plugins/git-log/src/graph/git-graph-model.test.ts`
Expected: FAIL —`Cannot find module './git-graph-model'`

- [ ] **Step 3: 写模型**

Create `plugins/git-log/src/graph/git-graph-model.ts`:

```ts
import {
  buildDefaultGitHistoryColorMap,
  buildGitHistoryViewModels,
  getGitHistoryItemLaneIndex,
  type GitHistoryItemViewModel
} from '@shared/git-history-graph'
import type { GitHistoryGraphColorId } from '@shared/git-history-types'
import type { GitLogSnapshot } from '../domain/types'

export type GraphRow = {
  viewModel: GitHistoryItemViewModel
  laneIndex: number
}

export function buildGraphRows(snapshot: GitLogSnapshot): GraphRow[] {
  const colorMap = buildDefaultGitHistoryColorMap({
    currentRef: snapshot.currentRef,
    remoteRef: snapshot.remoteRef,
    baseRef: snapshot.baseRef
  })
  const viewModels = buildGitHistoryViewModels(
    snapshot.items,
    colorMap,
    snapshot.currentRef,
    snapshot.remoteRef,
    snapshot.baseRef,
    snapshot.hasIncomingChanges,
    snapshot.hasOutgoingChanges,
    snapshot.mergeBase
  )
  return viewModels.map((viewModel) => ({ viewModel, laneIndex: getGitHistoryItemLaneIndex(viewModel) }))
}

export function graphColorVar(colorId: GitHistoryGraphColorId): string {
  return `var(--git-graph-${colorId.replace(/^git-graph-/, '')})`
}
```

- [ ] **Step 4: 运行测试确认通过**

Run: `pnpm test plugins/git-log/src/graph/git-graph-model.test.ts`
Expected: PASS

Run: `pnpm typecheck`
Expected: PASS（shared 类型经 `@shared/*` 解析）

- [ ] **Step 5: Commit**

```bash
git add plugins/git-log/src/graph
git commit -m "feat(git-log): 图谱行模型复用 shared 泳道 + git-graph CSS 变量映射"
```

---

## Task 5: 主区 Tab UI（虚拟图谱列表 + SVG 泳道 + 工具条 + 详情）

**Files:**
- Create: `plugins/git-log/src/ui/GitLogView.tsx`
- Create: `plugins/git-log/src/ui/Toolbar.tsx`
- Create: `plugins/git-log/src/ui/RefPanel.tsx`
- Create: `plugins/git-log/src/ui/GraphList.tsx`
- Create: `plugins/git-log/src/ui/GraphLane.tsx`
- Create: `plugins/git-log/src/ui/CommitDetail.tsx`
- Create: `plugins/git-log/src/ui/format.ts`
- Create: `plugins/git-log/src/ui/select-rows.ts`
- Create: `plugins/git-log/src/ui/select-rows.test.ts`
- Modify: `plugins/git-log/src/ui/tab-main.tsx`（替换占位为真实挂载）
- Create: `plugins/git-log/src/ui/git-log-view.test.tsx`

**Interfaces:**
- Consumes: `GraphRow`/`buildGraphRows`/`graphColorVar`（Task 4）、`GitLogRepository`/`GitLogSnapshot`（Task 3）、`FixtureRepository`（Task 3）、`theme.css`（Task 2）
- Produces: `GitLogView`（**具名导出** React 组件，props `{ repository: GitLogRepository }`）、`selectVisibleRows(rows, query, activeRef): GraphRow[]`、`formatRelativeDate(ts?): string`、`shortHash(oid): string`

- [ ] **Step 1: 写纯逻辑助手（日期 + 本地过滤）+ 单测**

Create `plugins/git-log/src/ui/format.ts`:

```ts
export function formatRelativeDate(ts: number | undefined, now = Date.now()): string {
  if (!ts) return ''
  const diff = Math.max(0, now - ts)
  const min = Math.floor(diff / 60_000)
  if (min < 1) return 'just now'
  if (min < 60) return `${min}m ago`
  const hr = Math.floor(min / 60)
  if (hr < 24) return `${hr}h ago`
  const day = Math.floor(hr / 24)
  if (day < 30) return `${day}d ago`
  return new Date(ts).toISOString().slice(0, 10)
}

export function shortHash(oid: string): string {
  return oid.slice(0, 7)
}
```

Create `plugins/git-log/src/ui/format.test.ts`:

```ts
import { expect, it } from 'vitest'
import { formatRelativeDate, shortHash } from './format'

it('formatRelativeDate 按阈值输出相对时间', () => {
  const now = Date.UTC(2026, 9, 18, 12)
  expect(formatRelativeDate(now - 30_000, now)).toBe('just now')
  expect(formatRelativeDate(now - 5 * 60_000, now)).toBe('5m ago')
  expect(formatRelativeDate(now - 3 * 3_600_000, now)).toBe('3h ago')
  expect(formatRelativeDate(now - 2 * 86_400_000, now)).toBe('2d ago')
})
it('shortHash 取前 7 位', () => expect(shortHash('abcdef0123456789')).toBe('abcdef0'))
```

Run: `pnpm test plugins/git-log/src/ui/format.test.ts` → PASS（实现已在同一提交内）

Create `plugins/git-log/src/ui/select-rows.ts`（纯函数，把过滤从组件抽出以便 node 环境测，不依赖虚拟列表布局）:

```ts
import type { GraphRow } from '../graph/git-graph-model'

export function selectVisibleRows(rows: GraphRow[], query: string, activeRef: string | null): GraphRow[] {
  const q = query.trim().toLowerCase()
  return rows.filter((r) => {
    const it = r.viewModel.historyItem
    if (activeRef && !(it.references ?? []).some((x) => x.id === activeRef)) return false
    if (!q) return true
    return it.subject.toLowerCase().includes(q) || (it.author ?? '').toLowerCase().includes(q)
  })
}
```

Create `plugins/git-log/src/ui/select-rows.test.ts`:

```ts
import { expect, it } from 'vitest'
import { selectVisibleRows } from './select-rows'
import { buildGraphRows } from '../graph/git-graph-model'
import { buildFixtureSnapshot } from '../fixtures/fixture-snapshot'

const rows = buildGraphRows(buildFixtureSnapshot())

it('无过滤返回全部行', () => expect(selectVisibleRows(rows, '', null).length).toBe(rows.length))
it('query 收窄到匹配 subject', () => {
  const feat = selectVisibleRows(rows, 'experiment', null)
  expect(feat.length).toBeGreaterThan(0)
  expect(feat.length).toBeLessThan(rows.length)
  expect(feat.every((r) => r.viewModel.historyItem.subject.includes('experiment'))).toBe(true)
})
it('activeRef 过滤到携带该 ref 的提交', () => {
  expect(selectVisibleRows(rows, '', 'refs/tags/v1.0.0').length).toBe(1)
})
```

Run: `pnpm test plugins/git-log/src/ui/select-rows.test.ts` → PASS（需 Task 4 的 `buildGraphRows` 已就绪）

- [ ] **Step 2: 写失败视图测试（happy-dom）**

Create `plugins/git-log/src/ui/git-log-view.test.tsx`:

```tsx
// @vitest-environment happy-dom
import { act } from 'react'
import { createRoot, type Root } from 'react-dom/client'
import { afterEach, beforeEach, describe, expect, it } from 'vitest'
import { GitLogView } from './GitLogView'
import { CommitDetail } from './CommitDetail'
import type { GitHistoryItem } from '@shared/git-history-types'
import { FixtureRepository } from '../fixtures/fixture-repository'

let host: HTMLDivElement
let root: Root
beforeEach(() => {
  host = document.createElement('div')
  document.body.append(host)
  root = createRoot(host)
})
afterEach(() => {
  act(() => root.unmount())
  host.remove()
})

const flush = async () => { await act(async () => { await Promise.resolve() }) }

// 不依赖 @tanstack/react-virtual 的行测量（happy-dom 无布局）：
// 只断言始终渲染在虚拟列表之外的头部/分支；过滤与选中行为由 node 环境的
// select-rows.test.ts 纯函数覆盖。
describe('GitLogView', () => {
  it('挂载后显示标题与当前分支', async () => {
    await act(async () => { root.render(<GitLogView repository={new FixtureRepository()} />) })
    await flush()
    expect(host.textContent).toContain('Git Log')
    expect(host.textContent).toContain('main')
  })
})

describe('CommitDetail', () => {
  it('渲染选中提交的 subject 与文件列表', async () => {
    const item: GitHistoryItem = {
      id: 'a'.repeat(40), parentIds: [], subject: 'Merge branch feature into main', message: 'body',
      author: 'Ada', authorEmail: 'ada@example.com', timestamp: Date.UTC(2026, 9, 18), statistics: { files: 2, insertions: 10, deletions: 3 }
    }
    const files = [{ path: 'src/core/engine.ts', status: 'modified' as const, added: 1, removed: 1 }]
    await act(async () => { root.render(<CommitDetail item={item} files={files} />) })
    expect(host.querySelector('[data-testid="commit-detail"]')?.textContent).toContain('Merge branch feature into main')
    expect(host.textContent).toContain('src/core/engine.ts')
  })
})
```

- [ ] **Step 3: 运行确认失败**

Run: `pnpm test plugins/git-log/src/ui/git-log-view.test.tsx`
Expected: FAIL —`Cannot find module './GitLogView'`

- [ ] **Step 4: 写 GraphLane（SVG 泳道）**

Create `plugins/git-log/src/ui/GraphLane.tsx`:

```tsx
import type { GraphRow } from '../graph/git-graph-model'
import { graphColorVar } from '../graph/git-graph-model'

const CELL = 14
export function GraphLane({ row }: { row: GraphRow }): React.JSX.Element {
  const { viewModel } = row
  const x = (i: number) => i * CELL + CELL / 2
  const cy = CELL / 2
  const lines: React.JSX.Element[] = []
  const selfX = x(row.laneIndex)

  viewModel.inputSwimlanes.forEach((node, i) => {
    lines.push(<line key={`in-${node.id}-${i}`} x1={x(i)} y1={0} x2={selfX} y2={cy} stroke={graphColorVar(node.color)} strokeWidth={1.5} />)
  })
  viewModel.outputSwimlanes.forEach((node, i) => {
    const isSelf = node.id === viewModel.historyItem.parentIds[0]
    lines.push(<line key={`out-${node.id}-${i}`} x1={selfX} y1={cy} x2={x(i)} y2={CELL} stroke={graphColorVar(node.color)} strokeWidth={isSelf ? 1.8 : 1.2} />)
  })
  return (
    <svg width={(viewModel.inputSwimlanes.length + 1) * CELL} height={CELL} className="shrink-0">
      {lines}
      <circle cx={selfX} cy={cy} r={3.5} fill={viewModel.kind === 'HEAD' ? graphColorVar('git-graph-ref') : 'var(--muted-foreground)'} stroke="var(--background)" strokeWidth={1} />
    </svg>
  )
}
```

- [ ] **Step 5: 写 GraphList（虚拟滚动）**

Create `plugins/git-log/src/ui/GraphList.tsx`:

```tsx
import { useRef } from 'react'
import { useVirtualizer } from '@tanstack/react-virtual'
import type { GraphRow } from '../graph/git-graph-model'
import { GraphLane } from './GraphLane'
import { formatRelativeDate, shortHash } from './format'

type Props = {
  rows: GraphRow[]
  selectedOid: string | null
  onSelect: (oid: string) => void
}

export function GraphList({ rows, selectedOid, onSelect }: Props): React.JSX.Element {
  const parentRef = useRef<HTMLDivElement>(null)
  const virt = useVirtualizer({ count: rows.length, getScrollElement: () => parentRef.current, estimateSize: () => 24, overscan: 12 })
  return (
    <div ref={parentRef} className="h-full min-h-0 flex-1 overflow-auto">
      <div style={{ height: virt.getTotalSize(), position: 'relative' }}>
        {virt.getVirtualItems().map((vi) => {
          const row = rows[vi.index]!
          const item = row.viewModel.historyItem
          const selected = item.id === selectedOid
          return (
            <div
              key={item.id}
              data-testid="graph-row"
              role="button"
              tabIndex={0}
              onClick={() => onSelect(item.id)}
              className={`flex items-center gap-2 px-2 text-xs ${selected ? 'bg-accent' : 'hover:bg-accent/40'}`}
              style={{ position: 'absolute', top: 0, left: 0, height: vi.size, width: '100%', transform: `translateY(${vi.start}px)` }}
            >
              <GraphLane row={row} />
              <span className="font-mono text-muted-foreground w-16 shrink-0">{shortHash(item.id)}</span>
              <span className="truncate flex-1 text-foreground">{item.subject}</span>
              <span className="w-24 shrink-0 truncate text-muted-foreground">{item.author}</span>
              <span className="w-16 shrink-0 text-right text-muted-foreground">{formatRelativeDate(item.timestamp)}</span>
            </div>
          )
        })}
      </div>
    </div>
  )
}
```

- [ ] **Step 6: 写 Toolbar / RefPanel / CommitDetail**

Create `plugins/git-log/src/ui/Toolbar.tsx`:

```tsx
import type { ChangeEvent } from 'react'

type Props = {
  branch: string
  query: string
  onQuery: (v: string) => void
  remoteCount: number
  localCount: number
  onRefresh: () => void
}

export function Toolbar({ branch, query, onQuery, remoteCount, localCount, onRefresh }: Props): React.JSX.Element {
  return (
    <div className="flex items-center gap-2 border-b border-border px-3 py-2 text-sm">
      <span className="font-medium text-foreground">Git Log</span>
      <span className="rounded bg-secondary px-1.5 py-0.5 text-xs text-secondary-foreground">{branch}</span>
      <span className="text-xs text-muted-foreground">↑{localCount} ↓{remoteCount}</span>
      <input
        data-testid="search"
        value={query}
        placeholder="Filter commits…"
        onChange={(e: ChangeEvent<HTMLInputElement>) => onQuery(e.target.value)}
        className="ml-auto w-48 rounded border border-input bg-transparent px-2 py-1 text-xs text-foreground outline-none focus:ring-1 focus:ring-ring"
      />
      <button type="button" onClick={onRefresh} className="rounded border border-border px-2 py-1 text-xs hover:bg-accent">Refresh</button>
    </div>
  )
}
```

Create `plugins/git-log/src/ui/RefPanel.tsx`:

```tsx
import type { GitHistoryItemRef } from '@shared/git-history-types'

export function groupRefs(refs: GitHistoryItemRef[]): Record<string, GitHistoryItemRef[]> {
  const out: Record<string, GitHistoryItemRef[]> = {}
  for (const r of refs) (out[r.category ?? 'commits'] ??= []).push(r)
  return out
}

type Props = { refs: GitHistoryItemRef[]; activeRef: string | null; onPick: (id: string | null) => void }

export function RefPanel({ refs, activeRef, onPick }: Props): React.JSX.Element {
  const groups = groupRefs(refs)
  const order = ['branches', 'remote branches', 'tags'] as const
  return (
    <div className="w-44 shrink-0 overflow-auto border-r border-border p-2 text-xs">
      {order.map((cat) => (groups[cat]?.length ? (
        <div key={cat} className="mb-2">
          <div className="mb-1 uppercase tracking-wide text-muted-foreground">{cat}</div>
          {groups[cat]!.map((r) => (
            <button
              key={r.id}
              type="button"
              onClick={() => onPick(activeRef === r.id ? null : r.id)}
              className={`block w-full truncate rounded px-1.5 py-0.5 text-left ${activeRef === r.id ? 'bg-accent text-foreground' : 'hover:bg-accent/40 text-muted-foreground'}`}
            >
              {r.name}
            </button>
          ))}
        </div>
      ) : null))}
    </div>
  )
}
```

Create `plugins/git-log/src/ui/CommitDetail.tsx`:

```tsx
import type { GitBranchChangeEntry } from '@shared/git-diff-compare-types'
import type { GitHistoryItem } from '@shared/git-history-types'
import { shortHash } from './format'

type Props = { item: GitHistoryItem | null; files: GitBranchChangeEntry[] }

export function CommitDetail({ item, files }: Props): React.JSX.Element {
  return (
    <div data-testid="commit-detail" className="h-48 shrink-0 overflow-auto border-t border-border p-3 text-xs">
      {!item ? (
        <div className="text-muted-foreground">Select a commit.</div>
      ) : (
        <div className="space-y-2">
          <div className="font-mono text-muted-foreground">{shortHash(item.id)}</div>
          <div className="font-medium text-foreground">{item.subject}</div>
          <div className="text-muted-foreground">{item.author} &lt;{item.authorEmail}&gt;</div>
          <div className="text-muted-foreground">{item.statistics ? `+${item.statistics.insertions} −${item.statistics.deletions} in ${item.statistics.files} files` : ''}</div>
          <ul className="mt-1 space-y-0.5">
            {files.map((f) => (
              <li key={f.path} className="flex items-center gap-2">
                <span className="w-4 text-center text-muted-foreground">{f.status[0]!.toUpperCase()}</span>
                <span className="truncate">{f.path}</span>
              </li>
            ))}
          </ul>
        </div>
      )}
    </div>
  )
}
```

- [ ] **Step 7: 写 GitLogView（状态编排 + 本地过滤 + 键盘）**

Create `plugins/git-log/src/ui/GitLogView.tsx`:

```tsx
import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import type { GitBranchChangeEntry } from '@shared/git-diff-compare-types'
import type { GitHistoryItemRef } from '@shared/git-history-types'
import type { GitLogRepository } from '../domain/types'
import { buildGraphRows, type GraphRow } from '../graph/git-graph-model'
import { selectVisibleRows } from './select-rows'
import { Toolbar } from './Toolbar'
import { RefPanel } from './RefPanel'
import { GraphList } from './GraphList'
import { CommitDetail } from './CommitDetail'

export function GitLogView({ repository }: { repository: GitLogRepository }): React.JSX.Element {
  const [rows, setRows] = useState<GraphRow[]>([])
  const [refs, setRefs] = useState<GitHistoryItemRef[]>([])
  const [branch, setBranch] = useState('')
  const [ahead, setAhead] = useState(0)
  const [behind, setBehind] = useState(0)
  const [query, setQuery] = useState('')
  const [activeRef, setActiveRef] = useState<string | null>(null)
  const [selected, setSelected] = useState<string | null>(null)
  const [files, setFiles] = useState<GitBranchChangeEntry[]>([])
  const listRef = useRef<HTMLDivElement>(null)

  const load = useCallback(async () => {
    const snap = await repository.loadSnapshot()
    const next = buildGraphRows(snap)
    setRows(next)
    setBranch(snap.currentRef?.name ?? 'HEAD')
    setAhead(snap.hasOutgoingChanges ? 1 : 0)
    setBehind(snap.hasIncomingChanges ? 1 : 0)
    const seen = new Map<string, GitHistoryItemRef>()
    for (const item of snap.items) for (const r of item.references ?? []) seen.set(r.id, r)
    setRefs([...seen.values()])
    if (next.length && !selected) setSelected(next[0]!.viewModel.historyItem.id)
  }, [repository, selected])

  useEffect(() => { void load() }, [load])

  useEffect(() => {
    if (!selected) { setFiles([]); return }
    let alive = true
    repository.loadCommitFiles(selected).then((f) => { if (alive) setFiles(f) })
    return () => { alive = false }
  }, [selected, repository])

  // 拓扑几何在全量 rows 上计算（稳定）；过滤仅影响可见子集。
  const visible = useMemo(() => selectVisibleRows(rows, query, activeRef), [rows, query, activeRef])

  const onSelect = useCallback((oid: string) => setSelected(oid), [])
  const onKey = useCallback((e: React.KeyboardEvent) => {
    if (e.key !== 'ArrowDown' && e.key !== 'ArrowUp') return
    e.preventDefault()
    const idx = visible.findIndex((r) => r.viewModel.historyItem.id === selected)
    const next = e.key === 'ArrowDown' ? Math.min(visible.length - 1, idx + 1) : Math.max(0, idx - 1)
    if (visible[next]) setSelected(visible[next]!.viewModel.historyItem.id)
  }, [visible, selected])

  const selectedItem = rows.find((r) => r.viewModel.historyItem.id === selected)?.viewModel.historyItem ?? null

  return (
    <div className="flex h-screen flex-col bg-background text-foreground" tabIndex={0} onKeyDown={onKey} ref={listRef}>
      <Toolbar branch={branch} query={query} onQuery={setQuery} localCount={ahead} remoteCount={behind} onRefresh={() => { void load() }} />
      <div className="flex min-h-0 flex-1">
        <RefPanel refs={refs} activeRef={activeRef} onPick={setActiveRef} />
        <GraphList rows={visible} selectedOid={selected} onSelect={onSelect} />
      </div>
      <CommitDetail item={selectedItem} files={files} />
    </div>
  )
}
```

- [ ] **Step 8: 替换 tab-main.tsx 为真实挂载**

Overwrite `plugins/git-log/src/ui/tab-main.tsx`:

```tsx
import { createRoot } from 'react-dom/client'
import './theme.css'
import { GitLogView } from './GitLogView'
import { FixtureRepository } from '../fixtures/fixture-repository'

const el = document.getElementById('git-log-root')
if (el) createRoot(el).render(<GitLogView repository={new FixtureRepository()} />)
```

- [ ] **Step 9: 运行视图测试 + 构建确认通过**

Run: `pnpm test plugins/git-log/src/ui/git-log-view.test.tsx`
Expected: PASS（3 用例）

Run: `pnpm build:git-log && pnpm typecheck`
Expected: 构建成功、类型通过

- [ ] **Step 10: Commit**

```bash
git add plugins/git-log/src/ui
git commit -m "feat(git-log): 主区 tab UI（虚拟图谱/工具条/refs/详情/键盘导航）"
```

---

## Task 6: launcher 完成化 + 端到端手工验证

**Files:**
- Modify: `plugins/git-log/launcher.html`（补 openTab 结果回执 + 错误提示）
- Create: `plugins/git-log/src/launcher.test.ts`
- Create: `plugins/git-log/README.md`（dev 加载步骤）

**Interfaces:**
- Consumes: `workspace.openTab`（panel:true，`params:{tabId,title?,params?}` → `{instanceId,worktreeId}`）、postMessage 协议 `orca-panel-action` / `orca-panel-action-result`
- Produces: 可点开的右侧栏启动器；手工验证清单文档

- [ ] **Step 1: 写失败测试（launcher 内联安全 + 调用 openTab）**

Create `plugins/git-log/src/launcher.test.ts`:

```ts
import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { dirname } from 'node:path'
import { expect, it } from 'vitest'

const html = readFileSync(resolve(dirname(fileURLToPath(import.meta.url)), '../launcher.html'), 'utf8')

it('launcher 内联 workspace.openTab 且无外部引用', () => {
  expect(html).toContain("'workspace.openTab'")
  expect(html).toContain("tabId: 'git-log'")
  expect(html).not.toMatch(/<script[^>]+\ssrc=/i)
  expect(html).not.toMatch(/@import\s+url\(/i)
  expect(html).not.toMatch(/url\(\s*['"]?https?:/i)
})
```

- [ ] **Step 2: 运行确认失败**

Run: `pnpm test plugins/git-log/src/launcher.test.ts`
Expected: 目前 PASS（Task 1 已含两处字符串）→ 改为断言新增：补 Step 3 后要求含 `orca-panel-action-result`。先加断言：

在测试追加：
```ts
  expect(html).toContain('orca-panel-action-result')
```
重跑 → FAIL（launcher.html 尚无结果处理）

- [ ] **Step 3: 完成化 launcher.html**

Overwrite `plugins/git-log/launcher.html`:

```html
<!doctype html>
<html>
  <head>
    <meta charset="utf-8" />
    <style>
      body { margin: 0; padding: 10px; font-family: system-ui, sans-serif; color: var(--foreground, #ddd); background: var(--background, transparent); }
      button { width: 100%; padding: 8px 10px; border: 1px solid var(--border, #555); border-radius: 6px; background: var(--secondary, #2a2a2a); color: var(--foreground, #ddd); cursor: pointer; font: inherit; }
      .status { margin-top: 8px; min-height: 1.2em; font-size: 12px; color: var(--muted-foreground, #999); }
    </style>
  </head>
  <body>
    <button id="open">Open Git Log</button>
    <p class="status" id="status"></p>
    <script>
      'use strict'
      var statusEl = document.getElementById('status')
      function openTab() {
        var requestId = 'gl-open-' + Date.now()
        var onResult = function (event) {
          var data = event.data
          if (!data || data.type !== 'orca-panel-action-result' || data.requestId !== requestId) return
          window.removeEventListener('message', onResult)
          statusEl.textContent = data.ok ? 'Opened.' : ('Failed: ' + (data.error || data.errorCode))
        }
        window.addEventListener('message', onResult)
        window.parent.postMessage({ type: 'orca-panel-action', requestId: requestId, action: 'workspace.openTab', params: { tabId: 'git-log' } }, '*')
        statusEl.textContent = 'Opening…'
      }
      document.getElementById('open').addEventListener('click', openTab)
    </script>
  </body>
</html>
```

- [ ] **Step 4: 运行测试确认通过**

Run: `pnpm test plugins/git-log/src/launcher.test.ts`
Expected: PASS

- [ ] **Step 5: 写 README（dev 加载）**

Create `plugins/git-log/README.md`:

```markdown
# Git Log 插件（增量 1：fixture UI）

零后端、零能力表改动的 IDEA VcsLog 风格提交图谱预览。数据来自内嵌 fixture。

## 本地加载（tauri dev，真实桥）
1. `pnpm build:git-log`（产出 `dist/tab.html`，全内联；`dist` 不入库）。
2. 运行 `pnpm dev`（**须用真实桥**：mock 桥用固定 fixture，不反映本插件）。
3. 设置 → 插件 → 开发：`devPluginPaths` 指向 `plugins/git-log`。
4. 插件中心启用；consent 仅 `workspace:open-tab`。
5. 右侧栏出现 "Git Log" 启动器 → 点击打开主区 Git Log tab。
6. 改 UI 后重跑 `pnpm build:git-log`，dev watcher 会重新发现。

## 增量 2（后续）
`ChannelRepository` 经 worker 通道 spawn git 取真实数据；本增量仅留类型占位。
```

- [ ] **Step 6: Commit**

```bash
git add plugins/git-log/launcher.html plugins/git-log/src/launcher.test.ts plugins/git-log/README.md
git commit -m "feat(git-log): launcher openTab 回执 + dev 加载文档"
```

---

## Task 7: 集成门禁 + 后端隔离确认

**Files:**
- 无新建（运行门禁；如门禁暴露问题就近修复）

**Interfaces:**
- Consumes: Task 1–6 全部产物
- Produces: 绿色自动化门禁证据 + `cargo` 未受影响说明

- [ ] **Step 1: 全量自动化门禁**

Run: `pnpm typecheck && pnpm build:web && pnpm build:git-log && pnpm test`
Expected: 全部 PASS。`pnpm test` 含全部插件测试（build-inline / theme-parity / snapshot-shape / git-graph-model / format / git-log-view / launcher）。

- [ ] **Step 2: 确认未触碰后端与能力表**

Run: `git diff --name-only main...HEAD`
Expected: 变更文件仅在 `plugins/git-log/**`、`package.json`、`vitest.config.ts`、`.gitignore`、`docs/**`。**不得**出现 `src/shared/plugins/plugin-capability-gate(s).ts`、`plugin-host-api.ts`、`src-tauri/crates/ade-plugins/src/host/methods.rs`、`src/renderer/src/components/right-sidebar/source-control/**`、`src/shared/git-history*.ts`。

Run: `cargo test --workspace`（可选、耗时）
Expected: 若执行，全绿（本增量无 Rust 改动，结果与 `main` 一致）；在提交信息/记录中注明"增量 1 不触后端，cargo 门禁与 main 等价"。

- [ ] **Step 3: 手工验收清单（用户在 tauri dev 中过一遍）**

- [ ] dev 加载 → 右侧栏出现 "Git Log" 启动器（consent 仅 `workspace:open-tab`）。
- [ ] 点启动器 → 主区打开 Git Log tab，图谱/分支面板/详情区渲染 fixture。
- [ ] 搜索框过滤减少行；点 refs 面板过滤；↑/↓ 换选中；选中行详情显示 subject + 文件列表。
- [ ] 切宿主 light/dark → tab 与宿主同屏对照一致（图谱色随插件 `:root`/`.dark` token 切换）。
- [ ] ≥30 行虚拟滚动流畅，无明显卡顿。

- [ ] **Step 4: 记录并收尾提交（如以上暴露修复）**

```bash
git add -A
git commit -m "chore(git-log): 增量 1 集成门禁通过，确认零后端/零能力表改动"
```

---

## Self-Review（规格 → 任务覆盖）

**1. Spec coverage（对照规格逐节）：**
- §2.1 实现面：插件包/manifest/UI 框架/数据缝/图谱逻辑/打开入口/根接线 → Task 1（骨架+manifest+根接线）、Task 3（数据缝+fixture+`ChannelRepository` 占位）、Task 4（图谱逻辑复用 shared）、Task 5（UI 框架+视图）、Task 6（打开入口 launcher→openTab）。✓
- §2.2 不做：无 capability/host 方法/worker/真实数据/过滤执行/i18n/copy-hash/改现有 git → Task 7 Step 2 以 `git diff --name-only main...HEAD` 白名单强制；fixture 本地过滤（非真查询）在 Task 5；i18n 不做（v1 英文内嵌）。✓
- §3 视图分区（工具条/分支面板/图谱列表/详情/键盘/light-dark）→ Task 5 全覆盖；分支标签来自 `snapshot.currentRef`（不调 readContext）。✓
- §4 数据缝与 fixture（`GitLogRepository`、`GitLogSnapshot` 对齐 `GitHistoryResult`、≥30 提交、merge/tag/remote/ahead-behind）→ Task 3，测试锁 ≥30 + merge + tag + remote + topo。✓
- §5 构建与视觉一致（内联单文件、`@theme inline` 逐字 + parity、Geist 内联 data URI、tsconfig 禁 node types、dev 流程）→ Task 1（`inlineSingleFile`+CSP 断言）、Task 2（parity + git-graph tokens + 字体）、Task 1 Step 7（tsconfig `types:[]`）、Task 6 README（dev）。✓
- §6 测试与验收 → 各 Task TDD + Task 7 门禁/手工清单。✓
- §7 风险：抢改根接线（Task 7 白名单）、bundle 体积（构建打印 KiB；IIFE+minify；单 tab <10MiB）、增量 2 依赖（`ChannelRepository` 仅类型）、面板拿不到路径（全 fixture）、i18n 不可复用（v1 英文）。✓

**2. Placeholder 扫描：** 唯一显式占位是 `@theme inline { /* PASTE VERBATIM HERE */ }`，其内容被 Task 2 Step 3 的"逐字复制 main.css 块"动作填充，并由 Step 1 的 parity 测试硬校验；非 TBD。Task 1 的 tab-main/theme 占位在 Task 2/5 被替换为真实实现。其余步骤均含真实代码。✓

**3. 类型一致性：**
- `GitLogSnapshot = GitHistoryResult`（Task 3）在 Task 4/5 一致使用。
- `buildGraphRows(snapshot): GraphRow[]`、`GraphRow = { viewModel, laneIndex }`（Task 4）在 Task 5 `GraphList`/`GraphLane`/`GitLogView` 一致引用。
- `graphColorVar(colorId)`（Task 4）用于 Task 5 `GraphLane`。
- `GitLogRepository.loadCommitFiles(oid): Promise<GitBranchChangeEntry[]>`（Task 3）与 Task 5 `CommitDetail`/`GitLogView` 一致。
- `workspace.openTab` `params:{tabId}`（Task 6 launcher）匹配 `workspaceOpenTabParams`（`tabId` 必填）。✓

**4. 范围：** 单插件包、单可合可演示增量 → 单一实施计划合适。✓
