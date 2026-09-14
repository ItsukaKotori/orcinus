# ade Phase 0（Tauri 骨架 + UI）实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 在 `itsuka-orca/ade` 新仓库中建成 Tauri v2 骨架，fork Orca 渲染层并移除 G/I 顶层 UI，打通 `window.api` bridge mock，交付最左全局 rail、插件中心页、右侧项目级插件作用域过滤、简化设置页，并完成 CEF 打包与 PTY 吞吐两个 spike 的书面结论。

**Architecture:** 渲染层整包 fork（保持 `src/renderer` ↔ `src/shared` 相对布局），`window.api` 类型来自 fork 的 `src/preload/api-types.ts`，由 `src/bridge` 以 TypeScript 提供 mock 实现（未实现方法经 Proxy 兜底报错）。Rust 侧从 `src-tauri` workspace 起步，Phase 0 只建 `ade-pty` crate 与独立 spike 工程；所有真实后端能力留待 Phase 1+。

**Tech Stack:** Tauri v2（Windows + macOS）、Rust（Cargo workspace）、React 19、Tailwind 4、shadcn/Radix UI、Vite（rolldown-vite）、TypeScript、Vitest + happy-dom、pnpm。

**Spec:** `docs/superpowers/specs/2026-09-14-ade-design.md`（执行者需通读；本计划是该规格 Phase 0 的落地）

## Global Constraints

- 平台：Windows + macOS；不引入 Linux-only 假设与依赖。
- 禁止 Electron：`src/renderer`、`src/shared`、`src/bridge`、`src/preload` 不得 `import 'electron'` 或引用 Electron 运行时；发现残留引用即修。
- fork 布局：`src/renderer/...` 与 `src/shared/...` 的相对位置必须与 orca 一致（5,288 处深层相对导入依赖此布局），不得重排目录。
- 依赖版本以 `../orca-main/orca-main/package.json` 为准（逐个复制具体版本号），Tauri 相关新增依赖除外。
- 删除范围（Phase 0）：只移除 G/I 的**用户可见入口**（顶层视图、对话框、设置分组、导航项、快捷键默认项）；不做全量死代码清扫（Task 10 限时进行，其余记入清单留待 Phase 1）。
- 插件 manifest 名 `ade-plugin.json`，兼容读取 `orca-plugin.json`（Phase 3 实现；Phase 0 仅 mock 数据中出现 `scope` 字段）。
- 代码风格：遵循 `../orca-main/orca-main/AGENTS.md`——注释只写非显然的 WHY、每行尽量 1 句；不给 `max-lines` 加豁免；文件名用具业务含义的名称，禁止 `helpers`/`utils`；类型断言仅允许 `as const`，其余需 `SAFETY:` 行注释。
- 每个 Task 结束必须提交一次（Conventional Commits）；提交前跑该 Task 的验证命令。
- 命令均在 `ade/` 目录执行；orca 参照路径为 `../orca-main/orca-main`。
- 验证命令速查：`pnpm test [path]`、`pnpm typecheck`、`pnpm build:web`、`cargo check --manifest-path src-tauri/Cargo.toml`、`cargo test -p <crate>`。

---

## File Structure（Phase 0 结束时）

```
ade/
├── package.json / pnpm-lock.yaml / vite.config.ts / vitest.config.ts / tsconfig.json
├── index 相关：src/renderer/index.html（fork 自带）+ src/renderer/web-index.html（弃用可留）
├── src/
│   ├── bridge/                    # window.api 适配层
│   │   ├── create-api.ts          # 组合 mock 域，产出 PreloadApi
│   │   ├── install.ts             # window.api = createAdeApi()
│   │   ├── unimplemented-fallback.ts
│   │   └── mock/{app,settings,plugins,worktrees,ui}-api.ts
│   ├── renderer/                  # fork 自 orca
│   │   └── src/
│   │       ├── components/global-rail/{GlobalActivityRail,GlobalPluginHostView}.tsx
│   │       ├── components/plugin-center/{PluginCenterPage,PluginCenterInstalledList,...}.tsx
│   │       └── store/slices/plugin-center/ # 插件中心状态 slice（mock 数据源）
│   ├── shared/                    # fork 自 orca（含 TopLevelView 扩展）
│   └── preload/                   # fork 自 orca（仅取类型：api-types.ts + api/**）
└── src-tauri/
    ├── Cargo.toml                 # package ade-app + workspace members = ["crates/*"]
    ├── build.rs / tauri.conf.json / capabilities/default.json / icons/
    ├── src/{main.rs,lib.rs}
    └── crates/ade-pty/{Cargo.toml,src/lib.rs,src/bin/pty_bench.rs,tests/throughput.rs}
spikes/
└── cef-embed/                     # 独立 Cargo 工程（不入 workspace）
docs/
├── spikes/2026-09-14-pty-throughput.md   # PTY spike 结论
├── spikes/2026-09-14-cef-packaging.md    # CEF spike 结论
├── phase0-dead-code-inventory.md
└── phase0-acceptance.md
```

---

### Task 1: Tauri v2 骨架与最小渲染入口

**Files:**
- Create: `package.json`, `.gitignore`, `vite.config.ts`, `vitest.config.ts`, `tsconfig.json`
- Create: `src/renderer/index.html`, `src/renderer/src/main.tsx`, `src/renderer/src/PlaceholderApp.tsx`, `src/renderer/src/placeholder-smoke.test.tsx`
- Create: `src-tauri/Cargo.toml`, `src-tauri/build.rs`, `src-tauri/src/main.rs`, `src-tauri/src/lib.rs`, `src-tauri/tauri.conf.json`, `src-tauri/capabilities/default.json`
- Create: `resources/icon.png`（复制自 `../orca-main/orca-main/resources/build/icon.png`）

**Interfaces:**
- Produces: `pnpm dev`（tauri dev）、`pnpm build:web`（vite 构建渲染层）、`pnpm typecheck`、`pnpm test`；`src-tauri` 为 Cargo workspace 根（后续 crate 加入 `crates/*`）。

- [ ] **Step 1: 写渲染层冒烟测试（先失败）**

`src/renderer/src/placeholder-smoke.test.tsx`：

```tsx
// @vitest-environment happy-dom
import { render, screen } from '@testing-library/react'
import { describe, expect, it } from 'vitest'
import { PlaceholderApp } from './PlaceholderApp'

describe('placeholder root', () => {
  it('renders the ade placeholder', () => {
    render(<PlaceholderApp />)
    expect(screen.getByText('ade')).toBeTruthy()
  })
})
```

- [ ] **Step 2: 运行测试确认失败**

Run: `pnpm dlx vitest@4.1.11 run src/renderer/src/placeholder-smoke.test.tsx`
Expected: FAIL——`Cannot find module './PlaceholderApp'`（此刻仓库还没有任何配置）

- [ ] **Step 3: 写根配置与占位入口**

`package.json`（版本号取自 orca `package.json`；Tauri 用当前 2.x）：

```json
{
  "name": "ade",
  "private": true,
  "version": "0.0.1",
  "type": "module",
  "scripts": {
    "dev": "tauri dev",
    "build": "tauri build",
    "build:web": "vite build",
    "typecheck": "tsc --noEmit -p tsconfig.json",
    "test": "vitest run",
    "test:watch": "vitest",
    "tauri": "tauri"
  },
  "dependencies": {
    "@tauri-apps/api": "^2"
  },
  "devDependencies": {
    "@tailwindcss/vite": "^4.2.4",
    "@tauri-apps/cli": "^2",
    "@testing-library/react": "^16.3.2",
    "@types/react": "^19.2.17",
    "@types/react-dom": "^19.2.3",
    "@vitejs/plugin-react": "^5.2.0",
    "happy-dom": "^20.11.8",
    "react": "^19.2.8",
    "react-dom": "^19.2.8",
    "tailwindcss": "^4.2.4",
    "typescript": "^7.0.2",
    "vite": "npm:rolldown-vite@7.3.1",
    "vitest": "^4.1.11"
  }
}
```

`.gitignore`：

```
node_modules/
dist/
src-tauri/target/
src-tauri/gen/
*.local
```

`vite.config.ts`（结构参照 `../orca-main/orca-main/vite.web.config.ts`）：

```ts
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
```

`vitest.config.ts`：

```ts
import { resolve } from 'node:path'
import { defineConfig } from 'vitest/config'

export default defineConfig({
  resolve: {
    alias: {
      '@renderer': resolve('src/renderer/src'),
      '@': resolve('src/renderer/src')
    }
  },
  test: {
    include: ['src/**/*.test.ts', 'src/**/*.test.tsx'],
    hookTimeout: 60_000,
    testTimeout: 30_000
  }
})
```

`tsconfig.json`（Phase 0 先覆盖 bridge 与新增文件；Task 2 扩展到 fork 全量）：

```json
{
  "compilerOptions": {
    "target": "ES2022",
    "module": "ESNext",
    "moduleResolution": "bundler",
    "strict": true,
    "jsx": "react-jsx",
    "skipLibCheck": true,
    "noEmit": true,
    "types": ["vite/client"],
    "baseUrl": ".",
    "paths": {
      "@renderer/*": ["src/renderer/src/*"],
      "@/*": ["src/renderer/src/*"]
    }
  },
  "include": ["src/renderer/src", "src/bridge", "src/preload", "src/shared"]
}
```

`src/renderer/index.html`：

```html
<!doctype html>
<html lang="en">
  <head>
    <meta charset="UTF-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1.0" />
    <title>ade</title>
  </head>
  <body>
    <div id="root"></div>
    <script type="module" src="/src/main.tsx"></script>
  </body>
</html>
```

`src/renderer/src/PlaceholderApp.tsx`：

```tsx
export function PlaceholderApp(): React.JSX.Element {
  return <main>ade</main>
}
```

`src/renderer/src/main.tsx`：

```tsx
import { createRoot } from 'react-dom/client'
import { PlaceholderApp } from './PlaceholderApp'

createRoot(document.getElementById('root') as HTMLElement).render(<PlaceholderApp />)
```

- [ ] **Step 4: 运行测试确认通过**

Run: `pnpm install && pnpm test src/renderer/src/placeholder-smoke.test.tsx`
Expected: PASS（1 passed）

- [ ] **Step 5: 写 Tauri 工程**

`src-tauri/Cargo.toml`：

```toml
[package]
name = "ade-app"
version = "0.0.1"
edition = "2021"

[workspace]
members = ["crates/*"]

[lib]
name = "ade_app"
crate-type = ["staticlib", "cdylib", "rlib"]

[build-dependencies]
tauri-build = { version = "2", features = [] }

[dependencies]
tauri = { version = "2", features = [] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
```

`src-tauri/build.rs`：

```rust
fn main() {
    tauri_build::build()
}
```

`src-tauri/src/lib.rs`：

```rust
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .run(tauri::generate_context!())
        .expect("error while running ade");
}
```

`src-tauri/src/main.rs`：

```rust
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    ade_app::run()
}
```

`src-tauri/tauri.conf.json`：

```json
{
  "$schema": "https://schema.tauri.app/config/2",
  "productName": "ade",
  "version": "0.0.1",
  "identifier": "dev.itsuka.ade",
  "build": {
    "beforeDevCommand": "pnpm vite",
    "devUrl": "http://127.0.0.1:1420",
    "beforeBuildCommand": "pnpm build:web",
    "frontendDist": "../dist"
  },
  "app": {
    "windows": [{ "title": "ade", "width": 1440, "height": 900 }],
    "security": { "csp": null }
  },
  "bundle": {
    "active": true,
    "targets": "all",
    "icon": [
      "icons/32x32.png",
      "icons/128x128.png",
      "icons/128x128@2x.png",
      "icons/icon.icns",
      "icons/icon.ico"
    ]
  }
}
```

`src-tauri/capabilities/default.json`：

```json
{
  "identifier": "default",
  "description": "ade default capability",
  "windows": ["main"],
  "permissions": ["core:default"]
}
```

生成图标：

```bash
mkdir -p resources
cp ../orca-main/orca-main/resources/build/icon.png resources/icon.png
pnpm tauri icon resources/icon.png
```

- [ ] **Step 6: 验证 Tauri 工程可编译、可打包（无 bundle）**

Run: `cargo check --manifest-path src-tauri/Cargo.toml`
Expected: `Finished`（0 errors）

Run: `pnpm build:web && pnpm tauri build --debug --no-bundle`
Expected: 生成 `src-tauri/target/debug/ade-app.exe`（Windows）；无编译错误

- [ ] **Step 7: 人工冒烟（可后台）**

Run: `pnpm dev`
Expected: 打开 1440×900 的 ade 窗口，显示 `ade`；确认后关闭。
（若环境无显示器，记录未执行，交由验收步骤人工补测。）

- [ ] **Step 8: Commit**

```bash
git add package.json .gitignore vite.config.ts vitest.config.ts tsconfig.json resources/ src/ src-tauri/
git commit -m "feat: Tauri v2 骨架与最小渲染入口"
```

---

### Task 2: fork 渲染层 / shared / preload 类型并适配构建

**Files:**
- Create（拷贝）: `src/renderer/**`、`src/shared/**`、`src/preload/**`、`src/types/**`（来自 `../orca-main/orca-main/src/*`），以及按需从 `../orca-main/orca-main/src/main/**` 复制的**类型依赖**文件
- Modify: `package.json`（依赖清单以 orca 为基线）、`vite.config.ts`、`vitest.config.ts`、`tsconfig.json`
- Delete: `src/renderer/src/PlaceholderApp.tsx`、`src/renderer/src/placeholder-smoke.test.tsx`（被真实入口取代）

**Interfaces:**
- Consumes: Task 1 的构建配置与脚本。
- Produces: `src/renderer`（真实 Orca 渲染层）、`src/shared`（全量共享类型/逻辑）、`src/preload/api-types.ts` + `src/preload/api/**`（`PreloadApi` 契约）。

- [ ] **Step 1: 拷贝 fork 内容（保留相对布局）**

```bash
cp -r ../orca-main/orca-main/src/renderer src/renderer
cp -r ../orca-main/orca-main/src/shared src/shared
cp -r ../orca-main/orca-main/src/preload src/preload
cp -r ../orca-main/orca-main/src/types src/types
rm src/renderer/src/PlaceholderApp.tsx src/renderer/src/placeholder-smoke.test.tsx
```

- [ ] **Step 2: 以 orca 为基线合并依赖，删除 Electron/主进程专用依赖**

编辑 `package.json`：把 `dependencies`/`devDependencies` 替换为 orca `package.json` 的对应内容，然后删除以下条目（保留其余全部，版本号原样）：

- `dependencies` 删除：`@anthropic-ai/claude-agent-sdk`、`@electron-toolkit/utils`、`@linear/sdk`、`@parcel/watcher`、`agent-browser`、`electron-updater`、`node-pty`、`posthog-node`、`proper-lockfile`、`qrcode`、`serve-sim`、`sherpa-onnx`、`ssh2`、`tweetnacl`、`ws`（渲染层使用浏览器原生 WebSocket；若构建报缺再恢复）
- `devDependencies` 删除：`@electron-toolkit/tsconfig`、`@electron/rebuild`、`@playwright/test`、`@stablyai/playwright-test`、`@types/proper-lockfile`、`@types/qrcode`、`@types/ssh2`、`@types/ws`、`electron`、`electron-builder`、`electron-builder-squirrel-windows`、`electron-vite`、`husky`、`lint-staged`
- `optionalDependencies` 整段删除；`lint-staged`、`reactDoctor` 配置段删除
- 保留 `@tauri-apps/api`、`@tauri-apps/cli`（Task 1 新增）
- 新增 orca 版本相同的 `@tailwindcss/vite`、`tailwindcss`（Task 1 已加，核对版本）

Run: `pnpm install`
Expected: 安装成功；若 pnpm 提示忽略构建脚本，记录并执行 `pnpm approve-builds`（仅允许 `esbuild`、`@tailwindcss/oxide` 之类构建型包）

- [ ] **Step 3: 迁移 tsconfig（参照 orca `config/tsconfig.tc.web.json`）**

把 orca 的 `config/tsconfig.web.json` 复制为 `config/tsconfig.web.json`（保持 extends 链所需文件），并把 orca `config/tsconfig.tc.web.json` 复制为 `tsconfig.json` 后修改 `include` 相对路径（去掉 `../` 前缀，指向 `src/...`）。初始 include 集合：

```json
{
  "include": [
    "src/renderer/src/env.d.ts",
    "src/renderer/src/**/*",
    "src/preload/api-types.ts",
    "src/preload/api/**/*",
    "src/shared/**/*"
  ]
}
```

Run: `pnpm typecheck`
Expected: 报出第一处 `Cannot find module`（来自被引用的 `src/main/...` 文件）

- [ ] **Step 4: 逐轮补齐类型依赖（机械循环）**

规则：每轮 `pnpm typecheck` 报出的缺失模块若位于 `../orca-main/orca-main/src/main/` 下，就把该文件原样复制到 ade 对应路径（`mkdir -p` 后 `cp`），直到 typecheck 不再报主进程缺失。示例（第一轮常见）：

```bash
mkdir -p src/main/ipc src/main/gitlab
cp ../orca-main/orca-main/src/main/ipc/worktree-logic.ts src/main/ipc/
cp ../orca-main/orca-main/src/main/gitlab/mappers.ts src/main/gitlab/
```

已知初始集合（来自 orca `config/tsconfig.tc.web.json` 的 include；可一次性复制，缺谁的依赖再补谁）：

```
src/main/gitlab/mappers.ts
src/main/ipc/deferred-emoji-shortcode-dataset.ts
src/main/ipc/worktree-branch-name.ts
src/main/ipc/worktree-logic.ts
src/main/ipc/worktree-display-name.ts
src/main/ipc/worktree-linked-work-item-metadata.ts
src/main/ipc/worktree-metadata-merge.ts
src/main/ipc/worktree-path-comparison.ts
src/main/wsl-availability.ts
src/main/wsl-directory-probe-command.ts
src/main/wsl-distro-list-output.ts
src/main/wsl-distro-retry.ts
src/main/wsl-running-distro-cache.ts
src/main/wsl.ts
src/main/wsl-interop-spawn-directory.ts
src/main/persistence/applying-settings/ui-state-read.ts
src/main/persistence/applying-settings/ui-state-update.ts
src/main/persistence/applying-settings/ui-selection-normalization.ts
src/main/persistence/applying-settings/ui-interaction-merge.ts
src/main/protected-secret-persistence.ts
src/main/startup/serve-desktop-activation.ts
src/main/startup/serve-mode-argv.ts
src/main/startup/single-instance-lock.ts
src/main/startup/startup-diagnostics.ts
src/main/window/focus-existing-window.ts
src/main/window/foreground-activation-policy.ts
src/main/window/macos-app-activation.ts
```

约束：只允许复制 **type-only 依赖链**上的文件；若某文件 import `electron` 导致类型失败，创建 `src/types/electron-type-shim.d.ts`，按报错把用到的 Electron 类型声明为最小别名（如 `declare module 'electron' { export type BrowserWindow = unknown }`），并在文件头注明 `// Phase 0 typecheck shim; removed in Phase 1`。

- [ ] **Step 5: 适配 vitest 配置（跑通既有渲染层测试）**

把 `vitest.config.ts` 的 `test` 段替换为 orca `config/vitest.config.ts` 的等价配置（include `src/**/*.test.ts(x)`、`hookTimeout: 60_000`、`testTimeout: 30_000`、Windows `maxWorkers: 4`），并复制 orca 的三个 setup 文件到 `config/scripts/`：

```bash
mkdir -p config/scripts
cp ../orca-main/orca-main/config/scripts/happy-dom-offscreen-canvas.ts config/scripts/
cp ../orca-main/orca-main/config/scripts/happy-dom-mutation-observer-retention.ts config/scripts/
cp ../orca-main/orca-main/config/scripts/vitest-host-ports-setup.ts config/scripts/
```

若这三个 setup 文件存在主进程依赖，改为在 vitest 配置中逐项启用直到测试可跑。

- [ ] **Step 6: 验证构建与抽样测试**

Run: `pnpm build:web`
Expected: `built in ...s`，无 rollup 解析错误

Run: `pnpm test src/shared/top-level-view` 与 `pnpm test src/renderer/src/components/sidebar/SidebarNav.test.tsx`
Expected: PASS（若因 G/I 功能断言失败，记录到 Task 4 处理，不得现在删测试）

- [ ] **Step 7: Commit**

```bash
git add -A
git commit -m "feat: fork Orca 渲染层/shared/preload 类型并适配构建"
```

---

### Task 3: bridge 契约层与 mock 骨架

**Files:**
- Create: `src/bridge/create-api.ts`、`src/bridge/install.ts`、`src/bridge/unimplemented-fallback.ts`、`src/bridge/unimplemented-fallback.test.ts`、`src/bridge/create-api.test.ts`
- Create: `src/bridge/mock/app-api.ts`、`src/bridge/mock/settings-api.ts`、`src/bridge/mock/plugins-api.ts`、`src/bridge/mock/worktrees-api.ts`、`src/bridge/mock/ui-api.ts`、`src/bridge/mock/fixtures.ts`
- Modify: `src/renderer/src/main.tsx`（安装 bridge）

**Interfaces:**
- Consumes: `src/preload/api-types.ts` 的 `PreloadApi`；fork 渲染层启动路径对 `window.api.*` 的调用。
- Produces: `createAdeApi(): PreloadApi`、`installAdeBridge(): void`；mock 数据源（插件含 `scope` 字段）供 Task 5/6/7 使用；`UnimplementedBridgeError`（含 `path: string` 字段）。

- [ ] **Step 1: 摸清启动期实际调用的 API 面**

Run:

```bash
grep -rhoE "window\.api\.[a-zA-Z]+\.[a-zA-Z]+" src/renderer/src --include="*.ts" --include="*.tsx" \
  | sort | uniq -c | sort -rn | head -40
```

Expected: 得到高频调用清单（如 `window.api.settings.get`、`window.api.app.getPlatformInfo`、`window.api.ui.*`）。把启动路径（`src/renderer/src/main.tsx` → `App.tsx` → `AppWorkspaceShell` 直达的 hook）引用到的域记为 Phase 0 必 mock 域。

- [ ] **Step 2: 写 fallback 测试（先失败）**

`src/bridge/unimplemented-fallback.test.ts`：

```ts
import { describe, expect, it, vi } from 'vitest'
import { withUnimplementedFallback, UnimplementedBridgeError } from './unimplemented-fallback'

describe('withUnimplementedFallback', () => {
  it('passes through implemented namespaces', async () => {
    const api = withUnimplementedFallback({ app: { ping: async () => 'pong' } })
    await expect(api.app.ping()).resolves.toBe('pong')
  })

  it('rejects unknown namespace methods with a tagged error', async () => {
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => {})
    const api = withUnimplementedFallback({})
    await expect(api.files.readFile({})).rejects.toBeInstanceOf(UnimplementedBridgeError)
    expect(warn).toHaveBeenCalledWith(expect.stringContaining('files.readFile'))
  })
})
```

- [ ] **Step 3: 运行测试确认失败**

Run: `pnpm test src/bridge/unimplemented-fallback.test.ts`
Expected: FAIL——`Cannot find module './unimplemented-fallback'`

- [ ] **Step 4: 实现 fallback**

`src/bridge/unimplemented-fallback.ts`：

```ts
export class UnimplementedBridgeError extends Error {
  readonly path: string
  constructor(path: string) {
    super(`ade bridge method not implemented yet: ${path}`)
    this.name = 'UnimplementedBridgeError'
    this.path = path
  }
}

function createNamespace(prefix: string): Record<string, unknown> {
  const methods = new Map<string, (...args: unknown[]) => Promise<never>>()
  return new Proxy(
    {},
    {
      get(_target, property: string): unknown {
        if (!methods.has(property)) {
          methods.set(property, async (...args: unknown[]) => {
            console.warn(`[ade:bridge] unimplemented call ${prefix}.${property}`, args)
            throw new UnimplementedBridgeError(`${prefix}.${property}`)
          })
        }
        return methods.get(property)
      }
    }
  )
}

export function withUnimplementedFallback<T extends object>(partial: Partial<T>): T {
  const namespaces = new Map<string, unknown>()
  return new Proxy(partial as T, {
    get(target, property: string): unknown {
      const existing = (target as Record<string, unknown>)[property]
      if (existing !== undefined) return existing
      if (!namespaces.has(property)) namespaces.set(property, createNamespace(property))
      return namespaces.get(property)
    }
  })
}
```

- [ ] **Step 5: 运行测试确认通过**

Run: `pnpm test src/bridge/unimplemented-fallback.test.ts`
Expected: PASS（2 passed）

- [ ] **Step 6: 写 mock 域与组合**

`src/bridge/mock/fixtures.ts`（供 Task 5/6/7 复用）：

```ts
import type { PluginHostListEntry } from '../../preload/api/plugin-host-api'

export type PluginScope = 'global' | 'project'
export type PluginCenterEntry = PluginHostListEntry & { scope: PluginScope }

export const MOCK_GLOBAL_PLUGINS: PluginCenterEntry[] = [
  {
    pluginKey: 'database-manager',
    name: 'Database Manager',
    version: '0.1.0',
    publisher: 'ade-labs',
    description: 'Browse and query local databases',
    scope: 'global',
    consentFingerprint: 'mock-fingerprint-db',
    needsReconsent: false,
    status: 'idle',
    isDev: false,
    official: false,
    bundled: false,
    capabilities: [{ kind: 'workspace:read', description: 'Read workspace metadata' }],
    panels: [{ id: 'database-manager.main', title: 'Database', icon: 'database', tabKey: 'plugin:database-manager.main' }],
    commands: [],
    hasWorker: true,
    restarts: 0
  },
  {
    pluginKey: 'api-tester',
    name: 'API Tester',
    version: '0.2.0',
    publisher: 'ade-labs',
    description: 'Send HTTP requests from a panel',
    scope: 'global',
    consentFingerprint: 'mock-fingerprint-api',
    needsReconsent: false,
    status: 'idle',
    isDev: false,
    official: false,
    bundled: false,
    capabilities: [{ kind: 'storage', description: 'Store requests in the plugin folder' }],
    panels: [{ id: 'api-tester.main', title: 'API Tester', icon: 'plug', tabKey: 'plugin:api-tester.main' }],
    commands: [],
    hasWorker: true,
    restarts: 0
  }
]

export const MOCK_PROJECT_PLUGINS: PluginCenterEntry[] = [
  {
    ...MOCK_GLOBAL_PLUGINS[0],
    pluginKey: 'repo-notes',
    name: 'Repo Notes',
    version: '0.1.0',
    publisher: 'my-team',
    description: 'Project-scoped notes panel',
    scope: 'project',
    consentFingerprint: 'mock-fingerprint-notes',
    panels: [{ id: 'repo-notes.main', title: 'Repo Notes', icon: 'filetext', tabKey: 'plugin:repo-notes.main' }]
  }
]
```

`src/bridge/mock/plugins-api.ts`：

```ts
import type { PluginsApi } from '../../preload/api/plugin-host-api'
import { MOCK_GLOBAL_PLUGINS, MOCK_PROJECT_PLUGINS, type PluginCenterEntry } from './fixtures'

export function createPluginsApi(): PluginsApi {
  let state: PluginCenterEntry[] = [...MOCK_GLOBAL_PLUGINS, ...MOCK_PROJECT_PLUGINS]
  return {
    list: async () => state,
    listLanguagePacks: async () => [],
    consent: async () => state,
    setEnabled: async ({ pluginKey, enabled }) => {
      state = state.map((entry) =>
        entry.pluginKey === pluginKey
          ? { ...entry, status: enabled ? 'idle' : 'disabled' }
          : entry
      )
      return state
    },
    readPanelEntry: async () => null,
    invokeCommand: async () => undefined,
    panelAction: async () => ({ ok: true }),
    install: async () => ({ pluginKey: 'mock-installed', needsReconsent: true, consentFingerprint: 'mock' }),
    listMarketplaces: async () => [],
    addMarketplace: async () => undefined,
    removeMarketplace: async () => [],
    refreshMarketplaces: async () => [],
    listMarketplacePlugins: async () => [],
    previewMarketplacePlugin: async () => undefined,
    installMarketplacePlugin: async () => ({ pluginKey: 'mock-marketplace', needsReconsent: true, consentFingerprint: 'mock' }),
    previewMarketplaceUpdate: async () => undefined,
    rollbackMarketplacePlugin: async () => ({ pluginKey: 'mock-rollback', needsReconsent: true, consentFingerprint: 'mock' }),
    remove: async ({ pluginKey }) => {
      state = state.filter((entry) => entry.pluginKey !== pluginKey)
      return state
    },
    getLogs: async () => [{ ts: Date.now(), level: 'info', line: 'mock log line' }],
    refresh: async () => state
  }
}
```

（若类型字段/返回形状与 `src/preload/api/plugin-host-api.ts` 不一致，以该文件为准修正 mock——类型检查会强制。）

`src/bridge/mock/app-api.ts`、`settings-api.ts`、`worktrees-api.ts`、`ui-api.ts`：按 Step 1 清单，用同样的「读取类型文件 → 实现最小返回」方式补全启动必需成员；每个文件顶部注释一行 `// Phase 0 mock; replaced by Tauri IPC per view.`。settings mock 返回 6.4 节的简化分组数据；worktrees mock 返回 2 条示例工作区。

`src/bridge/create-api.ts`：

```ts
import type { PreloadApi } from '../preload/api-types'
import { createAppApi } from './mock/app-api'
import { createPluginsApi } from './mock/plugins-api'
import { createSettingsApi } from './mock/settings-api'
import { createUiApi } from './mock/ui-api'
import { createWorktreesApi } from './mock/worktrees-api'
import { withUnimplementedFallback } from './unimplemented-fallback'

export function createAdeApi(): PreloadApi {
  // SAFETY: mock 域覆盖 Phase 0 启动与 UI 所需命名空间；其余经 Proxy 兜底为“未实现”拒绝，
  // 因此断言为 PreloadApi 在运行期仍保持“调用必为函数”的契约。
  const partial: Partial<PreloadApi> = {
    app: createAppApi(),
    plugins: createPluginsApi(),
    settings: createSettingsApi(),
    ui: createUiApi(),
    worktrees: createWorktreesApi()
  }
  return withUnimplementedFallback(partial)
}
```

`src/bridge/install.ts`：

```ts
import { createAdeApi } from './create-api'

export function installAdeBridge(): void {
  window.api = createAdeApi()
}
```

- [ ] **Step 7: 写组合测试**

`src/bridge/create-api.test.ts`：

```ts
import { describe, expect, it } from 'vitest'
import { createAdeApi } from './create-api'

describe('createAdeApi', () => {
  it('exposes mocked namespaces with live data', async () => {
    const api = createAdeApi()
    const plugins = await api.plugins.list()
    expect(plugins.some((entry) => entry.scope === 'global')).toBe(true)
    expect(plugins.some((entry) => entry.scope === 'project')).toBe(true)
  })
})
```

Run: `pnpm test src/bridge/create-api.test.ts`
Expected: PASS

- [ ] **Step 8: 在渲染入口安装 bridge**

`src/renderer/src/main.tsx` 顶部（在 App lazy import 触发前）：

```tsx
import { installAdeBridge } from '../../bridge/install'
installAdeBridge()
```

Run: `pnpm build:web`
Expected: 构建通过（此时 `window.api` 在运行期由 mock 提供）

- [ ] **Step 9: Commit**

```bash
git add src/bridge src/renderer/src/main.tsx
git commit -m "feat: window.api bridge 契约层与 mock 骨架"
```

---

### Task 4: 移除 G/I 顶层 UI 面

**Files:**
- Modify: `src/shared/ui-chrome-types.ts`（`TopLevelView` 去 `'skills' | 'mobile'`）
- Modify: `src/shared/top-level-view.ts`（lookup 去 `skills`/`mobile`）
- Modify: `src/renderer/src/store/slices/ui/ui-slice-contract-core.ts`（去 `previousViewBeforeSkills`、`previousViewBeforeMobile` 等成员）
- Modify: `src/renderer/src/app-shell/AppWorkspaceShell.tsx`（去 `SkillsPage`/`MobilePage` lazy 与渲染行）
- Modify: `src/renderer/src/app-shell/AppRootSurfaces.tsx`（去 `SshPassphraseDialog`、`RemoteServerUpdateDialog`、`SkillFreshnessUpdateDialog` 的 lazy 与挂载）
- Modify: `src/renderer/src/components/settings/Settings.tsx`、`settings-page-renderer.tsx`（去 SSH/移动端/CLI/skills/VM/账号与用量分组入口）
- Modify: `src/shared/keybindings-default-bindings.ts` 及 `src/renderer/src/components/sidebar/SidebarNav.tsx`/`SidebarTaskNavButton.tsx`（去已删视图的导航与快捷键）

**Interfaces:**
- Consumes: Task 2 的 fork 代码、Task 3 的 bridge（保证移除后构建仍可跑）。
- Produces: `TopLevelView` 不再含 `skills`/`mobile`（Task 5 在其上加 `plugin-center`）。

**执行规则：** 只删“入口 + 独占组件”；被其他功能共享的模块不动。每个子步骤后跑验证。

- [ ] **Step 1: 定位全部入口引用**

Run:

```bash
grep -rln "SkillsPage\|MobilePage\|SshPassphraseDialog\|RemoteServerUpdateDialog\|SkillFreshnessUpdateDialog\|EphemeralVm\|AddRemoteHost" src/renderer/src src/shared | sort
```

Expected: 得到入口文件清单；逐一判断「独占」与「共享」。

- [ ] **Step 2: 移除顶层视图与 store 成员**

按 Files 列修改 `ui-chrome-types.ts`、`top-level-view.ts`、`ui-slice-contract-core.ts`；删除 `UiViewHistory` 中 `'skills' | 'mobile'` 成员（`grep -rn "UiViewHistory" src/shared src/renderer/src/store | head` 定位定义处）。保留 `'artifacts'`（不在删除清单）。

Run: `pnpm typecheck`
Expected: 仅剩引用已删视图的报错（来自 `AppWorkspaceShell.tsx` 等，下一步处理）

- [ ] **Step 3: 移除页面渲染与对话框挂载**

删除 `AppWorkspaceShell.tsx` 的 `SkillsPage`/`MobilePage` lazy 声明与 `{activeView === 'skills' ...}` / `{activeView === 'mobile' ...}` 行；删除 `AppRootSurfaces.tsx` 中三个 dialog 的 lazy 声明与 JSX 挂载（行号以 grep 结果为准）。独占文件（仅被这些入口 import）一并删除：

```bash
git rm src/renderer/src/components/skills/SkillsPage.tsx src/renderer/src/components/mobile/MobilePage.tsx
```

（以实际 grep 结果为准；若组件被设置页等共享，保留并在 Step 4 处理。）

- [ ] **Step 4: 移除设置页分组与导航项**

在 `components/settings/Settings.tsx` 与 `settings-page-renderer.tsx` 中删除 SSH/远程、移动端、CLI 安装、skills、ephemeral VM、账号与用量相关分组的渲染入口（分组 id 通过 grep `ssh|mobile|skills|cli|vm|account|usage` 定位）。同步删除 `SidebarNav`/`SidebarTaskNavButton`/`TitlebarLeftControls`/`TitlebarMainStrip` 中对应导航按钮，及 `keybindings-default-bindings.ts` 中对应快捷键项。

- [ ] **Step 5: 清理独占模块并跑测试**

对 Step 1 得到的每个文件执行：`grep -rl "<文件名主干>" src/renderer/src src/shared` 只剩自身时 `git rm`；否则保留（记入 Task 10 清单）。然后：

Run: `pnpm typecheck && pnpm build:web`
Expected: 0 errors，构建通过

- [ ] **Step 6: 完成度检查**

Run:

```bash
grep -rn "activeView === 'skills'\|activeView === 'mobile'\|SkillsPage\|MobilePage" src/renderer/src src/shared
```
Expected: 无输出

- [ ] **Step 7: Commit**

```bash
git add -A
git commit -m "refactor: 移除 G/I 顶层 UI 入口（视图/对话框/设置分组/导航）"
```

---

### Task 5: 最左 Global Rail 与 plugin-center 顶层视图

**Files:**
- Modify: `src/shared/ui-chrome-types.ts`（`TopLevelView` 增 `'plugin-center'`）
- Modify: `src/shared/top-level-view.ts`（lookup 增 `plugin-center: true`；validator 接受 `plugin:<key>`）
- Create: `src/renderer/src/components/global-rail/GlobalActivityRail.tsx`、`src/renderer/src/components/global-rail/GlobalPluginHostView.tsx`、`src/renderer/src/components/global-rail/global-rail-items.ts`
- Create: `src/renderer/src/components/global-rail/GlobalActivityRail.test.tsx`、`src/shared/top-level-view.test.ts`（若已存在则修改）
- Create: `src/renderer/src/store/slices/plugin-center/plugin-center-slice.ts`（含 `loadPlugins`、`plugins`、`pluginCenterStatus`）
- Modify: `src/renderer/src/app-shell/AppWorkspaceShell.tsx`（挂载 rail；非项目视图隐藏左侧 sidebar）

**Interfaces:**
- Consumes: `createAdeApi()` 的 `plugins.list()`（mock 含 `scope`）；`useAppStore` 的 `activeView`/`setActiveView`。
- Produces: `PluginCenterEntry`（来自 `src/bridge/mock/fixtures.ts`）、`usePluginCenterStore`（Task 6/7 使用）；`GlobalActivityRail` 点击行为：项目→`setActiveView('terminal')`、插件中心→`setActiveView('plugin-center')`、全局插件→`setActiveView('plugin:<pluginKey>')`。

- [ ] **Step 1: 扩展顶层视图校验（先写测试）**

`src/shared/top-level-view.test.ts`（新增或修改）：

```ts
import { describe, expect, it } from 'vitest'
import { isTopLevelView } from './top-level-view'

describe('isTopLevelView', () => {
  it('accepts plugin-center and plugin-scoped views', () => {
    expect(isTopLevelView('plugin-center')).toBe(true)
    expect(isTopLevelView('plugin:database-manager.main')).toBe(true)
  })
  it('rejects removed views and unknown prefixes', () => {
    expect(isTopLevelView('skills')).toBe(false)
    expect(isTopLevelView('mobile')).toBe(false)
    expect(isTopLevelView('plugin:')).toBe(false)
    expect(isTopLevelView('plugin:x y')).toBe(false)
  })
})
```

Run: `pnpm test src/shared/top-level-view.test.ts`
Expected: FAIL（`plugin-center` 未被接受）

- [ ] **Step 2: 实现类型与校验**

`ui-chrome-types.ts` 的 `TopLevelView` union：删除 `'skills' | 'mobile'`（Task 4 已做），新增 `| 'plugin-center'`。

`top-level-view.ts`：lookup 增 `'plugin-center': true`；validator 末尾追加：

```ts
const PLUGIN_VIEW_RE = /^plugin:[a-z0-9][a-z0-9.-]*$/

export function isTopLevelView(value: unknown): value is TopLevelView {
  if (typeof value === 'string' && PLUGIN_VIEW_RE.test(value)) return true
  return typeof value === 'string' && Object.hasOwn(TOP_LEVEL_VIEW_LOOKUP, value)
}
```

Run: `pnpm test src/shared/top-level-view.test.ts`
Expected: PASS

- [ ] **Step 3: 建插件中心状态 slice（数据来自 bridge mock）**

`src/renderer/src/store/slices/plugin-center/plugin-center-slice.ts`：

```ts
import type { StateCreator } from 'zustand'
import type { PluginCenterEntry } from '../../../../../bridge/mock/fixtures'

export type PluginCenterSlice = {
  pluginCenterEntries: PluginCenterEntry[]
  pluginCenterStatus: 'idle' | 'loading' | 'error'
  loadPluginCenterEntries: () => Promise<void>
}

export const createPluginCenterSlice: StateCreator<PluginCenterSlice> = (set) => ({
  pluginCenterEntries: [],
  pluginCenterStatus: 'idle',
  loadPluginCenterEntries: async () => {
    set({ pluginCenterStatus: 'loading' })
    try {
      // SAFETY: Phase 0 mock 在 PluginHostListEntry 上附带 scope 字段；真实 scope 字段由 Phase 3 后端补齐。
      const entries = (await window.api.plugins.list()) as PluginCenterEntry[]
      set({ pluginCenterEntries: entries, pluginCenterStatus: 'idle' })
    } catch {
      set({ pluginCenterStatus: 'error' })
    }
  }
})
```

把 slice 并入 store：`grep -n "createUiSlice\|UISlice" src/renderer/src/store/index.ts` 找到组合点，按现有模式追加。

- [ ] **Step 4: 写 rail 测试（先失败）**

`src/renderer/src/components/global-rail/GlobalActivityRail.test.tsx`：

```tsx
// @vitest-environment happy-dom
import { render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { describe, expect, it } from 'vitest'
import { GlobalActivityRail } from './GlobalActivityRail'
import { useAppStore } from '@/store'

describe('GlobalActivityRail', () => {
  it('switches to plugin-center view on click', async () => {
    render(<GlobalActivityRail />)
    await userEvent.click(screen.getByRole('button', { name: '插件中心' }))
    expect(useAppStore.getState().activeView).toBe('plugin-center')
  })

  it('renders an entry per installed global plugin', async () => {
    await useAppStore.getState().loadPluginCenterEntries()
    render(<GlobalActivityRail />)
    expect(screen.getByRole('button', { name: 'Database Manager' })).toBeTruthy()
  })
})
```

Run: `pnpm test src/renderer/src/components/global-rail/GlobalActivityRail.test.tsx`
Expected: FAIL——组件不存在

- [ ] **Step 5: 实现 rail 与插件视图占位**

`src/renderer/src/components/global-rail/global-rail-items.ts`：

```ts
import type { LucideIcon } from 'lucide-react'
import { Folder, Puzzle } from 'lucide-react'

export type GlobalRailItem =
  | { kind: 'project'; label: '项目'; icon: LucideIcon; view: 'terminal' }
  | { kind: 'plugin-center'; label: '插件中心'; icon: LucideIcon; view: 'plugin-center' }

export const GLOBAL_RAIL_ITEMS: GlobalRailItem[] = [
  { kind: 'project', label: '项目', icon: Folder, view: 'terminal' },
  { kind: 'plugin-center', label: '插件中心', icon: Puzzle, view: 'plugin-center' }
]
```

`GlobalActivityRail.tsx`：垂直按钮列，`aria-label` 用中文 label；项目/插件中心来自 `GLOBAL_RAIL_ITEMS`，全局插件来自 `useAppStore((s) => s.pluginCenterEntries.filter((e) => e.scope === 'global'))`；点击调 `useAppStore.getState().setActiveView(...)`；样式用 Tailwind token（参照 `src/renderer/src/components/right-sidebar/activity-bar-buttons.tsx` 的类名模式）；挂载时 `useEffect` 调 `loadPluginCenterEntries()`。

`GlobalPluginHostView.tsx`：接收 `pluginKey: string`，从 store 查条目，渲染标题 + 空状态文案「插件面板将在 Phase 3 接入」。

- [ ] **Step 6: 接入 AppWorkspaceShell**

在 `AppWorkspaceShell.tsx` 根 flex 容器最左侧渲染 `<GlobalActivityRail />`；在既有 lazy 声明区追加（组件用命名导出，与文件现有 `lazy(() => import(...).then(...))` 模式一致）：

```tsx
const PluginCenterPage = lazy(() =>
  import('../components/plugin-center/PluginCenterPage').then((module) => ({
    default: module.PluginCenterPage
  }))
)
```

在内容区分支追加：

```tsx
{activeView === 'plugin-center' ? <PluginCenterPage /> : null}
{activeView.startsWith('plugin:') ? <GlobalPluginHostView pluginKey={activeView.slice('plugin:'.length)} /> : null}
```

左侧 sidebar 的隐藏规则：`const isProjectView = !(activeView === 'plugin-center' || activeView.startsWith('plugin:'))`，仅 `isProjectView` 时渲染 `<Sidebar ... />`。同步在 `TitlebarMainStrip`/`TitlebarLeftControls` 保持不报错（编译驱动修正）。

Run: `pnpm test src/renderer/src/components/global-rail/GlobalActivityRail.test.tsx && pnpm typecheck`
Expected: 组件测试 PASS；typecheck 至多剩 `PluginCenterPage` 缺失（Task 6 立即补）

- [ ] **Step 7: Commit**

```bash
git add -A
git commit -m "feat: 最左全局 rail 与 plugin-center 顶层视图"
```

---

### Task 6: 插件中心页（mock 数据）

**Files:**
- Create: `src/renderer/src/components/plugin-center/PluginCenterPage.tsx`、`PluginCenterTabs.tsx`、`PluginCenterInstalledList.tsx`、`PluginCenterEntryRow.tsx`、`PluginCenterEntryRow.test.tsx`

**Interfaces:**
- Consumes: `usePluginCenterSlice` 的 `pluginCenterEntries`/`loadPluginCenterEntries`；`window.api.plugins.setEnabled/getLogs/remove`；`PluginCenterEntry.scope`。
- Produces: 三视图页面（已安装/市场/开发中）；已安装列表行展示 scope 徽标与启停开关（Task 7 的右侧过滤共用同一数据源）。

- [ ] **Step 1: 确认 shadcn 基元可用**

Run: `ls src/renderer/src/components/ui | tr '\n' ' '`
Expected: 看到 `button.tsx`、`tabs.tsx`（或等价）、`badge.tsx` 等；若 `tabs` 不存在，用两个 `Button` 实现 tab 切换，不新引依赖。

- [ ] **Step 2: 写行组件测试（先失败）**

`PluginCenterEntryRow.test.tsx`：

```tsx
// @vitest-environment happy-dom
import { render, screen } from '@testing-library/react'
import { describe, expect, it } from 'vitest'
import { PluginCenterEntryRow } from './PluginCenterEntryRow'
import { MOCK_GLOBAL_PLUGINS } from '../../../../bridge/mock/fixtures'

describe('PluginCenterEntryRow', () => {
  it('shows scope badge and plugin identity', () => {
    render(<PluginCenterEntryRow entry={MOCK_GLOBAL_PLUGINS[0]} onToggle={() => {}} onOpenLogs={() => {}} />)
    expect(screen.getByText('Database Manager')).toBeTruthy()
    expect(screen.getByText('全局')).toBeTruthy()
  })
})
```

Run: `pnpm test src/renderer/src/components/plugin-center/PluginCenterEntryRow.test.tsx`
Expected: FAIL——组件不存在

- [ ] **Step 3: 实现页面与行组件**

`PluginCenterPage.tsx`：渲染标题「插件中心」与三个 tab（`已安装`/`市场`/`开发中`）；挂载时 `useEffect` 调 `loadPluginCenterEntries()`；`已安装` 渲染 `PluginCenterInstalledList`；`市场` 调 `window.api.plugins.listMarketplaces()` 为空时显示空状态；`开发中` 显示空状态。样式沿用 shadcn 基元与 token，不加新颜色。

`PluginCenterEntryRow.tsx`：左侧 icon（用 manifest icon 名映射到 lucide，参照 `right-sidebar/plugin-panel-activity-items.ts` 的映射模式，Phase 0 允许只映射 `database/plug/filetext` 三者，其余回退 `Plug`）、名称/版本/发布者、描述、右侧 scope 徽标（`全局`/`当前项目`）与启用开关（`Switch` 不存在时用 `Button` 切换文案「启用/禁用」）。

- [ ] **Step 4: 跑测试与构建**

Run: `pnpm test src/renderer/src/components/plugin-center && pnpm typecheck && pnpm build:web`
Expected: PASS + 0 errors

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "feat: 插件中心页（mock 数据）"
```

---

### Task 7: 右侧栏项目级插件作用域过滤（mock）

**Files:**
- Modify: `src/renderer/src/components/right-sidebar/plugin-panel-activity-items.ts`、`src/renderer/src/components/right-sidebar/use-right-sidebar-activity-items.ts`
- Create: `src/renderer/src/components/right-sidebar/plugin-panel-scope-filter.test.ts`
- Modify: 现有 `plugin-panel-activity-items.test.ts`（若断言受影响）

**Interfaces:**
- Consumes: `PluginCenterEntry.scope`（全局插件不在右侧栏出现；`scope` 缺失时按 `'project'` 处理以兼容既有 fixture）。
- Produces: `filterRightSidebarPluginEntries(entries)` 纯函数 + 在 `use-right-sidebar-activity-items` 中的接线。

- [ ] **Step 1: 写过滤测试（先失败）**

`plugin-panel-scope-filter.test.ts`：

```ts
import { describe, expect, it } from 'vitest'
import { filterRightSidebarPluginEntries } from './plugin-panel-activity-items'
import { MOCK_GLOBAL_PLUGINS, MOCK_PROJECT_PLUGINS } from '../../../../bridge/mock/fixtures'

describe('filterRightSidebarPluginEntries', () => {
  it('keeps project-scoped and unset entries, drops global', () => {
    const kept = filterRightSidebarPluginEntries([
      ...MOCK_GLOBAL_PLUGINS,
      ...MOCK_PROJECT_PLUGINS,
      { pluginKey: 'legacy', scope: undefined }
    ] as never)
    expect(kept.map((e) => e.pluginKey)).toEqual(['repo-notes', 'legacy'])
  })
})
```

Run: `pnpm test src/renderer/src/components/right-sidebar/plugin-panel-scope-filter.test.ts`
Expected: FAIL——函数不存在

- [ ] **Step 2: 实现过滤并接线**

在 `plugin-panel-activity-items.ts` 导出：

```ts
export function filterRightSidebarPluginEntries<T extends { scope?: 'global' | 'project' }>(
  entries: T[]
): T[] {
  return entries.filter((entry) => (entry.scope ?? 'project') !== 'global')
}
```

在 `use-right-sidebar-activity-items.ts` 取插件面板数据处套用该过滤（数据源改为 store 的 `pluginCenterEntries`）。

- [ ] **Step 3: 跑测试与回归**

Run: `pnpm test src/renderer/src/components/right-sidebar/plugin-panel-scope-filter.test.ts src/renderer/src/components/right-sidebar/plugin-panel-activity-items.test.ts`
Expected: PASS（旧测试断言若不成立，仅修改与 scope 相关断言）

- [ ] **Step 4: Commit**

```bash
git add -A
git commit -m "feat: 右侧栏项目级插件作用域过滤（mock）"
```

---

### Task 8: ade-pty crate 骨架与 PTY 吞吐 spike

**Files:**
- Create: `src-tauri/crates/ade-pty/Cargo.toml`、`src-tauri/crates/ade-pty/src/lib.rs`、`src-tauri/crates/ade-pty/src/bin/pty_bench.rs`、`src-tauri/crates/ade-pty/tests/throughput.rs`
- Create: `docs/spikes/2026-09-14-pty-throughput.md`

**Interfaces:**
- Consumes: `portable-pty`（`cargo add` 解析当前版本）。
- Produces: `run_sink(mode)` 等最小 API；吞吐结论文档（含 Windows 实测数值与 Tauri Channel 承载决策建议）。

- [ ] **Step 1: 建 crate**

```bash
mkdir -p src-tauri/crates/ade-pty/src/bin src-tauri/crates/ade-pty/tests
```

`src-tauri/crates/ade-pty/Cargo.toml`：

```toml
[package]
name = "ade-pty"
version = "0.0.1"
edition = "2021"

[dependencies]
```

Run: `cargo add portable-pty -p ade-pty`
Expected: 版本解析成功并写入 Cargo.toml

- [ ] **Step 2: 写吞吐测试（先失败）**

`tests/throughput.rs`：

```rust
use ade_pty::measure_pty_throughput;

#[test]
fn pty_delivers_expected_bytes_within_budget() {
    let total_bytes = 8 * 1024 * 1024;
    let report = measure_pty_throughput(total_bytes).expect("pty run");
    assert_eq!(report.received_bytes, total_bytes as u64);
    println!(
        "pty throughput: {:.1} MB/s, chunk={} bytes, elapsed={:?}",
        report.mb_per_second, report.chunk_bytes, report.elapsed
    );
}
```

Run: `cargo test -p ade-pty --test throughput`
Expected: FAIL——`measure_pty_throughput` 不存在

- [ ] **Step 3: 实现 sink 与测量**

`src/lib.rs`：

```rust
use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use std::io::{Read, Write};
use std::time::{Duration, Instant};

pub struct ThroughputReport {
    pub received_bytes: u64,
    pub chunk_bytes: usize,
    pub elapsed: Duration,
    pub mb_per_second: f64,
}

pub fn measure_pty_throughput(total_bytes: usize) -> std::io::Result<ThroughputReport> {
    let pty = native_pty_system();
    let pair = pty.openpty(PtySize { rows: 40, cols: 120, pixel_width: 0, pixel_height: 0 }).map_err(std::io::Error::other)?;
    let exe = std::env::current_exe()?;
    let mut cmd = CommandBuilder::new(exe);
    cmd.arg("--pty-sink");
    cmd.arg(total_bytes.to_string());
    let mut child = pair.slave.spawn_command(cmd).map_err(std::io::Error::other)?;
    drop(pair.slave);
    let mut reader = pair.master.try_clone_reader().map_err(std::io::Error::other)?;
    let started = Instant::now();
    let mut received = 0usize;
    let chunk_bytes = 64 * 1024;
    let mut buf = vec![0u8; chunk_bytes];
    while received < total_bytes {
        let read = reader.read(&mut buf)?;
        if read == 0 {
            break;
        }
        received += read;
    }
    let elapsed = started.elapsed();
    let _ = child.kill();
    Ok(ThroughputReport {
        received_bytes: received as u64,
        chunk_bytes,
        elapsed,
        mb_per_second: (received as f64 / 1024.0 / 1024.0) / elapsed.as_secs_f64(),
    })
}
```

`src/bin/pty_bench.rs`：

```rust
fn main() {
    if std::env::args().nth(1).as_deref() == Some("--pty-sink") {
        let total: usize = std::env::args().nth(2).and_then(|v| v.parse().ok()).unwrap_or(1024 * 1024);
        let chunk = vec![b'x'; 4096];
        let stdout = std::io::stdout();
        let mut out = stdout.lock();
        let mut written = 0usize;
        while written < total {
            let n = chunk.len().min(total - written);
            if std::io::Write::write_all(&mut out, &chunk[..n]).is_err() {
                break;
            }
            written += n;
        }
        return;
    }
    let report = ade_pty::measure_pty_throughput(8 * 1024 * 1024).expect("pty run");
    println!(
        "pty throughput: {:.1} MB/s (received {} bytes in {:?})",
        report.mb_per_second, report.received_bytes, report.elapsed
    );
}
```

Run: `cargo test -p ade-pty --test throughput -- --nocapture`
Expected: PASS，并打印实测 MB/s

- [ ] **Step 4: 跑 bench 并记录结论**

Run: `cargo run -p ade-pty --bin pty_bench`
Expected: 打印吞吐数值（Windows 本机）

创建 `docs/spikes/2026-09-14-pty-throughput.md`，填写：环境（OS 版本、CPU）、命令、实测 MB/s（64KiB 读）、观察（是否有丢帧/阻塞）、结论（Tauri Channel 方案是否可行；若 <50 MB/s 则 Phase 1 改用本地 socket 投递，并把该决策写入文档）。

- [ ] **Step 5: Commit**

```bash
git add src-tauri/crates/ade-pty docs/spikes/2026-09-14-pty-throughput.md
git commit -m "feat: ade-pty 骨架与 PTY 吞吐 spike"
```

---

### Task 9: CEF 打包 spike

**Files:**
- Create: `spikes/cef-embed/`（独立 Cargo 工程：`Cargo.toml`、`build.rs`、`src/main.rs`）
- Create: `docs/spikes/2026-09-14-cef-packaging.md`

**Interfaces:**
- Consumes: `cef` crate（tauri-apps 维护）。
- Produces: Windows 可运行的最小 CEF 窗口 + 体积/构建耗时数据 + macOS 未验证声明；Phase 0 规格 10.1 的 go/no-go 建议。

- [ ] **Step 1: 拉取 cef-rs 当前用法（避免凭记忆写 API）**

Run: `webfetch https://github.com/tauri-apps/cef-rs`（或等价 README URL）
Expected: 得到当前版本的 `Cargo.toml` 依赖写法、`build.rs` 要求、最小 main 示例与 CEF 二进制获取方式。

- [ ] **Step 2: 建最小工程（按 README 落地）**

```bash
mkdir -p spikes/cef-embed/src
```

创建 `spikes/cef-embed/Cargo.toml`（依赖与 feature 按 Step 1 的 README 原文）、`build.rs`、`src/main.rs`：打开一个窗口加载 `data:text/html,<h1>ade cef spike</h1>`。工程**不加入** `src-tauri` workspace（独立构建，避免污染主构建）。

Run: `cargo build --release --manifest-path spikes/cef-embed/Cargo.toml`
Expected: 构建成功（首次会下载 CEF 二进制，记录耗时）

- [ ] **Step 3: 运行与度量**

Run: `spikes/cef-embed/target/release/cef-embed`（后台启动，确认窗口渲染出标题后关闭）
Expected: 窗口显示 `ade cef spike`

度量并记录：release 产物体积（`spikes/cef-embed/target/release` 下 exe + 所需 CEF dll/framework 总体积）、构建耗时、启动耗时、内存占用（任务管理器粗测）。

- [ ] **Step 4: 写结论文档**

创建 `docs/spikes/2026-09-14-cef-packaging.md`，填写：环境、cef crate 版本、体积、构建/启动耗时、内存、问题清单（签名/公证未测）、macOS 状态 = **未验证（本机为 Windows；需在 macOS 机器复测）**、go/no-go 建议（若 Windows 侧体积 <300MB 且可独立进程隔离 → go，Phase 3 实施；否则维持系统 WebView 降级方案并回写规格风险 10.1）。

- [ ] **Step 5: Commit**

```bash
git add spikes/cef-embed docs/spikes/2026-09-14-cef-packaging.md
git commit -m "spike: CEF 打包验证与结论"
```

---

### Task 10: 死代码清扫（限时）与清单

**Files:**
- Create: `docs/phase0-dead-code-inventory.md`
- Delete: 经证实无引用的 G/I 独占模块（逐文件 `git rm`）

**Interfaces:**
- Consumes: Task 4 已移除的入口。
- Produces: 剩余 G/I 引用清单（文件路径 + 计数 + 建议阶段），供 Phase 1 计划消费。

- [ ] **Step 1: 建立引用清单**

Run（逐项记录计数）：

```bash
for term in "SshPassphraseDialog" "RemoteServerUpdateDialog" "SkillsPage" "MobilePage" "EphemeralVm" "skill-share" "cli-install" "orca-profile"; do
  echo "== $term"; grep -rl "$term" src/renderer/src src/shared 2>/dev/null | wc -l
done
```

把结果写入 `docs/phase0-dead-code-inventory.md`，注明每项「Phase 0 已移除入口 / Phase 1 清理代码」。

- [ ] **Step 2: 限时清扫（一次提交，保持绿灯）**

对仅剩测试或仅剩自身引用的 G/I 模块执行 `git rm`；每删 20 个文件跑一次：

Run: `pnpm typecheck && pnpm build:web`
Expected: 0 errors；一旦出现跨引用删除失败立即回滚该批（`git checkout -- <paths>`），并把失败文件记入清单。

时间盒：本 Task 不超过 2 小时；到点即停止并把剩余项写进清单。

- [ ] **Step 3: 更新清单与 Commit**

在文档中记录清扫结果（删除数量、剩余数量、Phase 1 建议）。

```bash
git add -A
git commit -m "chore: Phase 0 死代码限时清扫与清单"
```

---

### Task 11: Phase 0 验收与文档

**Files:**
- Create: `docs/phase0-acceptance.md`
- Modify: `README.md`（新仓库尚无 README，创建并写明 dev/build/test 命令与参照仓库说明）

**Interfaces:**
- Consumes: 全部前序 Task。
- Produces: Phase 0 验收记录（对应规格第 8 节 Phase 0 验收标准的逐条打勾）。

- [ ] **Step 1: 跑全量验证**

```bash
pnpm typecheck
pnpm build:web
pnpm test -- --run 2>&1 | tail -20
cargo check --manifest-path src-tauri/Cargo.toml
pnpm tauri build --debug --no-bundle
```
Expected: 全部通过；`pnpm test` 全量若有既存失败，记录失败清单与原因（fork 带来的 G/I 断言），不得隐藏。

- [ ] **Step 2: 写验收文档**

`docs/phase0-acceptance.md` 逐条对照规格第 8 节：

- `pnpm dev` 启动 Tauri 窗口（记录执行环境与截图路径；无显示器则标注未执行）
- 三视图切换且可持久化（重启后回到上次视图）
- 插件中心可浏览（含 scope 徽标、启停）
- 右侧栏只出现项目级插件
- 设置页为简化分组
- 构建链无 Electron 依赖（`grep -rn "from 'electron'" src/renderer src/shared src/bridge` 无输出）
- CEF spike 与 PTY spike 均有书面结论（链接文档）

- [ ] **Step 3: 写 README**

```markdown
# ade

基于 stablyai/orca 二次开发的 AI 编排 IDE（Tauri v2 + Rust，Windows/macOS）。

- 规格：`docs/superpowers/specs/2026-09-14-ade-design.md`
- Phase 0 计划：`docs/superpowers/plans/2026-09-14-ade-phase0-skeleton-ui.md`
- 参照仓库（只读）：`../orca-main/orca-main`

## 开发

pnpm install
pnpm dev          # Tauri 开发窗口
pnpm typecheck    # TS 类型检查
pnpm test         # Vitest
pnpm build:web    # 仅构建渲染层
cargo test -p ade-pty
```

- [ ] **Step 4: Commit**

```bash
git add -A
git commit -m "docs: Phase 0 验收记录与 README"
```

---

## Self-Review 记录（写计划时已执行）

1. **Spec 覆盖**：规格 §3 结构→Tasks 1/2；§4 选型 1/4→Tasks 3/8；§6.1→Task 5；§6.2→Task 6；§6.3→Task 7；§6.4/6.5→Task 4；§7 全部→Phase 3（本计划仅 mock 数据形态，Task 5/6/7）；§8 Phase 0 验收→Task 11；§10 风险 1/2→Tasks 8/9。
2. **占位符扫描**：无 TBD/TODO；所有代码步骤含可运行代码；度量类步骤给出必填字段与判定阈值。
3. **类型一致性**：`PluginCenterEntry`、`PluginScope`、`filterRightSidebarPluginEntries`、`createAdeApi`、`UnimplementedBridgeError` 在任务间命名一致；`setActiveView`/`activeView` 沿用 fork 既有成员。
