# Phase 2 子项目 A：持久化基座 + 终端会话恢复（scrollback 快照 / 会话注册表 / agent resume）设计规格

- 日期：2026-10-01
- 状态：brainstorming 输出（恢复语义 = 快照重生、捕获 = transcript 扫描、resume = 自动触发、快照管道 = 渲染层 serialize 方案 A，均已由用户批准；本规格为实现依据）
- 基线：`main@69e03d5`（Phase 1 三子项目已合入）
- 行为 oracle：`/Users/itsuka/CodeSpace/orca`（Electron 版，只读参照；下文 `orca:` 前缀均为其内路径）
- 上游规格：`docs/superpowers/specs/2026-09-14-ade-design.md`（§4 决策 4、§5 crate 表、§8 Phase 2）、`docs/superpowers/specs/2026-09-28-phase1c-terminal-pty-agent-design.md`（§2.4 spawn 参数处置、§2.5 明确不做）
- 前置事实：`docs/phase1c-terminal-pty-agent-record.md`（§5 手工验收：重启/reload 后画面空白、resume 不可用；§6.1 显式延后清单）；两侧代码面调查（2026-10-01 本会话两个探索代理，关键结论已内嵌为 §2/§3 依据，文件行号以调查时基线为准）

## 1. 背景与目标

Phase 1C 交付了真实终端，但重启/reload 后终端画面空白、agent resume 不可用（1C 记录 §5、§6.1）。2A 以「快照重生」语义收口：

- **scrollback 快照与恢复**：重启/reload 后恢复 tabs、split 布局、scrollback 回看缓冲与 tab 标题；每 pane 在原 cwd 重生普通 shell 或 agent
- **agent 会话注册表与自动 resume**：休眠/关闭时捕获 providerSession（transcript 扫描），恢复时若有记录则自动带 `--resume` 重生 agent 会话
- **SQLite 持久化基座**：`ade-store` 增 rusqlite，承接工作区会话态；现有 JSON store 不迁移

**验收（自动 + 手工）**：终端对话 → 退出重启 → tabs/split/scrollback/标题恢复、shell 原 cwd 重生；claude 对话 → 重启 → 自动 resume 续聊（历史在、可继续）；codex 同验；split 两 pane 恢复；`location.reload()` 后画面恢复；关 tab 后重启无孤儿快照文件；SQLite 库文件损坏时应用可正常启动（降级无持久化）。自动化门禁：`cargo test --workspace` 全绿 + `pnpm test` 全绿 + `pnpm typecheck && pnpm build:web` exit 0。

## 2. 范围

### 2.1 实现面

| 面 | 载体 | 说明 |
|---|---|---|
| SQLite 基座 | `ade-store/src/sqlite.rs`（新） | rusqlite bundled、单库 WAL、`user_version` 迁移框架、`workspace_sessions` 单表（§3.1） |
| session 域命令 | `ade-bridge/src/commands/session.rs`（新） | `session_get/set/patch/flush` + `session_write/read_terminal_scrollback`（§3.2/§3.3） |
| transcript 扫描 | `ade-bridge/src/commands/agent_sessions.rs`（新） | `agent_sessions_resolve_capture`（§3.4） |
| session 域 real | `src/bridge/real/session.ts`（新） | get/set/patch/flush/readTerminalScrollback/setSync/writeTerminalScrollback（§5.1） |
| 快照采集 | 终端 pane 生命周期 hook | serialize 防抖上行（§5.2） |
| 恢复接线 | 启动水合 + 布局恢复路径 | 大部分 fork 机械已就位，接真即活（§5.3） |
| resume 重生 | 渲染层恢复路径 | 直接 argv spawn（§3.5） |

### 2.2 明确不做（2B+ 或独立延后项，防蔓延）

hook server、agent TUI 状态识别（2B）；通知/未读（2B）；daemon（宿主进程存活穿越重启，1C §6.1 独立延后项——本规格恢复语义为快照重生，应用退出即活进程终止）；delivery-health、OSC7 cwd、pending cap、kill 未知 id 容忍、incarnationId 穿透数据面（各自独立延后）；projects/ui_state JSON store 迁 SQLite；`session.cache`/`session.remoteWorkspace` 子域接真；fork 的「向 shell 键入 resume 行」路径（§3.5 偏差备案）；GitHub Provider/automations/AI Vault（2D/2E）。

## 3. 架构

### 3.1 SQLite 基座（ade-store 演进）

- 依赖：`rusqlite`（`bundled` feature，workspace 统一版本）；DB 文件 `app_data_dir/ade.sqlite`（`-wal`/`-shm` 同目录）
- `ade-store/src/sqlite.rs`：
  - `Store::open(path)`：建连接，`PRAGMA journal_mode=WAL`、`PRAGMA busy_timeout=5000`、`foreign_keys=ON`
  - 连接包装 `Mutex<Connection>`（进程内单写者；session 域为唯一写者，无高并发面）
  - 迁移框架：`PRAGMA user_version` + 有序迁移数组 `[(1, migrate_v1), ...]`，open 时逐个应用（事务内）；2B/2D 的表从该数组追加
  - `StoreError` 增 SQLite 变体（`#[error(transparent)] Rusqlite(#[from] rusqlite::Error)`）
- v1 schema（仅一表；休眠 agent 注册表是 session state 字段 `sleepingAgentSessionsByPaneKey`，随行持久化，不建独立表）：

```sql
CREATE TABLE workspace_sessions (
  key        TEXT PRIMARY KEY,   -- WorkspaceSessionState 顶层字段名
  value      TEXT NOT NULL,      -- JSON 文本
  updated_at INTEGER NOT NULL    -- unix ms
);
```

- 退出路径：`src-tauri/src/lib.rs` 现有 `RunEvent::Exit` 分支（flush_pending_writes → shutdown_all）追加：session flush（含 GC，§3.3）→ `PRAGMA wal_checkpoint(TRUNCATE)`

### 3.2 session 域 = 不透明 JSON 文档存储

`WorkspaceSessionState`（`src/shared/workspace-session-state-types.ts:36-135`）是 30+ 字段的 fork 巨型嵌套类型。Rust 侧**不做类型镜像**：

- 命令载荷用 `String`（JSON 文本）；TS 侧 stringify/parse 后按既有类型使用
- 存储粒度：**顶层字段一行**；`patch`（`WorkspaceSessionPatch = Partial<WorkspaceSessionState>`，workspace-session-state-types.ts:136）语义 = 逐顶层键整键替换；`set` = 整状态替换（删除库中存在而 payload 缺失的键 + 覆盖 payload 内各键）；`get` = 全行组装为 JSON 对象返回
- 写者唯一（渲染层 session real 实现），Rust 不校验内容 schema；宽松容忍未知键（与 ade-store 既有「宽松归一化」哲学一致）

### 3.3 scrollback 快照文件与 GC

- 采集产物 = `@xterm/addon-serialize` 序列化文本（契约 `readTerminalScrollback({ref}) => string | null`，`src/shared/preload-api/api/workspace-session-api.ts`；orca: `persistence-workspace-session-scrollback.test.ts` 同为文本）
- 存储：`app_data_dir/terminal-scrollback/<ref>.txt`；ref = 内容寻址 `v1-<sha256(content) 前 32 hex>`（orca: `terminal-scrollback-snapshots.ts` `makeTerminalScrollbackSnapshotRef` 同形）；写前读判存（同内容复用同 ref，零重复文件）
- ref 校验：读侧白名单正则 `^v1-[0-9a-f]{32}$`（orca 同形），不过即 null
- 字节上限：单快照超 **2 MiB** 拒绝写入返回错误（serialize 产物通常数百 KiB；上限防失控；orca 的 5 MiB store 上限针对原始流，serialize 终态更小，取 2 MiB）
- GC：`session_flush(activeRefs?: string[])` 删除 `terminal-scrollback/` 下不在 activeRefs 集合内的文件；渲染层 flush 时由当前 layouts 算活性集合传入；宿主不理解 layouts 内容，零耦合

### 3.4 transcript 扫描（providerSession 捕获）

命令：`agent_sessions_resolve_capture(args: {cwd: string, agentKind: string, windowFromMs: number, windowToMs: number}) -> AgentProviderSessionMetadata | null`

- 返回类型复用 fork 契约 `src/shared/agent-session-resume.ts:26-36` `AgentProviderSessionMetadata { key: 'session_id' | 'conversation_id', id, transcriptPath? }`（小类型，specta 镜像）
- **claude**：目录 `~/.claude/projects/<munged-cwd>/`（`/Users/a/b` → `-Users-a-b`，Claude Code 目录名规则）；列 `*.jsonl`，mtime ∈ [windowFromMs, windowToMs]，取最新；`key='session_id'`，id = 文件名 stem（UUID），transcriptPath 随行
- **codex**：目录 `~/.codex/sessions/`（`YYYY/MM/DD/rollout-*.jsonl` 递归）；读各文件头部（首 4KiB）匹配 cwd 字符串，mtime 在窗内取最新；`key='conversation_id'`，id 从文件名 rollout-`<uuid>` 段提取，transcriptPath 随行
- 无命中/目录不存在/无 home/`agentKind` 非 `claude|codex` → 返回 null（不报错）；扫描超时上限（单目录 500ms，防挂载卷卡顿）
- 调用时机：pane 休眠/tab 关闭时由渲染层触发，窗口 = [pane spawn 时刻, now]；返回值写入 `sleepingAgentSessionsByPaneKey[paneKey]` 随 session.patch 落库

### 3.5 resume 重生路径（直接 argv spawn，宿主零改动）

- 恢复 tab 时对每个带 sleeping 记录的 paneKey：**以 agent CLI argv 直接 spawn**（`claude --resume <id>` / `codex resume <id>`，命令行由渲染层从记录构建），复用 1C 已生效的 spawn `command` 面（1C 规格 §2.4：command 生效）；shell 参数（cwd/cols/rows/env）与普通 spawn 同路
- **偏差备案（对 fork）**：fork 的 resume 机械是「向活 shell 键入 shell-quoted 命令行」（`src/renderer/src/lib/agent-resume-launch-target.ts:41-45`，依赖 shell-ready 时序与 shell 集成投递——ade 两者均未实现，1C §10 偏差 2/§2.5）。ade 2A 改走直接 argv spawn：无 shell-ready 竞态、无引号问题（#12320 的 quoting 顾虑不适用于 argv）
- spawn 前可选调 `agent_sessions_resolve_capture` 二次校验 transcript 仍在（防记录过期）；失败则降级为普通 shell spawn，记录保留
- resume spawn 失败（CLI 缺失等）走现有 spawn 失败错误态；**注册表记录保留**，下次恢复可重试

## 4. 数据流

### 4.1 采集（上行）

```
xterm 缓冲变化 → 防抖（空闲 5s / 最长 30s）→ serializeAddon 序列化（绝对光标包装）
  → session_write_terminal_scrollback → {ref}（内容寻址落盘）
  → ref 写入渲染层 layout state（scrollbackRefsByLeafId）
  → session.patch（顶层键替换，含 layouts/tabs/sleepingAgents 等）
  → SQLite workspace_sessions
```

强制采集点（跳过防抖立即执行）：pane 休眠、tab 关闭、`visibilitychange → hidden`、flush 前。

### 4.2 恢复（下行）

```
启动/reload → session.get → WorkspaceSessionState
  → tabs/split 布局/活跃 tab 还原（use-app-startup-hydration 既有消费点，mock 空态转真态）
  → 每 leaf：readTerminalScrollback(ref) → null 走空分支 / 文本回放进 xterm（fork restore 机械）
  → 每 paneKey 查 sleepingAgentSessionsByPaneKey：
      有记录 → 直接 argv spawn（--resume），否则普通 shell（同 cwd）
  → 捕获命令补充新记录（pane 生命周期钩子）
```

## 5. 渲染层接线

### 5.1 session 域 real 实现（`src/bridge/real/session.ts`，新）

- `get/set/patch` → `session_get/session_set/session_patch`（JSON 文本信封）；`get` 解析后按 `WorkspaceSessionState` 使用；错误上抛由调用方 catch（启动水合已有容错）
- `flush` → `session_flush`（活性 refs 由当前 layouts 计算）
- `readTerminalScrollback({ref})` → `session_read_terminal_scrollback`，null 直通
- `writeTerminalScrollback({tabId, leafId, content}) → {ref}` → 新契约方法（`workspace-session-api.ts` 增补；mock 同步加桩；宿主 §3.3）
- `setSync` → fire-and-forget patch（无 ack，与 `pty.write` 吞错先例同形）
- 接入：`create-api.ts` RealDomains 增 `session`；parity 键集门禁同步（`session` 子域 `satisfies`）

### 5.2 快照采集 hook

- 复用 per-pane `serializeAddon` 与 `src/shared/terminal-serialize-absolute-cursor.ts` 包装（fork 已挂载：`lib/pane-manager/pane-dom-creation.ts:132`、注册点 `components/terminal-pane/pty-connection/pane-serializer-register.ts`）
- 新增防抖调度：缓冲写入活动后空闲 5s 触发；持续写入时最长 30s 强制；强制点（§4.1）跳过等待
- 序列化排除 alt-screen（fork 注册点已有语义）；已知 addon 缺口沿用（OSC 标题不回放——标题由 layouts 的 `titlesByLeafId` 单独持久化恢复，不受影响）
- ref 写 `scrollbackRefsByLeafId`；`buffersByLeafId`（内联）不使用，留空

### 5.3 恢复接线（多数为既有机械接真）

- `use-app-startup-hydration`/`use-app-session-persistence`：session.get/patch 从 mock 空态转真态（调用点已存在：`app-shell/use-app-session-persistence.ts:108`、`use-app-startup-hydration.ts:147`）
- 布局恢复 + scrollback 回放：`components/terminal-pane/terminal-pane-layout-restore.ts` 与 `readTerminalScrollback` 消费点（`components/terminal-pane/terminal-pane-lifecycle-primitives.ts:105`，mock 恒 null）接真
- 自动 resume：恢复路径查注册表 → §3.5 直接 argv spawn；捕获时机接线（§3.4）
- mock 侧同步：`workspace-session-api.ts` mock 工厂增 `writeTerminalScrollback` 桩，`readTerminalScrollback` 维持 null

## 6. 错误处理与边界

| 场景 | 行为 |
|---|---|
| SQLite 打不开（损坏/磁盘满） | session 命令返回 Err；渲染层 catch 降级为「无持久化」运行（同 mock 空态），应用不崩；DB 损坏时 open 尝试改名 `ade.sqlite.corrupt-<ts>` 后重建（日志留痕） |
| 快照 ref 悬空（文件被删/损坏头） | read 返回 null，渲染层走既有空分支 |
| 快照超 2 MiB | write 返回错误；渲染层吞错并放弃该 leaf 本次 ref 更新（保留旧 ref） |
| transcript 扫描无命中 | capture 返回 null，无记录，恢复为普通 shell |
| resume spawn 失败 | 现有 spawn 失败错误态；记录保留 |
| 退出竞态 | 防抖定期写保证快照最多损失最后 ≤30s；`RunEvent::Exit` 同步 flush + checkpoint。**已知边界**：退出瞬间渲染层来不及上行的最后一批序列化产物会丢（orca: `quit-path-durable-write-blocking` 用阻塞写解决；Tauri 下 Exit 时渲染层不可达，接受并留档） |
| 快照文件写入失败（磁盘满） | write 返回错误，同超限处理；session.patch 不含新 ref |
| 多窗口并发 | 2A 单窗口（现状），session 域单写者 |

## 7. 测试与门禁

### 7.1 Rust

- `ade-store`：迁移框架（v1 建表 + 追加假想 v2 的升级演练）、CRUD、`updated_at` 维护、损坏库文件降级改名重建、WAL checkpoint
- `ade-bridge` session 命令：patch/get round-trip、顶层键整键替换语义、未知键容忍、flush GC（孤儿删/活性留/ref 正则拒收）、写读去重（同内容同 ref）、2 MiB 上限
- `agent_sessions`：fixture 目录测试（munged cwd 正/误例、时窗过滤、多文件取最新、codex 三级目录递归、头部 cwd 匹配、null 分支、超时）
- bindings 新鲜度 + `export_lists_every_command` 清单更新 + 命令名锁定

### 7.2 TS（vitest）

- `real/session.ts`：信封、错误形态、null 直通、JSON round-trip、setSync 吞错
- parity 门禁：session 子域键集（含新增 writeTerminalScrollback，mock/real 双侧）
- 采集 hook：防抖节奏（5s idle/30s max）、强制点立即触发、失败吞错保留旧 ref
- 恢复：session.get → tabs/layouts 还原 slice 测试；sleeping 记录 → resume spawn 参数断言（argv 正确性：claude/codex/未知 agent 降级）；ref 悬空回退
- mock/real 一致性

### 7.3 手工验收

见 §1 验收清单（对话→重启恢复、claude/codex 自动 resume 续聊、split 恢复、reload 恢复、孤儿 GC、损坏库降级）。

## 8. 风险与偏差备案

1. **退出瞬间的最后一批快照丢失**（§6）：Tauri Exit 时渲染层不可达；缓解 = 防抖 5s/30s；若手工验收不可接受，备选 = `ExitRequested` 先 emit 事件给渲染层、限时 500ms 等待 flush 再放行（计划阶段视验收结果决定是否实现）。
2. **serialize 产物与 xterm 版本耦合**：xterm 系已 pnpm patch 钉版本（`pnpm-workspace.yaml:16-24`），升级需重验恢复 parity（既有 headless 夹具覆盖）。
3. **codex transcript 目录结构稳定性**：`~/.codex/sessions` 布局无官方契约，扫描实现按当前观测布局 + 宽松匹配（头部 cwd 包含即可）；失败降级 null，不阻塞恢复。
4. **session state 顶层键整键替换的写放大**：layouts dict 随 patch 全量重写（数百 KiB 级、防抖节流后低频）；SQLite 单行 TEXT 写入可承受；若实测成为瓶颈，计划阶段可拆 per-tabId 行（schema 兼容，键名约定变化）。
5. **与 fork resume 机械的偏差**（§3.5）：直接 argv spawn 替代键入行；fork 侧 hibernation 机械中依赖键入路径的部分不迁移，仅复用其注册表数据形状。
