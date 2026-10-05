# Phase 2 子项目 A：持久化基座 + 终端会话恢复（scrollback 快照 / 会话注册表 / agent resume）设计规格

- 日期：2026-10-01
- 状态：brainstorming 输出 + 落地核实修订（R1/R2/R3，2026-10-01 用户批准）。批准序列：恢复语义 = 快照重生、捕获 = transcript 扫描、resume = 自动触发、快照管道 = 方案 A（渲染层 serialize）→ **修订 R1 将落盘形态改为内联 buffers**（serialize 产物仍由渲染层产生，仅落盘位置从独立文件改为 session state 内联）。本规格为实现依据。
- 基线：`main@69e03d5`（Phase 1 三子项目已合入）
- 行为 oracle：`/Users/itsuka/CodeSpace/orca`（Electron 版，只读参照；下文 `orca:` 前缀均为其内路径）
- 上游规格：`docs/superpowers/specs/2026-09-14-ade-design.md`（§4 决策 4、§5 crate 表、§8 Phase 2）、`docs/superpowers/specs/2026-09-28-phase1c-terminal-pty-agent-design.md`（§2.4 spawn 参数处置、§2.5 明确不做）
- 前置事实：`docs/phase1c-terminal-pty-agent-record.md`（§5 手工验收：重启/reload 后画面空白、resume 不可用；§6.1 显式延后清单）；两侧代码面调查（2026-10-01 本会话两个探索代理 + 计划前精读，关键结论已内嵌，文件行号以调查时基线为准）

## 0. 落地核实修订记录（2026-10-01，用户批准）

- **R1（scrollback 内联化）**：fork 桌面路径的关停捕获把序列化文本**内联**写进 `TerminalLayoutSnapshot.buffersByLeafId`（每 leaf ≤ `TERMINAL_SCROLLBACK_SESSION_BUFFER_BYTE_LIMIT` 512 KiB，UTF-8 字节上限 + 插值探针截尾内置，`terminal-shutdown-layout-capture.ts:45-131`）；恢复主源即内联 buffers（`terminal-pane-lifecycle-primitives.ts:89-118` 对 refs null 容错）。`scrollbackRefsByLeafId`/`readTerminalScrollback` 为已删域 G 的移动端镜像残留。**原 §3.3 快照文件 + 写读命令 + GC 全部取消**；serialize 产物落 SQLite session state。
- **R2（退出路径宿主驱动 flush）**：orca 的 `stageBeforeUnloadSync` 依赖 Electron 同步 IPC，Tauri 无同步 invoke。改为 `RunEvent::ExitRequested` → `prevent_exit` → emit `session:flush-requested` → 渲染层捕获 + `session.flush` → ack 命令 → 宿主线程等 ack（2s 超时）→ `exit(0)`。配套翻转 `shouldPreserveTerminalScrollbackBuffers`（fork 裁掉本地 repo buffers 因有 daemon 兜底；ade 无 daemon，本地也保留）。
- **R3（采集降频）**：fork 因主线程卡顿移除过定期全量 re-serialize（orca `use-app-session-persistence.ts:266` #461）。原 5s/30s 防抖改为 60s 间隔（跳过 `document.hidden`，对齐既有 60s resume 捕获间隔）+ `visibilitychange→hidden` 立即捕获；优雅退出零损失由 R2 保证，60s 间隔只兜硬崩溃。
- **R4（退出握手迁移至 CloseRequested；2026-10-05 终审裁定，用户批准流程内）**：R2 原文假设 `RunEvent::ExitRequested { code: None }` 阶段渲染层可达——终审对照 vendored tauri-runtime-wry 2.11.4 证伪（该事件在窗口销毁后发出，emit 无人接收，且每次退出白付 2s 停顿）。握手迁移至 `WindowEvent::CloseRequested`（webview 存活），一次性 `CloseFlushLatch` 防 `window.close()` 重入死循环；`session-flush-persist` 对 `captureTranscripts` 拒绝免疫（patch/flush 必达）。详见 §3.4 终态。

## 1. 背景与目标

Phase 1C 交付了真实终端，但重启/reload 后终端画面空白、agent resume 不可用（1C 记录 §5、§6.1）。2A 以「快照重生」语义收口：

- **scrollback 快照与恢复**：重启/reload 后恢复 tabs、split 布局、scrollback 回看缓冲与 tab 标题；每 pane 在原 cwd 重生普通 shell 或 agent
- **agent 会话注册表与自动 resume**：休眠/关闭时捕获 providerSession（transcript 扫描），恢复时若有记录则自动重生 agent 会话
- **SQLite 持久化基座**：`ade-store` 增 rusqlite，承接工作区会话态；现有 JSON store 不迁移

**验收（自动 + 手工）**：终端对话 → 退出重启 → tabs/split/scrollback/标题恢复、shell 原 cwd 重生；claude 对话 → 重启 → 自动 resume 续聊（历史在、可继续）；codex 同验；split 两 pane 恢复；`location.reload()` 后画面恢复（尽力语义，见 §6）；关 tab 后重启无残留会话记录；SQLite 库文件损坏时应用可正常启动（降级无持久化）。自动化门禁：`cargo test --workspace` 全绿 + `pnpm test` 全绿 + `pnpm typecheck && pnpm build:web` exit 0。

## 2. 范围

### 2.1 实现面

| 面 | 载体 | 说明 |
|---|---|---|
| SQLite 基座 | `ade-store/src/sqlite.rs`（新） | rusqlite bundled、单库 WAL、`user_version` 迁移框架、`workspace_sessions` 单表（§3.1） |
| session 域命令 | `ade-bridge/src/commands/session.rs`（新） | `session_get/set/patch/flush`（§3.2） |
| 退出 flush 协议 | `src-tauri/src/lib.rs` + `session.rs` | `session:flush-requested` 事件 + `session_flush_ack` 命令 + ExitRequested 编排（§3.4） |
| transcript 扫描 | `ade-bridge/src/commands/agent_sessions.rs`（新） | `agent_sessions_resolve_capture`（§3.3） |
| session 域 real | `src/bridge/real/session.ts`（新） | get/set/patch/flush/setSync + flush-requested 订阅（§5.1） |
| 本地 buffers 保留 | `src/shared/workspace-session-terminal-buffers.ts` | 翻转为本地 repo 也保留（R2，§5.2） |
| 定期/隐藏捕获 | `use-app-session-persistence.ts` | 60s 间隔 + visibilitychange（R3，§5.3） |
| resume 捕获 | `agent-transcript-capture.ts`（新） | OSC 身份 + transcript 扫描 → 注册表（§5.4） |
| 恢复接线 | 既有 fork 机械接真 | 启动水合/布局恢复/冷恢复 resume 均已就位（§5.5） |

### 2.2 明确不做（2B+ 或独立延后项，防蔓延）

hook server、agent TUI 状态识别接真（2B；2A 仅消费 OSC 标题身份做捕获输入）；通知/未读（2B）；daemon（宿主进程存活穿越重启，1C §6.1 独立延后项——本规格恢复语义为快照重生，应用退出即活进程终止）；delivery-health、OSC7 cwd、pending cap、kill 未知 id 容忍、incarnationId 穿透数据面（各自独立延后）；projects/ui_state JSON store 迁 SQLite；`session.cache`/`session.remoteWorkspace` 子域接真；`readTerminalScrollback` 接真（refs 为域 G 残留，保持 null）；窗口关闭/`location.reload()` 的同步保证（§6 尽力语义）；GitHub Provider/automations/AI Vault（2D/2E）。

## 3. 架构

### 3.1 SQLite 基座（ade-store 演进）

- 依赖：`rusqlite`（`bundled` feature，workspace 统一版本）；DB 文件 `app_data_dir/ade.sqlite`（`-wal`/`-shm` 同目录）
- `ade-store/src/sqlite.rs`：
  - `Store::open(path)`：建连接，`PRAGMA journal_mode=WAL`、`PRAGMA busy_timeout=5000`、`foreign_keys=ON`
  - 连接包装 `Mutex<Connection>`（进程内单写者；session 域为唯一写者，无高并发面）
  - 迁移框架：`PRAGMA user_version` + 有序迁移数组 `[(1, migrate_v1), ...]`，open 时逐个应用（事务内）；2B/2D 的表从该数组追加
  - open 容错：库文件损坏（open 或首查询失败）→ 改名 `ade.sqlite.corrupt-<unix_ms>` 后重建空库，日志留痕
  - `StoreError` 增 SQLite 变体（`#[error(transparent)] Rusqlite(#[from] rusqlite::Error)`）
- v1 schema（仅一表；休眠 agent 注册表是 session state 字段 `sleepingAgentSessionsByPaneKey`，随行持久化，不建独立表）：

```sql
CREATE TABLE workspace_sessions (
  key        TEXT PRIMARY KEY,   -- WorkspaceSessionState 顶层字段名
  value      TEXT NOT NULL,      -- JSON 文本
  updated_at INTEGER NOT NULL    -- unix ms
);
```

- 退出路径：`src-tauri/src/lib.rs` 现有 `RunEvent::Exit` 分支（flush_pending_writes → shutdown_all）追加：session flush + `PRAGMA wal_checkpoint(TRUNCATE)`（Exit 阶段的兜底；主保证是 §3.4 的 flush-requested 编排）

### 3.2 session 域 = 不透明 JSON 文档存储

`WorkspaceSessionState`（`src/shared/workspace-session-state-types.ts:36-135`）是 30+ 字段的 fork 巨型嵌套类型。Rust 侧**不做类型镜像**：

- 命令载荷用 `String`（JSON 文本）；TS 侧 stringify/parse 后按既有类型使用
- 存储粒度：**顶层字段一行**；`patch`（`WorkspaceSessionPatch = Partial<WorkspaceSessionState>`，workspace-session-state-types.ts:136）语义 = 逐顶层键整键替换；`set` = 整状态替换（删除库中存在而 payload 缺失的键 + 覆盖 payload 内各键）；`get` = 全行组装为 JSON 对象返回
- 写者唯一（渲染层 session real 实现），Rust 不校验内容 schema；宽松容忍未知键（与 ade-store 既有「宽松归一化」哲学一致）
- scrollback 序列化文本以 `terminalLayoutsByTabId[tabId].buffersByLeafId` 内联随行（R1）；宿主不理解内容
- `flush` 语义 = SQLite 层面本就同步写（每 patch 即事务提交），`session_flush` 为显式 no-op 兼容位 + 返回 `wal_checkpoint(PASSIVE)` 结果（供退出编排确认落盘）

### 3.3 transcript 扫描（providerSession 捕获）

命令：`agent_sessions_resolve_capture(args: {cwd: string, agentKind: string, windowFromMs: number, windowToMs: number}) -> AgentProviderSessionMetadata | null`

- 返回类型复用 fork 契约 `src/shared/agent-session-resume.ts:26-36` `AgentProviderSessionMetadata { key: 'session_id' | 'conversation_id', id, transcriptPath? }`（小类型，specta 镜像）
- **claude**：目录 `~/.claude/projects/<munged-cwd>/`（`/Users/a/b` → `-Users-a-b`，Claude Code 目录名规则）；列 `*.jsonl`，mtime ∈ [windowFromMs, windowToMs]，取最新；`key='session_id'`，id = 文件名 stem（UUID），transcriptPath 随行
- **codex**：目录 `~/.codex/sessions/`（`YYYY/MM/DD/rollout-*.jsonl` 递归）；读各文件头部（首 4KiB）匹配 cwd 字符串，mtime 在窗内取最新；`key='session_id'`（codex 的 CLI resume id 即 rollout 文件名 uuid），id 从文件名提取，transcriptPath 随行
- 无命中/目录不存在/无 home/`agentKind` 非 `claude|codex` → 返回 null（不报错）；扫描超时上限（单目录 500ms，防挂载卷卡顿）
- 调用时机：渲染层 `agent-transcript-capture` 模块在捕获点调用（§5.4）；窗口 = [应用启动时刻, now]（2A 不做 per-pane 起始时间追踪）；返回值写入 `sleepingAgentSessionsByPaneKey[paneKey]` 随 session.patch 落库

### 3.4 退出 flush 协议（R2；终审修订 R4：迁移至 CloseRequested）

Tauri 无同步 IPC，beforeunload 阻塞写盘不可行（orca `stageBeforeUnloadSync` 的机制在 ade 不可复制）。**且终审（对照 vendored tauri-runtime-wry 2.11.4 源码）证实：`RunEvent::ExitRequested { code: None }` 在最后一个窗口（连同 webview/JS 上下文）销毁之后才发出**（`lib.rs:4309-4322`，`TaoWindowEvent::Destroyed` 内）——此阶段 emit 事件无人接收。协议终态（webview 存活期握手）：

1. `Builder::on_window_event` 拦 `WindowEvent::CloseRequested { api, .. }`（webview 此刻仍存活）：`begin_close_with_session_flush(window)` 返回 bool——一次性 `CloseFlushLatch`（AtomicBool）首次为 false：emit `session:flush-requested` + spawn 等待线程 + 返回 true → `api.prevent_close()`；latch 已置（含 `window.close()` 重入的 CloseRequested——`close()` 会再次触发该事件，不 gate 则死循环）则返回 false → 不拦，直接放行关闭
2. 渲染层 `real/session.ts` 订阅 `session:flush-requested` → 执行关停捕获（复用 `shutdownBufferCaptures` 逐 tab `capture()`）→ 全量 `session.patch` + `session.flush`（不依赖 150ms 防抖订阅器，避免与 ack 竞态）→ bridge 层在 handler 结束后（无论成败）调 `session_flush_ack`
3. 宿主 ack 命令置 Condvar；等待线程超时 2s 后无论结果 `window.close()`（放行被拦的关闭）；最后一个窗口关闭后 Tauri 默认退出 → `RunEvent::Exit` 跑既有收尾（flush_pending_writes + checkpoint_truncate + shutdown_all）
4. 边界（留档）：flush 窗口内（≤2s）用户再次关窗 = 截断 in-flight flush、立即关闭（尊重用户意图，latch 放行）；**Cmd+Q / `AppHandle::exit(code)` 路径不经 CloseRequested，不受握手覆盖**（与 R2 批准范围一致：窗口关闭退出）

### 3.5 resume 重生路径（fork 冷恢复机械接真，宿主零改动）

- 冷恢复消费链**完整存在于 fork**：`bindBuildColdRestoreAgentResumeStartup`（`pty-connection/cold-restore-resume-startup.ts:19-99`）从 store 读 `sleepingAgentSessionsByPaneKey`（`getSleepingRecordForPane`，paneKey = `${tabId}:${leafId}` 跨重启稳定）→ `buildAgentResumeStartupPlan`（`shared/tui-agent-startup.ts`）构建启动行 → 替代 spawn 携带 `{command, env, resumeProviderSession, launchToken}`；启动行经 1C 已生效的 `command` 面「spawn 即投递」进 shell（1C §10 偏差 2 语义；tty 输入缓冲保证 shell 就绪前键入不丢失）
- 2A 补齐的仅是**记录捕获**（§5.4）——消费侧零改动
- **偏差备案（对 fork）**：fork 桌面在 daemon 存活时走 warm-reattach 优先；ade 无 daemon，恢复即冷恢复，`bindBuildColdRestoreAgentResumeStartup` 的 live-entry 优先逻辑原样保留（`agentStatusByPaneKey` 恒空时自然落到 sleeping record 分支）
- resume spawn 失败（CLI 缺失等）走现有 spawn 失败错误态；**注册表记录保留**，下次恢复可重试

## 4. 数据流

### 4.1 采集（上行）

```
捕获触发（互不排斥）：
  A. 既有：tab 休眠 / 关 tab / force-park → captureTerminalShutdownLayout（fork 已接）
  B. 新增（R3）：60s 间隔（跳过 document.hidden）逐 tab 捕获
  C. 新增（R3）：visibilitychange→hidden 立即逐 tab 捕获
  D. 新增（R2）：session:flush-requested → 逐 tab 捕获 + session.flush
      ↓ captureTerminalShutdownLayout：serializeWithAbsoluteCursor → buffersByLeafId（≤512KiB 截尾）
      ↓ setTabLayout → zustand terminalLayoutsByTabId
createSessionWriteSubscriber（150ms 防抖 + 逐字段变更门控，fork 已接）
      ↓ buildWorkspaceSessionPatch（SESSION_RELEVANT_FIELDS 变更键）
      ↓ pruneLocalTerminalScrollbackBuffers —— 2A 翻转为本地 repo 也保留（R2）
session.patch（顶层键整键替换）→ session_patch 命令 → SQLite workspace_sessions（每 patch 即事务提交）
```

### 4.2 恢复（下行）

```
启动/reload → session.get → WorkspaceSessionState
  → tabs/split 布局/活跃 tab 还原（use-app-startup-hydration 既有消费点，mock 空态转真态）
  → 每 leaf：buffersByLeafId 内联回放进 xterm（fork restore 机械；refs null 容错不动）
  → 每 paneKey 查 sleepingAgentSessionsByPaneKey：
      有记录 → bindBuildColdRestoreAgentResumeStartup 构建 resume 启动行 → spawn 即投递
      无记录 → 普通 shell（同 cwd）
```

## 5. 渲染层接线

### 5.1 session 域 real 实现（`src/bridge/real/session.ts`，新）

- `get/set/patch` → `session_get/session_set/session_patch`（JSON 文本信封）；`get` 解析后按 `WorkspaceSessionState` 使用；错误上抛由调用方 catch（启动水合已有容错）
- `flush` → `session_flush`；`setSync` → fire-and-forget patch（无 ack，与 `pty.write` 吞错先例同形）
- `readTerminalScrollback` → 保持 null（域 G 残留，`hydrateTerminalScrollbackRefs` null 容错已验证；不删契约方法）
- 订阅 `session:flush-requested` → 关停捕获 + flush + `session_flush_ack`（§3.4；捕获回调由 `use-app-session-persistence` 注入，避免 real 层依赖渲染层 store）
- 接入：`create-api.ts` RealDomains 增 `session`；parity 键集门禁同步

### 5.2 本地 buffers 保留翻转（R2）

`src/shared/workspace-session-terminal-buffers.ts`：`repoNeedsRendererCapturedScrollback` 翻转为**恒 true**（注释说明：ade 无 daemon，渲染层捕获是唯一 scrollback 持久化；函数签名与调用点不动，最小 diff）。

### 5.3 定期/隐藏捕获（R3）

`use-app-session-persistence.ts` 增两个 effect：

- 60s 间隔：`document.visibilityState !== 'hidden'` 时逐 tab 调 `shutdownBufferCaptures.get(tabId)?.()`（默认 `includeLocalBuffers:true`）
- `visibilitychange` 监听：转 hidden 时立即同上

### 5.4 resume 捕获模块（`agent-transcript-capture.ts`，新；终审后验收修订 R5：门改为 transcript 存在性扫描）

- 输入：store 快照；枚举 terminal tabs × layouts leaves → paneKey（`${tabId}:${leafId}`）
- **捕获门（R5 修订）**：原文以 OSC 标题身份（`titleHasAgentName`，仅 `claude|codex`）作门——**验收证伪**：ade 无 shell 集成（1C 延后项），zsh 与 claude TUI 均不发 OSC 标题，标题恒为默认值，身份门永不打开。修订为：**无现有 providerSession 记录的活 pane（有 ptyId）一律扫描**——claude 先、codex 兜底，命中即以该 agent kind 建记录；双未命中 → 无记录（恢复为普通 shell）。transcript 的 mtime 落在本会话窗口内 = 「本会话期间该 cwd 跑过 agent」的存在性证据。误报语义（运行过并已退出的 pane 被自动 resume）在已批准的自动 resume UX 内；精度由 2B hooks 恢复
- **会话去重（R5 补充，验收轮 4）**：一个 providerSession id 至多被一个 paneKey 认领——认领集合 = 既有记录的 id + 本轮扫描先命中者；同 id 的兄弟 pane（split 同 cwd 场景）不建记录，恢复为普通 shell。先来者优先，位置歧义可接受（同 worktree 同布局）
- cwd：`window.api.pty.getCwd(ptyId)`（`ptyIdsByLeafId` 活会话）
- 命中则按 `SleepingAgentSessionRecord` 形状构造记录（`state` 用 `'waiting'`——`AgentStatusState` 无 `'idle'` 值、`prompt: ''`、`origin: 'quit'|'live'` 对齐调用方模式）并入 `sleepingAgentSessionsByPaneKey`
- 接入点：`captureAllSleepingAgentSessions` 动作末尾触发（`agent-status-recovery-actions.ts:54`，quit/periodic 两模式都跑）；异步执行、失败静默（无记录 = 恢复为普通 shell）

### 5.5 恢复接线（多数为既有机械接真）

- `use-app-startup-hydration`/`use-app-session-persistence`：session.get/patch 从 mock 空态转真态（调用点已存在：`app-shell/use-app-session-persistence.ts:108`、`use-app-startup-hydration.ts:147`）
- 布局恢复 + scrollback 回放：`terminal-pane-layout-restore.ts` 读 `buffersByLeafId` 的机械已就位
- 自动 resume：`bindBuildColdRestoreAgentResumeStartup` 机械已就位（§3.5）
- mock 侧同步：`workspace-session-api.ts` mock 工厂补 `set/flush/setSync/readTerminalScrollback` 桩（get/patch 已有）

## 6. 错误处理与边界

| 场景 | 行为 |
|---|---|
| SQLite 打不开（损坏/磁盘满） | session 命令返回 Err；渲染层 catch 降级为「无持久化」运行（同 mock 空态），应用不崩；DB 损坏时 open 改名 `ade.sqlite.corrupt-<ts>` 后重建（日志留痕） |
| buffers 超 512 KiB/leaf | fork 截尾机械内置（插值探针 + UTF-8 clamp），无需新逻辑 |
| transcript 扫描无命中 | capture 返回 null，无记录，恢复为普通 shell |
| resume 启动行投递失败 | 走现有 spawn 失败错误态；记录保留 |
| 退出编排超时（渲染层 2s 无 ack） | 宿主线程 `exit(0)` 放行；已收到的 patch 因每 patch 即事务提交已落库，最多丢最后一轮捕获（≤60s，R3） |
| 窗口关闭 vs reload | R2 编排只覆盖应用级退出（ExitRequested）；reload/窗口关闭为尽力语义（visibilitychange→hidden 触发的捕获 + 150ms 防抖 patch 竞态，多数落地；`location.reload()` 为 dev 场景，接受） |
| 多窗口并发 | 2A 单窗口（现状），session 域单写者 |
| flush-requested 时渲染层已死 | ack 超时路径兜底（上一行） |

## 7. 测试与门禁

### 7.1 Rust

- `ade-store`：迁移框架（v1 建表 + 追加假想 v2 的升级演练）、CRUD、`updated_at` 维护、损坏库文件改名重建、WAL checkpoint
- `ade-bridge` session 命令：patch/get round-trip、顶层键整键替换、set 整状态替换（缺失键删除）、未知键容忍、flush checkpoint
- 退出编排：flush-requested → ack 置位 → 等待返回；无 ack 超时路径（2s 缩短为测试值）
- `agent_sessions`：fixture 目录测试（munged cwd 正/误例、时窗过滤、多文件取最新、codex 三级目录递归、头部 cwd 匹配、null 分支、超时）
- bindings 新鲜度 + `export_lists_every_command` 清单更新 + 命令名锁定

### 7.2 TS（vitest）

- `real/session.ts`：信封、错误形态、JSON round-trip、setSync 吞错、flush-requested → 捕获回调 + flush + ack 序列
- parity 门禁：session 子域键集（mock/real 双侧）
- buffers 保留翻转：`pruneLocalTerminalScrollbackBuffers`/`shouldPreserveTerminalScrollbackBuffers` 单测更新（本地 repo 保留）
- R3 捕获：60s 间隔跳过 hidden、visibilitychange 触发（fake timers）
- `agent-transcript-capture`：身份解析 → 扫描调用参数（cwd/agentKind/时间窗）→ 记录构造与合并（无身份/非 claude-codex/已有 providerSession 的跳过分支）
- mock/real 一致性

### 7.3 手工验收

见 §1 验收清单（对话→重启恢复、claude/codex 自动 resume 续聊、split 恢复、reload 尽力恢复、损坏库降级、退出后无残留记录的关 tab）。

## 8. 风险与偏差备案

1. **退出编排的平台行为差异**：`prevent_exit` + 后台线程 `exit(0)` 在 macOS/Windows 的事件循环语义需实测；超时 2s 对启动盘慢的机器可能截断大 payload patch（每 patch 即提交，已落库部分不丢）。
2. **serialize 产物与 xterm 版本耦合**：xterm 系已 pnpm patch 钉版本（`pnpm-workspace.yaml:16-24`），升级需重验恢复 parity（既有 headless 夹具覆盖）。
3. **codex transcript 目录结构稳定性**：`~/.codex/sessions` 布局无官方契约，扫描实现按当前观测布局 + 宽松匹配（头部 cwd 包含即可）；失败降级 null，不阻塞恢复。
4. **session state 顶层键整键替换的写放大**：layouts dict（含内联 buffers）随 patch 全量重写（MB 级、低频）；SQLite 单行 TEXT 写入可承受；若实测成为瓶颈，后续可拆 per-tabId 行（schema 兼容）。
5. **OSC 身份检测的假阳/假阴**：标题启发式可能漏识别（无记录 → 普通 shell，无损）或误识别（扫描无命中 → 同样降级，无损）；失败模式全部收敛到「不 resume」，无破坏性。
6. **与 fork 语义的偏差**：本地 repo buffers 保留（R2，fork 为 daemon 而裁剪）；定期 60s 捕获（R3，fork 移除过 3min 全量）；恢复无 warm-reattach（无 daemon，冷恢复即唯一路径）。
