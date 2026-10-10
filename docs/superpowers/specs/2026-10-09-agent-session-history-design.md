# Phase 2 子项目 F：Agent Session History（本地会话历史）设计规格

- 日期：2026-10-09
- 状态：brainstorming 输出（解析器 = Claude + OpenCode、OpenCode 删除 = 拒绝同参照、能力集 = 最小集——均已批准；本规格为实现依据）
- 基线：`main@5619ae7d`
- 行为 oracle：`/Users/itsuka/CodeSpace/orca`（Electron 版，只读参照；`orca:` 前缀均为其内路径）
- 上游规格：`docs/superpowers/specs/2026-09-14-ade-design.md`（§8 Phase 2「AI Vault」——实际为 Agent Session History）
- 前置事实（2026-10-09 本会话三探索代理）：
  - **命名澄清**：orca 的 "AI Vault" = 用户可见的 **Agent Session History**（右侧栏 "Agents" 页签，`AiVaultPanel.tsx`）；不是密钥保险库（密钥存储是 `plugin-secrets-store`/Claude Keychain 等独立设施）。
  - **渲染层全量已移植且挂载**：面板/详情/删除/恢复/拖拽/标题同步 gate（`AiVaultTabTitleSyncGate` 全局挂载）/刷新 hook（取消、30s 节流、焦点刷新）/共享逻辑（过滤/排序/深度/恢复命令构造/删除可删性）约 107 个共享+渲染层文件；契约 `src/shared/preload-api/api/ai-vault-api.ts`（10 方法）。今日 `window.api.aiVault.*` 全部 reject（无 mock、无 real 域、`create-api.ts` 未列 `aiVault`），面板显示 "ade bridge method not implemented yet"。
  - **参照宿主侧**（`orca:src/main/ai-vault/`，~100 文件）：扫描器（18 agent 解析器、独立 fork 服务进程、持久解析缓存 schema v2）、解析上限（默认 1000 / RPC 2000）、60s 列表缓存 + 15s 主机腿缓存、请求级取消/合并、删除两段校验 + `shell.trashItem`、标题（claude/codex）、首条提示/子代理按需读、搜索索引（FTS5，独立子系统、main 未装配）。
  - **Claude 存储形态**：`~/.claude/projects/<slug>/<uuid>.jsonl`（追加写、行级 JSON）；同目录 `<uuid>/` 为子代理 transcript（扫描 Prune，仅在父行按需展开）；兄弟 `session-env/<uuid>/`、`file-history/`（用户文件回滚副本，**绝不删**）。
  - **OpenCode 存储形态**：旧版文件 `$XDG_DATA_HOME/opencode/storage/session/<projectId>/<sessionId>.json` + `storage/message/<sessionId>/*.json`；1.17.x 迁移到 **SQLite** `~/.local/share/opencode/opencode.db`（+ `opencode-*.db` 同胞、`OPENCODE_DB` 覆盖）；读取一律 `readonly + query_only`（`session-scanner-opencode-sqlite-open.ts:38-46`），逐列探 schema（`session`,`message`,`part`），list SQL 排除 `parent_id` 子会话与 `time_archived`；SQLite 行 `filePath` = **DB 路径**（`session-scanner-opencode-sqlite.ts:260-270`）。
  - **OpenCode 删除不可行（参照裁定）**：不在 `AI_VAULT_DELETABLE_AGENTS`（`shared/ai-vault-session-deletion.ts:20-46`，原因：SQLite 行非文件）；旧版文件删除需级联 `message/<id>/`、`part/`、`session_diff/`、`share/`、子会话递归，参照不实现不校验，上游 issue #5734 记录「只删 session 文件 → Session not found」；用户已批准本切片拒绝 OpenCode 删除。
  - **trash 能力**：端口已有 objc2/objc2-foundation（2B.1 通知原生引入）；rusqlite 0.32 bundled（ade-store）。

## 1. 背景与目标

GitHub 外部集成与其余外部项按用户决定暂停后，Phase 2 剩余本地项之一是会话历史。本子项目（2F）交付**本地最小切片**，使 "Agents" 面板从报错变为可用：

- **扫描/解析**：Claude（JSONL）与 OpenCode（旧版 JSON + SQLite 只读）→ 统一 `AiVaultSession` 行
- **能力**：列表（取消/60s 缓存）、标题（Claude）、恢复准备（透传）、删除（仅 Claude，到废纸篓）、窗口焦点事件
- **渲染层零改动**：面板/详情/恢复/删除/拖拽/标题同步全部沿用既有实现

**验收（自动 + 手工）**：面板列出真实 Claude/OpenCode 会话（含 OpenCode SQLite 行）；恢复动作打开终端并携带正确 resume 命令；删除把 Claude 会话（含 companions）移入废纸篓；Claude 标签页出现会话标题；窗口焦点触发刷新。自动化门禁：`cargo test --workspace` 全绿 + `pnpm test` 全绿 + `pnpm typecheck && pnpm build:web` exit 0。

## 2. 范围

### 2.1 实现面

| 面 | 载体 | 说明 |
|---|---|---|
| 内核 crate | `ade-ai-vault`（新） | roots/解析/scanner/cache/delete/resume（§3.1） |
| Tauri 命令 | `ade-bridge` 5 命令 | list/titles/cancel/prepare/delete（§3.2） |
| 窗口焦点 | `real/ai-vault.ts` 订阅 Tauri 窗口焦点 | `onWindowFocused`（§3.3） |
| 桥接 | `real/ai-vault.ts`（新）+ create-api/parity | 6 方法显式；4 方法维持 fallback |
| 前端 | 无改动 | 面板等既有约 107 文件原样工作 |

### 2.2 明确不做（防蔓延）

搜索索引（`ai-vault-search/*`，FTS5，契约保留 fallback）；子代理列表 `listSubagentSessions`；首条提示 `getFirstUserPrompt`；Codex/Cursor/Gemini 及其余 14 个解析器（`resolveSessionTitles` 的 codex 请求返回空）；SSH/远端/运行时的 `aiVault.*`（本地单一主机）；**OpenCode 删除**（一律 `unsupported-agent`，同参照；另含 `.db` 路径硬拒绝）；持久解析缓存（跨启动）；独立扫描服务进程（`spawn_blocking` 替代）；结构化会话所有权守卫；WSL；`unlimited` 扫描（参数接受但恒 1000 上限，披露）；拖拽层保持既有（依赖 prepare/resume，具备即可工作）。

## 3. 架构

### 3.1 Rust 内核（`ade-ai-vault`）

- **`roots.rs`**：`claude_projects_root(home)=~/.claude/projects`；`opencode_data_dir()` = `$XDG_DATA_HOME/opencode` 或 `~/.local/share/opencode`；`OPENCODE_DB` 覆盖（`-` 忽略、绝对用之、相对拼 data dir）；`opencode_db_paths()`（`opencode.db` 优先 + `opencode-*.db` 同胞）。测试可注入 home/env。
- **`claude.rs`**：`~/.claude/projects/*/<uuid>.jsonl` 解析：
  - 逐行 JSON（坏行跳过计数）；`sessionId` 取行内 id 或文件茎；`cwd`（末尾行优先）、`branch`、`model`、`title`（行内 summary/首条 user 文本截断，对齐参照标题抽取）；`messageCount`（user/assistant 行数）；`totalTokens`（usage 汇总，尽力）；预览消息取**尾部窗口**（最近 5 条 user/assistant，截断标记）；`modifiedAt` 文件 mtime；`createdAt` 首行时间戳。
  - **不进列表**：同目录 `<uuid>/`（子代理目录）不产生行；`file-history/`、`session-env/` 不参与发现。
  - `resumeCommand`：`cd '<cwd>' && claude --resume '<sessionId>'`（POSIX 单引号转义，cwd 缺省省略前缀）。
- **`opencode.rs`**：
  - 旧版：`storage/session/<proj>/<id>.json` + `storage/message/<id>/*.json`（按 `time.created` 排序）；`filePath` = session JSON 路径。
  - SQLite：rusqlite `OpenFlags::SQLITE_OPEN_READ_ONLY` + `pragma query_only=ON`、busy timeout 1500ms；逐列探 schema；list SQL 照参照（`parent_id IS NULL`、`time_archived IS NULL` 条件按列存在性添加、按 effective recency 排序、LIMIT）；metadata/preview（最近 100 条消息窗口、文本 part 拼接）照参照；`filePath` = DB 路径；同一 sessionId 出现在多 DB → 取最新；**任何 DB 读失败 → 该 DB 记 scope issue，其余源照常**。
  - `resumeCommand`：`cd '<cwd>' && opencode --session '<sessionId>'`。
- **`scanner.rs`**：发现两源 → 候选（上限 `limit*2`，默认 1000）→ 解析（`spawn_blocking` 内 8 并发有界）→ 去重（同 id：SQLite 压过旧版文件）→ 排序（effective 最近活动降序）→ `AiVaultListResult{sessions, issues, scannedAt, cancelled?}`；`scopePaths`（≤64）内 cwd 的 Claude 候选不受上限截断（scope 保证）；`requestToken` 注册表支持取消（取消返回 `{cancelled:true}`，不抛）。
- **`cache.rs`**：60s 列表缓存（key = 排序后 scopePaths 序列 + limit + host=local）；`force:true` 绕过；删除成功后失效。
- **`delete.rs`**：
  - 校验（顺序对齐参照）：空/非法路径 → `invalid-path`；agent ≠ claude → `unsupported-agent`（OpenCode/其余一律）；路径含 `#` → `synthetic-path`；**路径以 `.db` 结尾 → `unsupported-agent`（本切片附加防线，防 OpenCode DB 误删）**；非本地 host（本地恒真）；resolved 路径必须位于 `~/.claude/projects` 内且满足可发现性（`.jsonl`）→ 否则 `path-outside-known-roots`/`undiscoverable-path`。
  - 执行：先 companions（`<uuid>/` 子代理目录（若存在）、兄弟 `session-env/<uuid>/`（若存在））后 `<uuid>.jsonl`；`lstat` 类型核对 + `realpath` 二次包含核对；ENOENT 视为成功（幂等）；**绝不触碰 `file-history/`**；trash 用 objc2-foundation `trashItemAtURL`（macOS；失败 → `failed` + error 文案）。成功 → 失效列表缓存。
  - 结果映射 `AiVaultDeleteSessionResult`（`deleted`/`rejected{reason}`/`failed{error}`）。
- **`resume.rs`**：`prepareSessionResume` 语义——端口无托管 Codex home/账号体系 → 恒返回 `{useRealCodexHome: true}`（对 claude/opencode 行无副作用；披露）。
- **`titles.rs`**：`resolveSessionTitles`（仅 claude）：按 `sessionId` 定位 `<uuid>.jsonl`，读 title/summary；找不到 → 该请求无结果；上限 64/请求，超限截断（对齐契约）。

### 3.2 Tauri 命令（ade-bridge）

`ai_vault_list_sessions(args?)`（`requestToken` 注册）、`ai_vault_resolve_session_titles({requests})`、`ai_vault_cancel_list_sessions({requestToken})`、`ai_vault_prepare_session_resume({agent, filePath, codexHome, sessionId?})`、`ai_vault_delete_session({agent, filePath, sessionId?, executionHostId?})`；均 camelCase args、`run_blocking` 包阻塞 IO；specta 注册 + bindings 重生成。AppState 持有 `AiVaultService`（缓存/取消注册表/roots 配置）。

### 3.3 TS 域（`real/ai-vault.ts`）

- 显式：`listSessions`、`resolveSessionTitles`、`cancelListSessions`、`prepareSessionResume`、`deleteSession`（命令路由）+ `onWindowFocused`（`@tauri-apps/api/window` 的 `getCurrentWindow().onFocusChanged` 转发，返回退订）。
- 维持 fallback：`searchSessions`、`searchStatus`、`listSubagentSessions`、`getFirstUserPrompt`（面板对后两者有 catch 降级；搜索无 UI 消费）。
- `create-api.ts` `RealDomains` 加 `'aiVault'`；parity `surfaceCases` 显式 6 / missing 4；`create-api.test.ts` 如涉及补路由断言。

## 4. 数据流

1. 面板挂载/刷新 → `useAiVaultSessionRefresh` 调 `listSessions({scopePaths, limit, requestToken, force?})` → 命令 → 扫描（缓存命中直接返回）→ 行渲染。
2. 刷新触发器：窗口焦点事件（`onWindowFocused`）、30s 强制节流、agent-session-id 变化、SSH 重连分支（本地恒不触发）——均为既有 hook 行为。
3. 恢复：行动作 → `prepareSessionResume`（透传）→ 既有 `resumeCommand` 构造/拖拽层 → 终端以 resume 命令启动。
4. 删除：确认 → `deleteSession` → 校验/trash/缓存失效 → 既有刷新。
5. 标题同步 gate → `resolveSessionTitles`（Claude）→ 标签页标题（手动重命名优先，既有规则）。

## 5. 错误处理与边界

| 场景 | 行为 |
|---|---|
| 单源/单文件解析失败 | 跳过或记 issue（`kind:'scope'/'notice'`），整体不抛 |
| 扫描取消 | 返回 `{cancelled:true}`（渲染层识别） |
| OpenCode DB 忙/WAL 不可读 | 该 DB 记 scope issue（文案对齐参照），其余源照常 |
| OpenCode 删除请求 | `rejected{unsupported-agent}`（UI 本就不显示删除） |
| `.db` 路径删除请求 | `rejected{unsupported-agent}`（附加防线） |
| 路径越界/非法 | 对应拒绝码，绝不 trash |
| trash API 失败 | `failed{error}`（不降级为 unlink） |
| `requestToken` 未知 | no-op |
| `unlimited:true` | 忽略（仍 1000 上限，披露） |
| Codex 标题请求 | 无结果（该 agent 未实现解析器） |

## 6. 测试与门禁

### 6.1 Rust

- **claude**：fixture JSONL（多行、坏行跳过、cwd 漂移取尾部、标题抽取、预览尾部窗口、token 汇总、`subagents/` 目录不产生行、mtime/createdAt）。
- **opencode 旧版**：session+message fixture、排序、坏消息文件跳过。
- **opencode SQLite**：临时 DB 建表（`session`/`message`/`part` 各族 schema 变体）→ list（排除子会话/归档、跨 DB 去重取新）、metadata/preview、缺表降级、WAL 忙 → scope issue。
- **scanner**：去重（SQLite 压旧版）、排序、上限（含 scope 保证）、取消（中途 requestToken → `cancelled`）。
- **cache**：60s TTL、force 绕过、删除失效、key 含 scopePaths。
- **delete**：拒绝码矩阵（unsupported agent/`#`/`.db`/越界/不可发现/非法）+ 临时目录真删（companions 顺序、`file-history/` 不动、ENOENT 幂等、trash 落地到 `~/.Trash` 可回读性断言——以“原路径消失且 Trash 出现同名项”近似）。
- **resume/titles**：命令构造引用转义（含空格/引号 cwd）、标题命中/未命中。
- bindings 新鲜度 + 命令清单（5 新命令）。

### 6.2 TS

- `real/ai-vault.ts`：5 命令路由断言 + `onWindowFocused` 订阅/退订（mock window API）+ 4 fallback 方法仍 reject。
- create-api/parity 更新；既有约 70 个 ai-vault 渲染层测试全绿（面板行为不变）。

### 6.3 手工验收

1. 打开 Agents 面板 → 列出真实 Claude 会话与 OpenCode 会话（SQLite 行可见）；
2. 点某个 Claude 会话恢复 → 新终端按 resume 命令启动；
3. 删除一个 Claude 会话 → 原路径消失、废纸篓可见（含子代理目录）；
4. 删除菜单对 OpenCode 行不可用/拒绝；
5. Claude 标签页出现会话标题；手动重命名优先；
6. 切换应用焦点回来 → 列表自动刷新。

### 6.4 门禁

`cargo test --workspace`、`pnpm typecheck && pnpm build:web`、`pnpm test`。

## 7. 风险与偏差备案

1. **无持久解析缓存**：跨启动冷扫（上限 1000、Claude JSONL 全量行读）；大目录首扫较慢（披露）。
2. **无独立扫描进程**：`spawn_blocking` + 8 并发 + 取消注册表；超长扫描占用阻塞线程（可接受）。
3. **OpenCode 删除拒绝**：同参照；附加 `.db` 路径防线；SQLite 行 `filePath` 即 DB 路径，绝不可 trash。
4. **SQLite 只读限制**：WAL 未 flush/被锁 → 该 DB 降级（文案对齐参照「exit OpenCode cleanly」语义）。
5. **能力裁剪**：子代理/首条提示/搜索/结构化守卫未接，面板对应区域降级（既有 catch）；`resolveSessionTitles` 仅 Claude。
6. **`unlimited` 忽略**：契约参数存在但恒 1000 上限（披露）。
7. **trash 平台**：仅 macOS（objc2-foundation）；非 macOS 分支不顺延（POSIX 前提）。
8. **`prepareSessionResume` 恒 `{useRealCodexHome:true}`**：端口无托管 Codex home/账号体系（2A 现状），对 claude/opencode 无副作用。
9. **scope 保证的近似**：Claude scope 内 cwd 候选不受上限截断按参照实现；OpenCode 无 cwd 级 scope 保证（SQLite list 无 cwd 过滤），与参照一致但披露。
