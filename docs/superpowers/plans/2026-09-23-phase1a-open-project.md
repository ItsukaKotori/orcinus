# Phase 1 子项目 A（打开项目纵切）实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 把渲染层从全 mock bridge 推进到真实 Rust 后端：本地项目/分组/文件夹工作区注册与持久化、文件树与 Monaco 读写、文件监听、内嵌搜索与 quick open、设置与 UI 状态持久化、最小 worktree 投影；`pnpm dev` 下跑通「打开 → 编辑保存 → 重启恢复」。

**Architecture:** 新增 `ade-core`（模型/id/错误/默认值）、`ade-store`（JSON 原子持久化）、`ade-fs`（授权/读写/遍历/搜索/watch）、`ade-git`（最小 git CLI）、`ade-bridge`（Tauri commands/事件/specta）；`orcinus-app` 改为 Rust 构建主窗口并注入 bootstrap。前端 `src/bridge/real/*` 按域替换 mock，`VITE_ADE_BRIDGE=mock` 全量回退；projects 域在 TS 侧由 repos 投影。

**Tech Stack:** Rust（tauri 2.11、serde、specta、uuid、thiserror、notify、ignore、grep-searcher/grep-regex、globset、trash、rfd）、TypeScript 7、Vite（rolldown-vite）、React 19、Vitest 4、pnpm 12。

**Spec:** `docs/superpowers/specs/2026-09-23-phase1a-open-project-design.md`（执行者需通读；本计划是其落地）

## Global Constraints

- 分支 `phase1a-open-project`（Task 0 创建），base `main@103776a`；不 push；完成后由 finishing-a-development-branch 交用户决定。
- 行为 oracle：`/Users/itsuka/CodeSpace/orca`（只读）；语义基准 shared 模块见 spec §5.6，其测试保持全绿。
- 门禁：每个 Rust 任务 `cargo test --manifest-path src-tauri/Cargo.toml` 相关包全绿；每个 TS 任务 `rm -f tsconfig.tsbuildinfo && pnpm typecheck && pnpm build:web` exit 0；最终 `pnpm test`（既有 3825 文件）全绿。
- 命令名契约：`<域>_<方法 snake_case>`（`fs_read_dir`、`repos_add`…）；Rust 结构体 `#[serde(rename_all = "camelCase")]` 与 `src/shared/preload-api/api/*.ts` 形状逐字对齐。
- 未接真方法必须抛 `UnimplementedBridgeError('<域>.<方法>')`，禁止静默成功。
- 拒绝访问文案逐字：`Access denied: path resolves outside allowed directories. If this blocks a legitimate workflow, please file a GitHub issue.`
- 品牌：用户可见默认路径/文案用 `Orcinus`（替换 oracle 的 Orca）；不改既有 i18n 键。
- Windows 不在本计划验证范围（记录跟踪）；不在本计划做 mock 残渣清理。
- 一次性脚本放 `.superpowers/sdd/2026-09-23-phase1a-open-project/tools/`（gitignored）；生成物（defaults JSON、specta bindings）必须 checked in。

## 文件结构（结束时）

```
src-tauri/
├── Cargo.toml                         # workspace + orcinus-app 依赖扩展
├── crates/
│   ├── ade-core/                      # models/ids/errors/path_compare/defaults(内嵌生成 JSON)
│   ├── ade-store/                     # json_store(原子写/轮换) + settings/ui/projects store
│   ├── ade-git/                       # version/rev-parse/worktree-list 解析
│   ├── ade-fs/                        # auth/read/mutate/walk/search/watch
│   ├── ade-bridge/                    # commands/events/errors/state/specta_export
│   └── orcinus-pty/                   # 既有（不动）
└── src/{lib.rs,main.rs}               # 窗口构建 + init script + generate_handler + manage state
src/bridge/
├── real/                              # 各域真实适配 + generated/tauri-bindings.ts
├── create-api.ts                      # 按域组装 + VITE_ADE_BRIDGE=mock 回退
└── mock/                              # 既有（未接真域保留）
config/scripts/
├── generate-ade-defaults.test.mjs     # 生成/新鲜度（ADE_WRITE_DEFAULTS=1 写入）
└── ...
docs/phase1a-open-project-record.md    # Task 12 产出
```

---

## Task 0: 分支、工作区、依赖可用性与基线

**Files:**
- Create: `docs/superpowers/plans/2026-09-23-phase1a-open-project.md`（本文件，先提交）
- Create: `.superpowers/sdd/2026-09-23-phase1a-open-project/`（gitignored）

- [ ] **Step 1: 建分支并提交本计划**

```bash
git checkout main && git pull --ff-only || true
git checkout -b phase1a-open-project
git add docs/superpowers/plans/2026-09-23-phase1a-open-project.md
git commit -m "docs: 新增 Phase 1 子项目 A 实施计划（打开项目纵切）"
```

- [ ] **Step 2: 建 SDD 工作区**

```bash
mkdir -p .superpowers/sdd/2026-09-23-phase1a-open-project/tools
```

- [ ] **Step 3: 验证 crates.io 可用（新增依赖是硬前提）**

```bash
cargo search specta --limit 1
cargo search tauri-specta --limit 1
cargo search trash --limit 1
```

预期：均返回结果。若网络不可达，STOP 并报告 BLOCKED（附完整错误）——不要手写替代方案。

- [ ] **Step 4: 记录基线**

```bash
cargo test --manifest-path src-tauri/Cargo.toml 2>&1 | tail -5
rm -f tsconfig.tsbuildinfo && pnpm typecheck && pnpm build:web
git status -sb
```

预期：cargo 全绿（含 orcinus-pty 3 测试）；门禁 exit 0；工作树干净。

---

## Task 1: ade-core（模型、id、错误、默认值）

**Files:**
- Create: `src-tauri/crates/ade-core/Cargo.toml`、`src/lib.rs`、`src/ids.rs`、`src/errors.rs`、`src/path_compare.rs`、`src/defaults/mod.rs`、`src/defaults/ade-defaults.generated.json`
- Create: `config/scripts/generate-ade-defaults.test.mjs`
- Modify: `src-tauri/Cargo.toml`（workspace members 自动含 `crates/*`，无需改）

**Interfaces:**
- Produces: `ade_core::ids::{new_uuid, worktree_id, folder_workspace_root_id}`；`ade_core::path_compare::normalize_for_comparison(&str) -> String`；`ade_core::errors::CoreError`；`ade_core::defaults::{settings_defaults(home: &str) -> serde_json::Value, ui_state_defaults() -> serde_json::Value}`

- [ ] **Step 1: 脚手架 + id/路径归一单测（TDD）**

`Cargo.toml` 依赖：`serde`、`serde_json`、`uuid = { version = "1", features = ["v4"] }`、`thiserror`。

`src/ids.rs`：

```rust
use uuid::Uuid;

pub fn new_uuid() -> String {
    Uuid::new_v4().to_string()
}

pub fn worktree_id(repo_id: &str, path: &str) -> String {
    format!("{repo_id}::{path}")
}

pub fn folder_workspace_root_id(repo_id: &str, path: &str) -> String {
    worktree_id(repo_id, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn worktree_id_uses_double_colon_separator() {
        assert_eq!(worktree_id("repo-1", "/tmp/proj"), "repo-1::/tmp/proj");
    }
}
```

`src/path_compare.rs`（照抄 oracle `cross-platform-path.ts:51-67` 语义：NFC + 反斜杠折叠 + Windows 小写；macOS 保留大小写）：

```rust
use unicode_normalization::UnicodeNormalization;

/// Normalize a path for equality comparison: NFC, fold backslashes, lowercase on Windows.
pub fn normalize_for_comparison(path: &str) -> String {
    let normalized: String = path.nfc().collect();
    let folded = normalized.replace('\\', "/");
    if cfg!(windows) {
        folded.to_lowercase()
    } else {
        folded
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folds_backslashes_and_normalizes_unicode() {
        assert_eq!(normalize_for_comparison("C:\\Repo\\A"), if cfg!(windows) { "c:/repo/a".to_string() } else { "C:/Repo/A".to_string() });
        assert_eq!(normalize_for_comparison("e\u{0301}"), "é");
    }
}
```

（`Cargo.toml` 增加 `unicode-normalization = "0.1"`。）

- [ ] **Step 2: 默认值生成器（TS 单一源 → checked-in JSON）**

创建 `config/scripts/generate-ade-defaults.test.mjs`：

```js
import { readFileSync, writeFileSync, mkdirSync } from 'node:fs'
import { dirname, resolve } from 'node:path'
import { expect, it } from 'vitest'
import { getDefaultSettings, getDefaultUIState } from '../../src/shared/constants'

const OUT = resolve('src-tauri/crates/ade-core/src/defaults/ade-defaults.generated.json')
const HOME_PLACEHOLDER = '{{HOME}}'

function buildPayload() {
  const settings = getDefaultSettings(HOME_PLACEHOLDER)
  const uiState = getDefaultUIState()
  return `${JSON.stringify({ schemaVersion: 1, settings, uiState }, null, 2)}\n`
}

it('keeps ade-defaults.generated.json fresh', () => {
  const payload = buildPayload()
  if (process.env.ADE_WRITE_DEFAULTS === '1') {
    mkdirSync(dirname(OUT), { recursive: true })
    writeFileSync(OUT, payload)
    return
  }
  expect(readFileSync(OUT, 'utf8')).toBe(payload)
})
```

生成命令（执行者运行一次并提交产物）：

```bash
ADE_WRITE_DEFAULTS=1 pnpm vitest run config/scripts/generate-ade-defaults.test.mjs
pnpm vitest run config/scripts/generate-ade-defaults.test.mjs
```

预期：第二次为纯比对，PASS。注意：`getDefaultSettings` 若引用了 homedir 之外的环境（如 `process.platform`），生成物在双平台可能不同——若发现平台相关字段，记录并在 Rust 侧按平台覆盖（Task 1 Step 3 的 `settings_defaults` 中实现）。

- [ ] **Step 3: Rust 加载默认值（含 `{{HOME}}` 替换）**

`src/defaults/mod.rs`：

```rust
use serde_json::Value;

const GENERATED: &str = include_str!("ade-defaults.generated.json");

fn substitute_home(value: &mut Value, home: &str) {
    match value {
        Value::String(s) => {
            if s.contains("{{HOME}}") {
                *s = s.replace("{{HOME}}", home);
            }
        }
        Value::Array(items) => items.iter_mut().for_each(|item| substitute_home(item, home)),
        Value::Object(map) => map.values_mut().for_each(|item| substitute_home(item, home)),
        _ => {}
    }
}

pub fn settings_defaults(home: &str) -> Value {
    let mut payload: Value = serde_json::from_str(GENERATED).expect("generated defaults are valid JSON");
    let mut settings = payload["settings"].take();
    substitute_home(&mut settings, home);
    settings
}

pub fn ui_state_defaults() -> Value {
    let mut payload: Value = serde_json::from_str(GENERATED).expect("generated defaults are valid JSON");
    payload["uiState"].take()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn substitutes_home_placeholder() {
        let defaults = settings_defaults("/Users/tester");
        let text = defaults.to_string();
        assert!(!text.contains("{{HOME}}"));
    }
}
```

- [ ] **Step 4: errors.rs**

```rust
use thiserror::Error;

#[derive(Debug, Error)]
pub enum CoreError {
    #[error("Not a valid git repository: {0}")]
    NotAGitRepository(String),
    #[error("Path is not allowed: {0}")]
    PathNotAllowed(String),
    #[error("Invalid input: {0}")]
    InvalidInput(String),
    #[error("Not found: {0}")]
    NotFound(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}
```

- [ ] **Step 5: 门禁 + 提交**

```bash
cargo test --manifest-path src-tauri/Cargo.toml -p ade-core
pnpm vitest run config/scripts/generate-ade-defaults.test.mjs
git add -A && git commit -m "feat(core): ade-core 骨架（id/路径归一/错误/默认值生成）"
```

---

## Task 2: ade-store（JSON 持久化）

**Files:**
- Create: `src-tauri/crates/ade-store/Cargo.toml`、`src/lib.rs`、`src/json_file.rs`、`src/settings_store.rs`、`src/ui_state_store.rs`、`src/projects_store.rs`
- Test: 各文件内 `#[cfg(test)]`

**Interfaces:**
- Produces: `JsonFile::{load, save}`（原子写 + `.bak1/.bak2` 轮换 + 损坏回退）；`SettingsStore::{load, get, set_partial, snapshot}`；`UiStateStore::{load, get, set, record_feature_interaction}`；`ProjectsStore::{load, repos, project_groups, folder_workspaces, mutate_repos, mutate_groups, mutate_folder_workspaces}`

- [ ] **Step 1: 原子写 + 轮换（TDD）**

`src/json_file.rs` 关键实现：

```rust
pub struct JsonFile {
    path: PathBuf,
}

impl JsonFile {
    pub fn new(path: impl Into<PathBuf>) -> Self { Self { path: path.into() } }

    pub fn load(&self) -> serde_json::Value {
        let candidates = [self.path.clone(), backup(&self.path, 1), backup(&self.path, 2)];
        for candidate in candidates {
            if let Ok(text) = std::fs::read_to_string(&candidate) {
                if let Ok(value) = serde_json::from_str(&text) {
                    return value;
                }
            }
        }
        serde_json::Value::Null
    }

    pub fn save(&self, value: &serde_json::Value) -> std::io::Result<()> {
        if let Some(parent) = self.path.parent() { std::fs::create_dir_all(parent)?; }
        if self.path.exists() {
            let _ = std::fs::rename(&self.path, backup(&self.path, 1));
            if let Ok(text) = std::fs::read_to_string(backup(&self.path, 1)) {
                let _ = std::fs::write(backup(&self.path, 2), text);
            }
        }
        let tmp = self.path.with_extension("tmp");
        let payload = format!("{}\n", serde_json::to_string_pretty(value)?);
        let mut file = std::fs::File::create(&tmp)?;
        use std::io::Write;
        file.write_all(payload.as_bytes())?;
        file.sync_all()?;
        std::fs::rename(&tmp, &self.path)
    }
}
```

单测：写入→读取一致；损坏主文件→回退 bak1；再损坏→bak2；保存两次产生轮换。

- [ ] **Step 2: settings store（浅合并 + 深合并例外 + 不可写键剔除）**

```rust
const DEEP_MERGE_KEYS: &[&str] = &["notifications", "telemetry", "worktreeVisibilityDefaults"];
const RENDERER_READONLY_KEYS: &[&str] =
    &["pluginConsents", "disabledPlugins", "activeRuntimeEnvironmentId", "floatingTerminalTrustedCwds"];
```

`set_partial(updates: Value) -> Value`：`updates` 剔除只读键 → 与当前值浅合并（`DEEP_MERGE_KEYS` 递归对象合并）→ `save` → 返回完整对象；`load` 时 `defaults ∪ stored`。单测覆盖：浅合并、notifications 深合并、只读键被忽略、返回完整对象。

- [ ] **Step 3: ui-state store（整对象 set + 例外合并）**

`set(updates: Value) -> Value`：数组整体替换；`contextualToursSeenIds` 并集；`featureInteractions` 逐 id 合并（`firstInteractedAt` 取 min、`interactionCount` 取 max）；`workspaceCleanup` 深合并。`record_feature_interaction(id)`：读当前值 → 计数 +1 / 首次时间 → save → 返回完整对象。单测覆盖四个例外。

- [ ] **Step 4: projects store**

结构：`{ schemaVersion, repos: [], projectGroups: [], folderWorkspaces: [] }`；提供按 id 查找/替换的 `mutate_*`（闭包式：读 → 变换 → save），并在保存前保证 id 唯一。单测：增删改查往返。

- [ ] **Step 5: 门禁 + 提交**

```bash
cargo test --manifest-path src-tauri/Cargo.toml -p ade-store
git add -A && git commit -m "feat(store): JSON 原子持久化与三存储（settings/ui/projects）"
```

---

## Task 3: ade-git（最小 CLI 封装）

**Files:**
- Create: `src-tauri/crates/ade-git/Cargo.toml`、`src/lib.rs`、`src/porcelain.rs`
- Test: fixtures 内联字符串 + `git init` 临时仓库集成测试

**Interfaces:**
- Produces: `ade_git::{is_available, rev_parse_toplevel, is_inside_work_tree, worktree_list}`；`worktree_list(path) -> Vec<GitWorktreeEntry>`，`GitWorktreeEntry { path, head, branch: Option<String>, is_bare, is_main_worktree }`

- [ ] **Step 1: porcelain 解析（TDD，fixtures 驱动）**

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitWorktreeEntry {
    pub path: String,
    pub head: String,
    pub branch: Option<String>,
    pub is_bare: bool,
    pub is_main_worktree: bool,
}

/// Parse `git worktree list --porcelain -z` output. The first block is the main worktree.
pub fn parse_worktree_list(bytes: &[u8]) -> Vec<GitWorktreeEntry> { /* NUL 分隔记录；worktree/HEAD/branch/bare 字段；首块 is_main_worktree=true；prunable 块跳过 */ }
```

单测 fixtures：主工作树 + 链接工作树 + detached HEAD + bare + prunable。

- [ ] **Step 2: CLI 调用**

```rust
pub fn is_available() -> bool { /* git --version，1.5s 超时（wait_timeout 轮询 kill） */ }
pub fn rev_parse_toplevel(path: &str) -> Result<String, CoreError> { /* git -C path rev-parse --show-toplevel */ }
pub fn is_inside_work_tree(path: &str) -> bool { /* rev-parse --is-inside-work-tree == "true" */ }
pub fn worktree_list(path: &str) -> Result<Vec<GitWorktreeEntry>, CoreError> { /* -C path worktree list --porcelain -z */ }
```

集成测试：`git init` 临时目录 → 添加一次 commit（`-c user.email=... -c user.name=... commit --allow-empty`）→ `worktree_list` 返回单条且 `is_main_worktree`。

- [ ] **Step 3: 门禁 + 提交**

```bash
cargo test --manifest-path src-tauri/Cargo.toml -p ade-git
git add -A && git commit -m "feat(git): 最小 git CLI 封装与 worktree porcelain 解析"
```

---

## Task 4: ade-fs 基础（授权、读写、回收站删除）

**Files:**
- Create: `src-tauri/crates/ade-fs/Cargo.toml`、`src/lib.rs`、`src/auth.rs`、`src/read.rs`、`src/mutate.rs`
- Test: 模块内 + `tests/fs_basic.rs`（tempdir）

**Interfaces:**
- Produces: `FsService`（持有 `PathAuthRegistry`）：
  - `authorize_root(path)` / `revoke_root(path)`（repo/folder workspace 增删时调用）
  - `authorize_external(path) -> Result<()>`（`authorizeExternalPath` 命令）
  - `resolve(path) -> Result<PathBuf, FsError>`（canonicalize + 前缀段边界匹配）
  - `read_file / read_dir / stat / path_exists / paths_exist / write_file / create_file / create_dir / rename / copy / delete_path`

- [ ] **Step 1: 授权（TDD）**

规则：allowed roots = 登记的 repo/folderWorkspace 根 ∪ 显式授权外部路径；比较用 canonicalize 后 `normalize_for_comparison` + 段边界（`root` 或 `root + "/"` 前缀）。拒绝返回 `FsError::PathAccessDenied`，文案逐字（Global Constraints）。单测：根内允许、`/root-other` 不允许、symlink 逃逸（canonicalize 后越界）不允许、外部显式授权后允许。

- [ ] **Step 2: 读取**

- `read_file`：读入上限（文本 5 MiB、二进制预览上限对齐 oracle 常量；超过返回错误/截断语义以 oracle 为准：文本超限报错，图片按 MIME 返回 base64？——A 按 oracle 行为：非图片二进制返回 `isBinary:true` 且 content 为空，图片按 `mimeType` 返回 base64 data）。实现时对照 `src/shared/filesystem-entry-types` 与渲染端消费点（`useEditorPanelFileContentLoader`）锁定字段。
- `read_dir`：`read_dir` + `sortDirEntries` 语义（目录在前、`Intl.Collator('en',{numeric:true})` 等价自然序——Rust 用 `natord`/自定义比较；加单测：`99 - a` 在 `100 - b` 前、目录优先、大小写与数字 tie-break 用码位）。
- `stat`/`path_exists`/`paths_exist`（批量 ≤128，逐项 `{exists}` 或 `{error}`）。

- [ ] **Step 3: 写入与删除**

- `write_file`：temp + rename 原子写（同目录 `.ade-tmp-*`）；`create_file`（父目录必须存在，存在即报错）、`create_dir`（递归）、`rename`、`copy`（文件/目录递归）。
- `delete_path`：`trash::delete`（回收站）；拒绝删除授权根自身；失败回退 `std::fs::remove_dir_all/remove_file`？——不回退，返回错误（对齐 oracle：trash 失败即失败）。
- 单测：原子写内容、重命名跨目录、复制目录、删除后不存在（trash 在 CI/沙箱可能不可用——单测用 `#[cfg(target_os)]` 或在断言中容忍 trash 错误并仅验证错误路径；集成测试在 macOS 本地跑真回收站）。

- [ ] **Step 4: 门禁 + 提交**

```bash
cargo test --manifest-path src-tauri/Cargo.toml -p ade-fs
git add -A && git commit -m "feat(fs): 路径授权、读写与回收站删除"
```

---

## Task 5: ade-fs listFiles（quick open）

**Files:**
- Create: `src-tauri/crates/ade-fs/src/walk.rs`
- Test: `tests/list_files.rs`（git init tempdir + .gitignore）

**Interfaces:**
- Produces: `FsService::list_files(root: &str, exclude_paths: &[String], max_results: usize, token: &str, cancel: CancelRegistry) -> Result<Vec<String>, FsError>`（返回 root 相对、`/` 分隔路径）；`FsService::list_markdown_documents(root) -> Vec<MarkdownDocument>`

- [ ] **Step 1: 遍历语义（TDD）**

用 `ignore` crate：`hidden(true)`、`git_ignore(true)`、`git_global(true)`、`git_exclude(true)`、`parents(false)`、不 follow symlink；两趟（primary + `no_ignore_vcs` 忽略趟，ignored 趟结果排后）；隐藏目录黑名单（目录形 glob 剪枝）：

```rust
const HIDDEN_DIR_BLOCKLIST: &[&str] = &[
    ".git", ".next", ".nuxt", ".cache", ".stably", ".vscode", ".idea", ".yarn", ".pnpm-store",
    ".terraform", ".docker", ".husky", ".npm", ".npm-global", ".gvfs", ".local/share", "node_modules",
];
```

`exclude_paths`：绝对路径 → root 相对前缀，段边界排除，越界/畸形静默丢弃。`max_results`：达到即停止并返回已收集。`token` 取消：`CancelRegistry`（`Mutex<HashMap<String, Arc<AtomicBool>>>`），遍历每 N 项检查；`cancel_list_files(token)` 置位。

单测：gitignore 命中排除、hidden 文件包含但 `node_modules` 剪枝、ignored 趟包含被 ignore 文件且排后、excludePaths 段边界、maxResults 截断、取消提前返回。

- [ ] **Step 2: markdown 文档**

`list_markdown_documents`：同遍历规则过滤 `.md`（含隐藏趟？对齐 oracle：listMarkdownDocuments 用同 listFiles 遍历）→ `{filePath,relativePath,basename,name}`（name = 去扩展名 basename）。

- [ ] **Step 3: 门禁 + 提交**

```bash
cargo test --manifest-path src-tauri/Cargo.toml -p ade-fs
git add -A && git commit -m "feat(fs): listFiles 与 markdown 文档遍历（ignore 语义）"
```

---

## Task 6: ade-fs search（内嵌 ripgrep 语义）

**Files:**
- Create: `src-tauri/crates/ade-fs/src/search.rs`
- Test: `tests/search.rs`

**Interfaces:**
- Produces: `FsService::search(options: SearchOptions, cancel: CancelRegistry) -> Result<SearchResult, FsError>`；`SearchOptions { query, root_path, case_sensitive, whole_word, use_regex, include_pattern, exclude_pattern, max_results }`（camelCase 序列化）

- [ ] **Step 1: 搜索实现（TDD）**

依赖 `ignore`、`grep-searcher`、`grep-regex`、`globset`。常量与语义（照抄 spec §5.1）：

```rust
const MAX_RESULTS_DEFAULT: usize = 2000;
const MAX_RESULTS_CAP: usize = 2000;
const PER_FILE_MAX_MATCHES: usize = 100;
const MAX_FILE_SIZE: u64 = 5 * 1024 * 1024;
const MAX_LINE_CONTENT_CHARS: usize = 500;
const SEARCH_TIMEOUT_MS: u64 = 15_000;
```

- glob：`includePattern`/`excludePattern` 逗号分隔（支持 `\` 转义）；无 `/` 的自动加 `**/` 前缀；include 正向、exclude 反向（`globset` 组合）。
- `line` = 1-based 行号；`column` = **UTF-8 字节偏移 + 1**；`matchLength` = 字节数；`lineContent` 去 `\n`、>500 字符窗口截断加 `…`；非 UTF-8 行 `lineContent=""`。
- 遵循 gitignore、含隐藏文件、排 `.git`、不 follow symlink、二进制跳过（searcher 的 binary detection）。
- `truncated`：达到 `maxResults` 立即置 true（含恰好相等）并停止；15s 超时同样置 true。
- 结果顺序 = 遍历序，不排序；同 root 新搜索取消旧搜索（`CancelRegistry` 以 `rootPath` 为 key）。

单测：基本命中、include/exclude glob、大小写/整词/正则、gitignore、隐藏文件、5 MiB 跳过、每文件 100、总量截断（恰好等于）、字节列（多字节字符前有内容时 column 为字节偏移）、非 UTF-8 行、取消。

- [ ] **Step 2: 门禁 + 提交**

```bash
cargo test --manifest-path src-tauri/Cargo.toml -p ade-fs
git add -A && git commit -m "feat(fs): 内嵌搜索（rg 语义对齐）"
```

---

## Task 7: ade-fs watch

**Files:**
- Create: `src-tauri/crates/ade-fs/src/watch.rs`
- Test: 聚合函数单测 + `tests/watch.rs`（真 notify，宽松超时）

**Interfaces:**
- Produces: `FsWatcher::watch(root, subscriber_id) -> ()` / `unwatch(root, subscriber_id)` / `subscribe(callback)`；事件 payload `FsChangedPayload { worktreePath, events: Vec<FsChangeEvent> }`

- [ ] **Step 1: 聚合语义（TDD，纯函数先行）**

```rust
pub const WATCH_BATCH_TRAILING_MS: u64 = 150;
pub const WATCH_BATCH_MAX_WAIT_MS: u64 = 500;
pub const MAX_BATCHED_WATCHER_EVENTS: usize = 5000;
pub const WATCHER_IGNORE_DIRS: &[&str] = &[".git", "node_modules", "dist", "build", ".next", ".cache", "target", ".venv", "__pycache__"];
```

`coalesce_events(raw: Vec<RawEvent>) -> Vec<FsChangeEvent>`：同路径取最后事件；`delete→create` 保留两条（顺序 delete, create）；`create→delete` 抵消；`notify` 的 `Rename{from,to}` 折叠为 `delete(from)` + `create(to)`（若 `to` 在忽略目录则只 delete）；raw > 5000 → 单条 `overflow`（absolutePath=root）。

单测：每种规则各一例 + overflow。

- [ ] **Step 2: 订阅管理**

- root 级共享：`HashMap<root, RootWatch { watcher, subscribers: HashSet<String>, pending_drop_at: Option<Instant> }>`；最后订阅者离开 → 记录 `pending_drop_at = now + 30s`，定时器到期真正 drop；新订阅取消 pending。
- 安装失败 → 负缓存（不重试），并向下游发 `overflow`。
- 批处理：首事件起 150ms 尾沿 / 500ms 上限 flush；`isDirectory` 对 create/update 用 `stat` 探测（并发上限 8），delete 留 `None`。
- 事件通过 `ade-bridge` 的 emitter 回调送出（`Fn(FsChangedPayload)`）。

单测：refcount 宽限（注入时钟）、失败负缓存、isDirectory 探测。

- [ ] **Step 3: 集成测试**

`tests/watch.rs`：tempdir 安装 watch → 写文件 → 500ms 内收到 create/update；创建目录 → 收到带 `is_directory` 的事件；删除 → delete；创建后立即删除 → 抵消（无事件或 overflow 之外不出现 create）。

- [ ] **Step 4: 门禁 + 提交**

```bash
cargo test --manifest-path src-tauri/Cargo.toml -p ade-fs
git add -A && git commit -m "feat(fs): 文件监听（聚合/引用计数/overflow 语义）"
```

---

## Task 8: ade-bridge + orcinus-app（bootstrap 与 settings/ui/platform 域）

**Files:**
- Create: `src-tauri/crates/ade-bridge/Cargo.toml`、`src/lib.rs`、`src/state.rs`、`src/errors.rs`、`src/events.rs`、`src/commands/{mod.rs,settings.rs,ui.rs,platform.rs,app.rs}`、`src/specta_export.rs`
- Create: `src/bridge/real/generated/tauri-bindings.ts`（生成物，checked in）
- Modify: `src-tauri/Cargo.toml`（orcinus-app 依赖 ade-bridge 等）、`src-tauri/src/lib.rs`、`src-tauri/tauri.conf.json`（移除声明式窗口）、`package.json`（无新依赖）

**Interfaces:**
- Produces: `AppState { settings: SettingsStore, ui: UiStateStore, projects: ProjectsStore, fs: FsService, watchers: FsWatcher, app: AppHandle }`；commands：`settings_get/settings_set`、`ui_get/ui_set/ui_record_feature_interaction`、`platform_get`、`app_get_identity`；事件 `settings:changed`、`ui:stateChanged`

- [ ] **Step 1: state/errors/events + specta**

- `errors.rs`：`BridgeError`（`Core/Fs/Git/Store/Io` 包装）→ `Serialize` 为 `{ "message": string }`；`From` 实现。
- `events.rs`：`emit_json(app, event, payload)` 封装（`app.emit`）；常量事件名。
- `specta_export.rs`：所有命令入参/出参结构体派生 `specta::Type`；提供 `pub fn export_bindings() -> String`（`tauri_specta::Typescript::default().export(...)`），`#[cfg(test)]` 内比较 checked-in 文件：

```rust
#[test]
fn bindings_are_fresh() {
    let expected = include_str!("../../../../src/bridge/real/generated/tauri-bindings.ts");
    assert_eq!(export_bindings(), expected, "regenerate with: cargo run -p ade-bridge --bin export-bindings");
}
```

并加 `src/bin/export-bindings.rs` 写文件（执行者运行一次并提交产物）。若 `tauri-specta` 的 API 与上述不符，以 crate 文档为准并保持「导出内容与 checked-in 文件一致」这一断言不变。

- [ ] **Step 2: orcinus-app 改造（窗口 + init script + handler）**

`src-tauri/src/lib.rs`：

```rust
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            let state = AppState::initialize(app.handle())?;
            let bootstrap = state.bootstrap_payload();           // { settings, platform, schemaVersion }
            let script = format!(
                "window.__ADE_BOOTSTRAP__ = {};",
                serde_json::to_string(&bootstrap)?
            );
            tauri::WebviewWindowBuilder::new(app, "main", tauri::WebviewUrl::App("index.html".into()))
                .title("Orcinus")
                .inner_size(1440.0, 900.0)
                .initialization_script(&script)
                .build()?;
            app.manage(state);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            /* settings/ui/platform/app commands */
        ])
        .run(tauri::generate_context!())
        .expect("error while running Orcinus");
}
```

`tauri.conf.json` 删除 `app.windows` 声明（窗口改由 Rust 建）。

- [ ] **Step 3: settings/ui/platform/app 命令**

- `settings_get` → `{defaults ∪ stored}`；`settings_set(partial)` → 合并 + save + `emit("settings:changed", changed_keys)`（只含变更键）+ 返回完整对象；同时更新内存快照供 bootstrap。
- `ui_get` → defaults ∪ stored；`ui_set(partial)` → 例外合并 + save + `emit("ui:stateChanged", full)`；`ui_record_feature_interaction(id)` → 返回完整对象。
- `platform_get`：`{platform, osRelease, arch, shell, displayServer}`（macOS/Linux 用 `std::env::consts::OS` + `uname` 或 `sysinfo`？——用 `std::process::Command("uname")` 读 `-sr` 与 `$SHELL`；`displayServer` 非 Linux 返回 null）。
- `app_get_identity`：`{name:"Orcinus", version: env!("CARGO_PKG_VERSION")}`。

单测：命令函数体（不依赖 tauri runtime 的部分抽成纯函数）+ 错误映射形状。

- [ ] **Step 4: TS 侧最小接线并冒烟**

创建 `src/bridge/real/bootstrap.ts`（读 `window.__ADE_BOOTSTRAP__`）与 `settings.ts`（`getSync` 读 bootstrap、`get/set/onChanged` 走 invoke/listen）；`create-api.ts` 暂不改（Task 11 统一组装），但加一个临时冒烟测试文件 `src/bridge/real/bootstrap.test.ts` 断言缺失快照时 `getSync()` 返回 null 且 `platform.get()` 抛错。

```bash
pnpm vitest run src/bridge/real
cargo test --manifest-path src-tauri/Cargo.toml -p ade-bridge
pnpm dev   # 启动冒烟：窗口打开、无 bootstrap 报错；Ctrl-C 退出
git add -A && git commit -m "feat(bridge): Tauri 命令骨架、bootstrap 注入与 settings/ui/platform 域"
```

---

## Task 9: repos 与 worktrees 命令

**Files:**
- Create: `src-tauri/crates/ade-bridge/src/commands/{repos.rs,worktrees.rs}`、`src-tauri/crates/ade-core/src/models/repo.rs`
- Test: `src-tauri/crates/ade-bridge/tests/repos.rs`（tempdir + git init）

**Interfaces:**
- Produces: commands `repos_list/add/update/remove/reorder_for_host/pick_folder/pick_folders/pick_directory/is_git_available/get_default_create_project_parent`、`worktrees_list/list_all`；事件 `repos:changed`（空）、`worktrees:changed`（`{repoId}`）

- [ ] **Step 1: repos 命令（TDD）**

- `repos_add({path, kind?, displayName?})`：kind 默认 git；git kind `rev_parse_toplevel`（失败 → `{error: "Not a valid git repository: <path>"}`）；folder kind 原样路径；`displayName = trim || basename 去 .git`；Repo 字段：`id=new_uuid()`、`badgeColor="#737373"`（oracle `DEFAULT_REPO_BADGE_COLOR = REPO_COLORS[0]`）、`addedAt=now_ms`、`kind`、`externalWorktreeVisibilityLegacy=false`（git kind）。重复（`normalize_for_comparison` 比较）→ 返回既有 repo + `alreadyExisted:true`；保存 + 广播 `repos:changed` + 注册 fs 授权根。
- `repos_list`：读 projects store。
- `repos_update`：仅允许 spec 列出的字段；保存 + 广播；若 path 变更则更新授权根（A 不允许改 path——契约 Pick 里没有 path，天然满足）。
- `repos_remove({repoId})`：删除 + 撤销授权根 + 广播；不级联。
- `repos_reorder_for_host({orderedIds,hostId})`：按序写 `projectGroupOrder` + 广播；`hostId!='local'` 时返回 `{status:'rejected'}`。
- `repos_pick_folder/pick_folders/pick_directory`：`rfd::AsyncFileDialog`（macOS 主线程要求由 rfd 处理）；取消 `None`/`[]`。
- `repos_is_git_available`：`ade_git::is_available()`。
- `repos_get_default_create_project_parent`：读 settings `defaultWorktreeLocation`，未改默认（等于生成默认值）时回 `{{HOME}}/orcinus/projects`，否则返回配置值。

- [ ] **Step 2: worktrees 投影（TDD）**

- `worktrees_list({repoId})`：git repo → `ade_git::worktree_list` → 映射（字段按 spec §5.3；`displayName = branchShort || repo.displayName || basename`；prunable 跳过）；folder repo → 主 workspace + folderWorkspaces 倒序。
- `worktrees_list_all`：全 repo 合并。
- 事件：A 只在 repos 变更时发 `worktrees:changed {repoId}`（真实 git 元数据监听属 B）。

集成测试：tempdir `git init` + commit → `repos_add` → `worktrees_list` 单条且 `is_main_worktree`、`branch=refs/heads/<默认分支>`；folder kind → 主 workspace 投影；重复 add 幂等。

- [ ] **Step 3: 门禁 + 提交**

```bash
cargo test --manifest-path src-tauri/Cargo.toml -p ade-bridge
git add -A && git commit -m "feat(bridge): repos 注册表与最小 worktree 投影"
```

---

## Task 10: projectGroups 与 folderWorkspaces 命令

**Files:**
- Create: `src-tauri/crates/ade-bridge/src/commands/{project_groups.rs,folder_workspaces.rs}`、`src-tauri/crates/ade-core/src/models/{project_group.rs,folder_workspace.rs}`
- Test: `src-tauri/crates/ade-bridge/tests/catalogs.rs`

**Interfaces:**
- Produces: `project_groups_{list,create,update,delete,move_project,scan_nested,cancel_nested_scan,import_nested}` + 事件 `project-groups:scan-nested-progress`；`folder_workspaces_{list,create,update,delete,get_path_status}`；所有变更广播 `repos:changed`

- [ ] **Step 1: projectGroups（TDD）**

- `create`：`{name(min 1), parentPath?, parentGroupId?, createdFrom?}` → `id=new_uuid()`、`isCollapsed=false`、`color=null`、`tabOrder=max+1`、时间戳；保存 + 广播。
- `update/delete/move_project`：按 id 变换；`move_project` 设置 repo 的 `projectGroupId/projectGroupOrder`。
- `scan_nested`：有界遍历，选项归一照抄 oracle `nested-repo-scan-rules.ts:38-73`：`maxDepth` 默认 3（clamp 1..8）、`maxRepos` 默认 100（clamp 1..500）、`timeoutMs` 默认 null（clamp 500..30000）、`SKIPPED_DIRS`/`VCS_METADATA_DIRS` 与 `nested-repo-discovery.ts` 的 git marker 判定（`.git` 目录/文件或 HEAD+objects+refs 裸库）逐条对齐；进度事件 `{scanId, scanned, found}`；`cancel_nested_scan(scanId)`；结果 `NestedRepoScanResult` 字段对齐契约（`truncated/timedOut/stopped/durationMs`）。
- `import_nested`：对扫描结果逐路径 `repos_add`（kind 探测）→ 返回 `ProjectGroupImportResult{group, projects:[{path,projectId?,status,error?}], importedCount, alreadyKnownCount, failedCount}`。
- 单测：create 默认值/tabOrder 递增、move、scan 上限与取消、import 幂等计数。

- [ ] **Step 2: folderWorkspaces（TDD）**

- `create`：校验 group 存在、`folderPath ?? group.parentPath` 非空、路径存在且是目录（`get_path_status` 语义）→ 默认 `name=normalize(name, '<group> workspace')`、`connectionId=group.connectionId`、`comment=''`、`sortOrder=now`、`creatorProvenance={kind:'host'}`；保存 + 广播 + 授权根。
- `update/delete`：`delete → bool`；删除撤销授权根。
- `get_path_status`：三种 scope（folder-workspace/project-group/path）→ `{path, exists, reason?}`；TTL 10s 由渲染端缓存（Rust 不做缓存）。
- 单测：必填校验错误、默认值、路径状态三态。

- [ ] **Step 3: 门禁 + 提交**

```bash
cargo test --manifest-path src-tauri/Cargo.toml -p ade-bridge
git add -A && git commit -m "feat(bridge): projectGroups 与 folderWorkspaces 注册表"
```

---

## Task 11: TS 真实适配层与组装

**Files:**
- Create: `src/bridge/real/{fs,repos,projects,project-groups,folder-workspaces,ui,worktrees,app}.ts`
- Modify: `src/bridge/real/{settings,platform,bootstrap}.ts`（Task 8 已建最小版，此处补齐事件订阅与全部方法）
- Create: `src/bridge/real/*.test.ts`（契约）+ `src/bridge/real/parity.test.ts`
- Modify: `src/bridge/create-api.ts`、`src/bridge/mock/boot-namespaces.test.ts` 等既有 mock 期望测试（改为显式 `{mode:'mock'}`）、`src/bridge/install.ts`（读 `import.meta.env.VITE_ADE_BRIDGE`）

**Interfaces:**
- Produces: `createAdeApi(options?: { mode?: 'real' | 'mock' })`；各 `createXxxRealApi()`

- [ ] **Step 1: 适配层模式（TDD 一域先行）**

```ts
// src/bridge/real/repos.ts
import { invoke } from '@tauri-apps/api/core'
import type { PreloadApi } from '../../../shared/preload-api/api-types'
import { withMethodFallback } from '../unimplemented-fallback'

export function createReposRealApi(): PreloadApi['repos'] {
  return withMethodFallback<PreloadApi['repos']>('repos', {
    list: () => invoke('repos_list'),
    add: (args) => invoke('repos_add', { args }),
    // …其余实现方法
  })
}
```

事件域（`fs.onFsChanged`、`repos.onChanged`、`settings.onChanged`、`ui.onStateChanged`、`worktrees.onChanged`）：`listen('<event>', (e) => cb(e.payload))` 返回退订。

契约测试模式：

```ts
vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn(async () => []) }))
it('maps repos.add to repos_add with args envelope', async () => {
  await createReposRealApi().add({ path: '/tmp/x' })
  expect(invoke).toHaveBeenCalledWith('repos_add', { args: { path: '/tmp/x' } })
})
```

每个域断言：命令名 + 参数包裹形状（统一 `{ args }` 约定——Rust 命令签名为 `fn repos_add(args: AddArgs)`）+ 错误透传 + 事件名/退订。

- [ ] **Step 2: projects 域（TS 投影）**

`src/bridge/real/projects.ts`：`list` = `await invoke('repos_list')` → `projectHostSetupProjectionFromRepos(repos).projects`；`listHostSetups` 同源；`update` 返回应用 `localWindowsRuntimePreference` 的投影对象（不落盘）。

- [ ] **Step 3: 组装与 mock 回退**

`create-api.ts`：

```ts
export function createAdeApi(options?: { mode?: 'real' | 'mock' }): PreloadApi {
  const mode = options?.mode ?? (import.meta.env.VITE_ADE_BRIDGE === 'mock' ? 'mock' : 'real')
  if (mode === 'mock') return createMockAdeApi()
  const real = {
    fs: createFsRealApi(), repos: createReposRealApi(), projects: createProjectsRealApi(),
    projectGroups: createProjectGroupsRealApi(), folderWorkspaces: createFolderWorkspacesRealApi(),
    settings: createSettingsRealApi(), ui: createUiRealApi(), worktrees: createWorktreesRealApi(),
    app: createAppRealApi(), platform: createPlatformRealApi()
  }
  const partial: Partial<PreloadApi> = { ...real, /* 其余域 mock */ }
  return withUnimplementedFallback(partial)
}
```

既有 mock 期望测试（`boot-namespaces`、`onboarding-update-integration`、`create-api.test` 等）改为 `createAdeApi({ mode: 'mock' })` 或 `installAdeBridge({ mode: 'mock' })`；新增测试断言默认模式 = real、`VITE_ADE_BRIDGE=mock` 时 = mock（用 `vi.stubEnv`）。

- [ ] **Step 4: parity 测试**

`parity.test.ts`：对 mock 与 real（mock invoke 返回固定 fixtures）跑同一组契约断言：方法存在性、参数包裹、返回值形状关键字段、未实现方法 reject `UnimplementedBridgeError`。

```bash
pnpm vitest run src/bridge
rm -f tsconfig.tsbuildinfo && pnpm typecheck && pnpm build:web
git add -A && git commit -m "feat(bridge): 真实 IPC 适配层与按域组装（含 mock 回退）"
```

---

## Task 12: 全绿门禁、手工验收与收尾

**Files:**
- Create: `docs/phase1a-open-project-record.md`
- Modify: `docs/phase0-dead-code-inventory.md`（如需：把本次接真的域从 backlog 标注为完成）

- [ ] **Step 1: 全量门禁**

```bash
cargo test --manifest-path src-tauri/Cargo.toml
rm -f tsconfig.tsbuildinfo && pnpm typecheck && pnpm build:web
pnpm test
```

预期：cargo 全绿；`pnpm test` 0 失败（既有 3825 文件 + 新增测试）。

- [ ] **Step 2: 手工验收（`pnpm dev`，逐项记录证据）**

1. 添加本地 git 仓库（pickFolder → 侧栏出现项目与主工作树）与本地文件夹（folder kind）
2. 新建项目分组并把项目移入；新建文件夹工作区
3. 文件树展开、Monaco 打开文件、编辑保存；`git status` 外部查看文件确已修改
4. 外部编辑同一文件 → 文件树/编辑器收到刷新
5. 全局搜索（含 include/exclude 与正则）与 quick open 命中真实文件
6. 设置页改动少数分组 → 检查 `settings.json`；UI 布局变化 → 检查 `ui-state.json`
7. 重启应用 → 项目/分组/文件夹工作区/上次选中与布局恢复
8. 拒绝访问验证：`authorizeExternalPath` 之外路径读取被拒（文案正确）
9. `VITE_ADE_BRIDGE=mock pnpm dev` → 回退 mock 行为正常

- [ ] **Step 3: 写记录**

`docs/phase1a-open-project-record.md`：提交序、crate/命令清单、对齐语义与偏差（spec §10 四条）、测试证据（cargo/pnpm 计数）、手工验收结果、延后项（B/C、Windows、下载/日志尾随、projects.update 不落盘、mock 残渣）。

- [ ] **Step 4: 提交并交付**

```bash
git add -A && git commit -m "docs: Phase 1 子项目 A 收尾记录"
```

调用 superpowers:finishing-a-development-branch：确认 base `main`、全绿证据，向用户给出合并/PR/保留选项。

---

## 自检记录

- **Spec 覆盖**：spec §2.1 方法表逐项映射到 Task 8–11；§4 持久化→Task 2；§5.1 fs 四条→Task 4/5/6/7；§5.2/5.3 注册表与投影→Task 9/10；§5.4→Task 2/8；§5.5→Task 8/11；§7 测试策略→各任务测试步骤；§10 偏差→Task 1/2/8/11 实现方式。
- **占位符扫描**：无 TBD/TODO；易漂移常量（默认色 `#737373`、scanNested 默认 3/100/null 与跳过目录）均给逐字值或精确 oracle 文件:行；命令名/常量/文案逐字给出。
- **类型一致性**：`FsService` 方法名跨 Task 4–7 一致；`CancelRegistry` 在 Task 5/6 共用；`AppState` 字段在 Task 8–10 一致；TS 适配层统一 `{ args }` 包裹约定与 Rust 命令签名 `fn x(args: T)` 对应。
