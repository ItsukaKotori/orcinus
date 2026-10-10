# Phase 2F Agent Session History（本地会话历史）Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 右侧栏 "Agents" 面板接真：扫描本地 Claude/OpenCode 会话 → 列表/标题/恢复准备/删除（仅 Claude 到废纸篓）/窗口焦点；渲染层零改动。

**Architecture:** 新 crate `ade-ai-vault`（roots/解析/scanner/缓存/删除/resume/标题）；`ade-bridge` 5 命令；TS `real/ai-vault.ts` 6 方法显式 + 4 方法 fallback。参照实现 `/Users/itsuka/CodeSpace/orca`（只读），`orca:` 前缀均为其内路径。

**Tech Stack:** Rust（ade-ai-vault、rusqlite 0.32 bundled、objc2 0.6/objc2-foundation 0.3（trash））、TypeScript（Vitest、Tauri IPC）。

**Spec:** `docs/superpowers/specs/2026-10-09-agent-session-history-design.md`（执行者须同时阅读）

## Global Constraints

- Rust 依赖：`ade-ai-vault` 新增 `serde/serde_json/thiserror/rusqlite(0.32,bundled)/objc2(0.6)/objc2-foundation(0.3)`（后两者 workspace 既有版本，仅 macOS）；不新增任何 npm 依赖。
- bindings 唯一生成方式：`cargo run -p ade-bridge --bin export-bindings`（workdir `src-tauri`）；`bindings_are_fresh` 校验；新命令名进 `specta_export.rs` 两处清单。
- 提交信息中文 conventional + `Co-Authored-By: Claude Code <noreply@anthropic.com>`。
- cargo 命令在 `src-tauri` 下执行；pnpm 在仓库根。
- 仅 macOS/POSIX；可选字段省略不物化 null。
- 参照逐字：取消结果 `{cancelled:true}`（非抛错）、OpenCode DB 只读 + `query_only`、删除拒绝码集合、`file-history/` 绝不触碰、OpenCode 删除拒绝。
- 渲染层零改动（既有约 70 个 ai-vault 测试全绿是硬门禁）。

---

### Task 1: crate 骨架 + roots + Claude 解析

**Files:**
- Create: `src-tauri/crates/ade-ai-vault/Cargo.toml`
- Create: `src-tauri/crates/ade-ai-vault/src/lib.rs`
- Create: `src-tauri/crates/ade-ai-vault/src/roots.rs`
- Create: `src-tauri/crates/ade-ai-vault/src/claude.rs`
- Test: 同 crate `#[cfg(test)]` + fixtures 就地在测试里写临时文件

**Interfaces:**
- Consumes: 无（首任务）。
- Produces（后续任务依赖，名称固定）：
  - 会话行采用 **`serde_json::Value`**（键名 camelCase 对齐 `src/shared/ai-vault-types.ts` 的 `AiVaultSession`；Rust 不重复 TS schema）。
  - `pub struct ScanIssue { pub agent: String, pub kind: String, pub path: String, pub message: String }`
  - `roots::claude_projects_root(home: &Path) -> PathBuf`
  - `roots::opencode_data_dir(home: &Path, xdg: Option<&str>) -> PathBuf`
  - `claude::scan_claude_sessions(projects_root: &Path, limit: usize) -> (Vec<Value>, Vec<ScanIssue>)`

- [ ] **Step 1: 创建 crate**

`Cargo.toml`（镜像 ade-hooks/ade-automations 惯例）：

```toml
[package]
name = "ade-ai-vault"
version = "0.0.1"
edition = "2021"

[dependencies]
ade-core = { path = "../ade-core" }
rusqlite = { version = "0.32", features = ["bundled"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
thiserror = "2"

[target.'cfg(target_os = "macos")'.dependencies]
objc2 = "0.6"
objc2-foundation = "0.3"
```

- [ ] **Step 2: 写失败测试（Claude 行为锚）**

行为锚（对照 `orca:src/main/ai-vault/session-scanner-claude-*` 与测试 `session-scanner.test.ts`、`session-scanner-claude-cwd-drift.test.ts`、`session-scanner-preview-window-truncation.test.ts`、`session-scanner-claude-title.test.ts`）：

1. 单个 `<uuid>.jsonl`：首行/末行时间戳 → `createdAt/updatedAt`（ISO 字符串原样）；`cwd` 取**最后一条有 cwd 的行**；`branch`/`model` 同理；`messageCount` 计 user+assistant 行；预览取尾部窗口（≤5 条 user/assistant，每条 text 截断 + `previewMessagesTruncated` 标记逻辑）；`modifiedAt` = 文件 mtime；`id` = `local:claude:<sessionId>:<filePath>`；`resumeCommand` = `cd '<cwd>' && claude --resume '<sessionId>'`（无 cwd → `claude --resume '<id>'`）；`filePath` 为绝对路径。
2. 坏行跳过（半行/非 JSON）不失败；空文件仍产出行（`messageCount:0`；`isAiVaultSessionRecoverableEmpty` 由渲染层判断）。
3. `<uuid>/` 子代理目录：其中的 `.jsonl` **不**产生独立行（`subagents/` prune）。
4. 排序：按 `modifiedAt` 降序。
5. 上限：超过 limit 时截断（scope 保证在 Task 3 scanner 统一做，本任务仅按 limit 截断）。

- [ ] **Step 3: 跑测试确认失败 → 实现 → 通过**

Run: `cargo test -p ade-ai-vault claude`

- [ ] **Step 4: 提交**

```bash
git add src-tauri/crates/ade-ai-vault src-tauri/Cargo.lock
git commit -m "feat(ai-vault): crate 骨架、roots 与 Claude 会话解析"
```

---

### Task 2: OpenCode 解析（旧版文件 + SQLite 只读）

**Files:**
- Create: `src-tauri/crates/ade-ai-vault/src/opencode.rs`
- Modify: `src-tauri/crates/ade-ai-vault/src/lib.rs`

**Interfaces:**
- Produces:
  - `opencode::scan_opencode_sessions(data_dir: &Path, limit: usize) -> (Vec<Value>, Vec<ScanIssue>)`
  - `opencode::opencode_db_paths(data_dir: &Path, env_db: Option<&str>) -> Vec<PathBuf>`（`opencode.db` 优先 + `opencode-*.db`；`env_db == ":memory:"` → 空集合）
  - 行 `agent:"opencode"`；`filePath`：旧版 = session JSON 路径，SQLite = **DB 路径**；跨形态去重在 Task 3 scanner 按 `agent+sessionId` 完成（SQLite 行压旧版），本任务只需保证各自解析正确。

- [ ] **Step 1: 写失败测试**

锚（`orca:src/main/ai-vault/session-scanner-opencode-*` 与各测试）：
1. 旧版：`storage/session/<proj>/<sid>.json`（`record.id` 优先、文件茎回退）+ `storage/message/<sid>/*.json` 按 `time.created` 排序；坏消息文件跳过。
2. SQLite：临时 DB 建 `session(id, time_created, time_updated, parent_id, time_archived, title?, directory?, model?, tokens_*?, message_count?)` 最小列集；list 排除 `parent_id NOT NULL` 与 `time_archived NOT NULL`（列存在时）；排序按 effective recency（`time_updated>0 ? time_updated : time_created`）；`message`/`part` 缺列时 preview 降级为空且不报错；`message_count` 需 `message.session_id/data` 可读。
3. 只读保证：以 `SQLITE_OPEN_READ_ONLY` + `pragma query_only=ON` 打开；对不可写/忙 DB（WAL 未 flush 模拟）→ 返回 scope issue（文案含 "write-ahead log" 或 "busy"，对齐参照 `session-scanner-opencode-sqlite-open.ts:97-105`），其余源不受影响。
4. `opencode_db_paths`：`OPENCODE_DB` 绝对/相对/`-` 语义 + canonical 优先。
5. 跨 DB 同 id → 取 `time_updated` 最大者；`filePath` 为各自 DB 路径。

- [ ] **Step 2: 实现要点**

- rusqlite：`Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)` + `pragma_update(None, "query_only", "ON")` + `busy_timeout(1500ms)`。
- 逐列探测：`pragma table_info(<table>)` 收集列名集合，按存在性拼 SQL（对齐参照 `opencode-usage/schema-helpers.ts:14-31` 的 `tableExists/columnExists` 语义）。
- list SQL 照参照 `session-scanner-opencode-sqlite-list.ts:32-46`；metadata/preview 建议只做：title（`title` 或 `summary`——DB 无则目录名/首条 user 文本兜底）、cwd=directory、messageCount（user/assistant 计数查询，可读时）、预览取最近 ≤5 条 user/assistant（text 由 `part` 拼，可读时；不可读 → 空 + `previewMessagesTruncated` 不置位）。
- 旧版 roots：`data_dir/storage`。

- [ ] **Step 3: 跑测试 + 提交**

Run: `cargo test -p ade-ai-vault opencode`

```bash
git add src-tauri/crates/ade-ai-vault
git commit -m "feat(ai-vault): OpenCode 解析（旧版文件 + SQLite 只读）"
```

---

### Task 3: scanner + 取消 + 60s 缓存

**Files:**
- Create: `src-tauri/crates/ade-ai-vault/src/scanner.rs`
- Create: `src-tauri/crates/ade-ai-vault/src/cache.rs`
- Modify: `src-tauri/crates/ade-ai-vault/src/lib.rs`

**Interfaces:**
- Consumes: Task 1/2 的 `scan_*`。
- Produces:
  - `pub struct ScanRequest { pub scope_paths: Vec<String>, pub limit: usize, pub force: bool, pub request_token: Option<String> }`
  - `pub fn scan_sessions(config: &VaultConfig, request: &ScanRequest, cancel: &CancelFlag) -> Value`（返回 `AiVaultListResult` JSON：`{sessions, issues, scannedAt, cancelled?}`）
  - `pub struct ListCache { /* 60s TTL */ } impl ListCache { pub fn get_or_scan(...); pub fn invalidate(&self); }`
  - 取消注册表：`pub struct CancelRegistry`（`register(token) -> CancelFlag`、`cancel(token)`、`finish(token)`；沿 `ade-bridge` `GitCancelRegistry` 模式）。
  - 合并规则：两源 rows 拼接 → 去重按 **`agent + sessionId`** 分组（OpenCode 组内存在 `filePath` 以 `.db` 结尾的 SQLite 行时取之，压过同 id 旧版文件；其余保留）→ `modifiedAt` 降序 → `limit` 截断；`scopePaths`（≤64）：Claude 行 `cwd` 命中 scope 的候选不受 limit 截断（scope 保证）；`limit*2` 候选上限；扫描中每文件前检查 `cancel` → 命中则返回 `{..., cancelled:true}`（不抛）。

- [ ] **Step 1: 写失败测试**

1. 缓存：同 key 60s 内二次调用不重扫（以计数器/注入时钟验证）；`force` 绕过；`invalidate` 后重扫。
2. 去重与排序：SQLite 行压同 id 旧版；`modifiedAt` 降序。
3. scope 保证：scope 命中 cwd 的 Claude 行在超 limit 时仍在结果中。
4. 取消：扫描中途 `cancel(token)` → 结果 `cancelled:true` 且不抛。
5. issues 透传（两源 issues 合并）。

- [ ] **Step 2: 实现 → 跑测试 → 提交**

Run: `cargo test -p ade-ai-vault scanner`

```bash
git add src-tauri/crates/ade-ai-vault
git commit -m "feat(ai-vault): scanner、取消注册表与 60s 列表缓存"
```

---

### Task 4: 删除（Claude，两段校验 + 废纸篓）

**Files:**
- Create: `src-tauri/crates/ade-ai-vault/src/delete.rs`
- Create: `src-tauri/crates/ade-ai-vault/src/trash.rs`
- Modify: `src-tauri/crates/ade-ai-vault/src/lib.rs`

**Interfaces:**
- Produces:
  - `delete::validate_delete_target(home: &Path, agent: &str, file_path: &str) -> DeleteValidation`（枚举：`Allowed { transcript: PathBuf, companions: Vec<PathBuf> }` / `Rejected { reason: &'static str }`）
  - `delete::execute_delete(home: &Path, agent: &str, file_path: &str) -> Value`（返回 `AiVaultDeleteSessionResult` JSON）
  - `trash::trash_path(path: &Path) -> Result<(), String>`（macOS：objc2-foundation `NSFileManager::trashItemAtURL_resultingItemURL_error`）
- 拒绝码（对齐 `src/shared/ai-vault-session-deletion.ts`）：`invalid-path`、`unsupported-agent`、`synthetic-path`、`path-outside-known-roots`、`undiscoverable-path`、`no-session-directory`、`unexpected-target-kind`、`non-local-host`（本地恒不触发）。
- 顺序：空/非法 → `invalid-path`；`agent != "claude"` → `unsupported-agent`；含 `#` → `synthetic-path`；**以 `.db` 结尾 → `unsupported-agent`（附加防线）**；resolve 后须位于 `<home>/.claude/projects` 内；须为 `.jsonl` 且父目录非 `subagents`/`session-env` → 否则对应拒绝。

- [ ] **Step 1: 写失败测试**

1. 拒绝矩阵：OpenCode 行（`db` 路径）→ `unsupported-agent`；`db#id` → `synthetic-path`（agent=claude 时也命中 `#`）；`/tmp/x.jsonl`（roots 外）→ `path-outside-known-roots`；`.../projects/x.txt` → `undiscoverable-path`；目录 → `unexpected-target-kind`。
2. 允许：临时构造 `~/.claude/projects/<proj>/<uuid>.jsonl` + `<uuid>/`（子代理）+ `xxx/session-env/<uuid>/` → companions = [`<uuid>/`, `session-env/<uuid>/`] 顺序先子代理目录后 session-env（执行顺序：companions → transcript）。
3. 执行：真删（临时 home）→ 原路径全部消失；`file-history/` 目录保持原样；对已不存在路径再次调用 → `deleted`（幂等）。
4. trash 落点：主断言 = 原路径消失且结果为 `deleted`（非 `failed`，即未走 unlink 降级）；副断言 = 测试前后枚举 `~/.Trash`，新增项的 basename 与 `<uuid>.jsonl` 一致（并发环境下若副断言不稳，保留主断言并在报告披露）。

- [ ] **Step 2: 实现 + 测试 + 提交**

Run: `cargo test -p ade-ai-vault delete`

```bash
git add src-tauri/crates/ade-ai-vault
git commit -m "feat(ai-vault): Claude 会话删除（两段校验 + 废纸篓）"
```

---

### Task 5: resume 准备 + 标题

**Files:**
- Create: `src-tauri/crates/ade-ai-vault/src/resume.rs`
- Create: `src-tauri/crates/ade-ai-vault/src/titles.rs`
- Modify: `src-tauri/crates/ade-ai-vault/src/lib.rs`

**Interfaces:**
- Produces:
  - `resume::prepare_session_resume(_args) -> Value` → 恒 `{ "useRealCodexHome": true }`（无托管 Codex home；披露）。
  - `titles::resolve_titles(home: &Path, requests: &[(String, String, Option<String>)]) -> Value`（`[{agent, sessionId, title}]`，仅 `claude`；≤64/请求截断；路径按 sessionId 在 projects 下定位 `<uuid>.jsonl`；标题抽取规则与 Task 1 一致，未命中 → 该请求省略）。
- 标题抽取复用 Task 1 的解析函数（导出 `claude::read_claude_title(path) -> Option<String>`）。

- [ ] **Step 1: 写失败测试**

1. 标题：fixture jsonl 命中 → 返回；不存在 sessionId → 空列表；请求 >64 → 截断为 64；agent=codex → 无结果。
2. resume：任意输入 → `{useRealCodexHome:true}`。

- [ ] **Step 2: 实现 + 测试 + 提交**

Run: `cargo test -p ade-ai-vault`

```bash
git add src-tauri/crates/ade-ai-vault
git commit -m "feat(ai-vault): 恢复准备透传与 Claude 标题解析"
```

---

### Task 6: ade-bridge 命令 + AppState + bindings

**Files:**
- Create: `src-tauri/crates/ade-bridge/src/commands/ai_vault.rs`
- Modify: `src-tauri/crates/ade-bridge/src/commands/mod.rs`
- Modify: `src-tauri/crates/ade-bridge/src/state.rs`（`ai_vault: Arc<AiVaultService>`：`VaultConfig{home}` + `ListCache` + `CancelRegistry`）
- Modify: `src-tauri/crates/ade-bridge/Cargo.toml`（`ade-ai-vault = { path = "../ade-ai-vault" }`）
- Modify: `src-tauri/crates/ade-bridge/src/specta_export.rs`
- Test: `src-tauri/crates/ade-bridge/tests/ai_vault.rs`
- Regenerate: `src/bridge/real/generated/tauri-bindings.ts`

**Interfaces:**
- Commands（camelCase args，`run_blocking`）：
  - `ai_vault_list_sessions({limit?, force?, scopePaths?, unlimited?, requestToken?}) -> AiVaultListResult`
  - `ai_vault_resolve_session_titles({executionHostScope?, requests}) -> {titles}`
  - `ai_vault_cancel_list_sessions({requestToken}) -> null`
  - `ai_vault_prepare_session_resume({agent, filePath, codexHome?, sessionId?}) -> {useRealCodexHome, substituteCodexHome?}`
  - `ai_vault_delete_session({agent, filePath, sessionId?, executionHostId?}) -> AiVaultDeleteSessionResult`
- `unlimited` 接受但忽略（恒 1000）。

- [ ] **Step 1: 写失败测试（tests/ai_vault.rs，state-free `*_impl` 风格）**

临时 home（注入 `VaultConfig`）：构造 claude fixture + opencode fixture → list 返回两源行；titles 命中；delete 拒绝矩阵 + Claude 真删；cancel no-op；prepare 透传。

- [ ] **Step 2: 实现 + bindings**

Run: `cargo test -p ade-bridge --test ai_vault`
Run: `cargo run -p ade-bridge --bin export-bindings && cargo test -p ade-bridge`

- [ ] **Step 3: 提交**

```bash
git add src-tauri/crates/ade-bridge src/bridge/real/generated/tauri-bindings.ts
git commit -m "feat(bridge): aiVault 五命令与 AppState 装配"
```

---

### Task 7: TS real/ai-vault.ts + create-api/parity + 全量

**Files:**
- Create: `src/bridge/real/ai-vault.ts`
- Create: `src/bridge/real/ai-vault.test.ts`
- Modify: `src/bridge/create-api.ts`
- Modify: `src/bridge/real/parity.test.ts`
- Modify: `src/bridge/create-api.test.ts`（如需）

**Interfaces:**
- Consumes: Task 6 命令；`subscribeToEvent`；`withMethodFallback`。
- Produces: `createAiVaultRealApi(): PreloadApi['aiVault']`。

- [ ] **Step 1: 写失败测试**

- 5 命令路由（命令名 + `{args}` 信封 + 参数形状，含 `cancelListSessions` 的 `{requestToken}`）；
- `onWindowFocused`：订阅 `tauri://focus`（`subscribeToEvent`），回调触发/退订（mock event 模块）；
- 4 方法（`searchSessions`/`searchStatus`/`listSubagentSessions`/`getFirstUserPrompt`）仍 reject `UnimplementedBridgeError`。

- [ ] **Step 2: 实现 + 全量回归**

`create-api.ts` `RealDomains` 加 `'aiVault'` + 工厂；parity 增加 surfaceCase（显式 6 / missing 4）。

Run: `pnpm vitest run src/bridge && pnpm typecheck`
Run: `pnpm test`（全量；性能抖动隔离复跑；其它失败 STOP + BLOCKED）

- [ ] **Step 3: 提交**

```bash
git add src/bridge/real/ai-vault.ts src/bridge/real/ai-vault.test.ts \
  src/bridge/create-api.ts src/bridge/real/parity.test.ts src/bridge/create-api.test.ts
git commit -m "feat(bridge): aiVault real 域接线与 parity"
```

---

### Task 8: 门禁复跑 + 收尾记录

**Files:**
- Create: `docs/phase2f-agent-session-history-record.md`
- Modify: `docs/superpowers/plans/2026-10-09-agent-session-history.md`（勾选复选框）

- [ ] **Step 1: 全量门禁**

Run: `cargo test --workspace`（workdir src-tauri）
Run: `rm -f tsconfig.tsbuildinfo && pnpm typecheck && pnpm build:web`
Run: `pnpm test`

- [ ] **Step 2: 写收尾记录**

结构照 `docs/phase2e-automations-record.md`：§1 范围与验收 / §2 提交清单 / §3 门禁证据 / §4 手工验收（6 项，待用户复核）/ §5 偏差与边界备案（spec §7 九条 + 实现中发现的新偏差）/ §6 已知边界与后续（搜索索引、子代理、首条提示、其余 16 解析器、SSH/WSL、持久解析缓存、独立扫描进程、结构化守卫、trash 回读断言的环境限制）。

手工验收清单（写入记录）：面板列出 Claude/OpenCode 会话；Claude 恢复启动终端；Claude 删除到废纸篓（含子代理目录）；OpenCode 删除被拒；Claude 标签页标题；焦点刷新。

- [ ] **Step 3: 提交**

```bash
git add docs/phase2f-agent-session-history-record.md docs/superpowers/plans/2026-10-09-agent-session-history.md
git commit -m "docs: Phase 2F 实施记录与门禁证据（Agent 会话历史）"
```

---

## Self-Review

**Spec coverage:**
- §3.1 内核模块（roots/claude/opencode/scanner+cache/delete+trash/resume/titles）→ Tasks 1–5；§3.2 命令 → Task 6；§3.3 TS 域 → Task 7。
- §4 数据流 → 各任务测试 + Task 7 路由；§5 错误处理 → Task 1–5 测试矩阵；§6 门禁 → Task 8。
- §7 风险 → Task 8 记录（含 trash 断言限制、unlimited 忽略、OpenCode 拒绝）。

**Placeholder scan:** 各任务含行为锚（参照 file:line）与测试清单；trash 测试给出降级断言策略（环境相关）并指示报告披露——这是有意的环境边界，不是占位。

**Type consistency:** 命令名 `ai_vault_*`（5）在 Tasks 6/7 一致；`ScanIssue{kid}` 值域 `host|scope|notice`；`AiVaultListResult`/`AiVaultDeleteSessionResult`/`AiVaultSessionTitlesResult` 键名对齐 `src/shared/ai-vault-*.ts`；拒绝码集合与 `ai-vault-session-deletion.ts` 一致；`filePath` 语义（Claude=jsonl、OpenCode 旧版=json、SQLite=db）在 Tasks 2/3/4 一致。
