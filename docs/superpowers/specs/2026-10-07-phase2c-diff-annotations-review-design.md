# Phase 2 子项目 C：diff 注释与评审（本地注释全链路）设计规格

- 日期：2026-10-07
- 状态：brainstorming 输出（范围 = A 本地注释全链路、方案 = TS 本地终端 RPC 适配器 + Rust 投影修复、发送后语义 = 桌面参照版删除——均已批准；本规格为实现依据）
- 基线：`main@5ec74c1`
- 行为 oracle：`/Users/itsuka/CodeSpace/orca`（Electron 版，只读参照；`orca:` 前缀均为其内路径）
- 上游规格：`docs/superpowers/specs/2026-09-14-ade-design.md`（§8 Phase 2「diff 注释与评审」）
- 前置事实（2026-10-07 本会话双探索代理）：
  - fork 的注释面几乎全量存在且大多已接线：DiffViewer/combined diff 行内注释、markdown 源码/富文本/预览三种注释面、侧栏 Notes 架、复制/清除/定位、发送菜单与 prompt 格式化（`formatDiffComment`）、store 乐观更新 + 串行持久化队列 + 回滚。
  - 断点 1：git worktree 注释写盘后**回读缺失**——`WorktreeMetaStore` 白名单含 `diffComments`（`ade-store/src/worktree_meta_store.rs:52`）并落 `worktrees.json`，但 `apply_worktree_meta`（`ade-bridge/src/commands/worktrees.rs:70-117`）不投影、`ade-core::Worktree` 无字段 → 列表刷新/重启后渲染层行被替换、注释消失；folder workspace 无此问题（`folder_workspaces_update` 原样回读）。
  - 断点 2：发给运行中 agent 的本地链路走 `callRuntimeRpc` → `window.api.runtime.call`（`runtime-rpc-client.ts:82-84`），runtime 域仍是 Phase-0 mock（`create-api.ts:129`）→ `terminal.list/agentStatus/isRunningAgent/wait/send` 全 reject。
  - 新 agent 路径预期已可用：`launchAgentInNewTab` → `pasteDraftWhenAgentReady` 走本地 `pty.onData` 就绪检测 + `pty.write` 直连（`agent-draft-readiness.ts`、`runtime-terminal-inspection.ts:250-253,288-291`）。
  - 宿主契约形状已核实：`RuntimeTerminalSummary/AgentStatus/Wait/Send`（`shared/runtime-terminal-contracts.ts`）；agent hook 状态 `AgentStatusState = working|blocked|waiting|done`，paneKey = `${tabId}:${leafId}`（`shared/agent-status-types.ts:24,107`）；`PtyListedSession`（`shared/pty-listed-session.ts:18`）。

## 1. 背景与目标

2B/2B.1 交付了 agent 状态与通知链路。diff 注释与评审（本地注释，非 PR 评论）在 fork 里 UI/数据面已基本完整，但存在两个功能性断点：worktree 注释刷新/重启后丢失；发送给运行中 agent 的本地链路全断。本子项目以最小改动补齐这两处，使「注释 → 持久化 → 发送给 agent」闭环可用：

- **持久化修复**：`Worktree` 投影回读 `diffComments`，注释跨刷新/重启存活
- **本地终端 RPC 适配器**：在 `callRuntimeRpc` 的 local 分支实现 `terminal.list/agentStatus/isRunningAgent/wait/send`，渲染层发送栈零改动
- **新 agent 发送**：验证既有直连路径（预期已可用）
- **发送后语义**：沿用桌面参照版——投递成功后删除已投递注释

**验收（自动 + 手工）**：diff 行注释 → 重启后仍在；未发送注释 → 发给运行中 claude → 文本出现在输入框并提交；发给新 agent；markdown 注释同管道；folder workspace 回归；复制/清除/定位回归。自动化门禁：`cargo test --workspace` 全绿 + `pnpm test` 全绿 + `pnpm typecheck && pnpm build:web` exit 0。

## 2. 范围

### 2.1 实现面

| 面 | 载体 | 说明 |
|---|---|---|
| worktree 投影 | `ade-core::Worktree` + `apply_worktree_meta` | 新增 `diffComments` 投影（§3.1） |
| bindings | Specta 重生成 | `bindings_are_fresh` 门禁 |
| 本地终端 RPC | `src/renderer/src/runtime/local-terminal-rpc.ts`（新）+ `runtime-rpc-client.ts` 接入 | 5 方法（§3.2） |
| 发送栈 | 复用，零改动 | orca 发送栈原样运行在 local target 上 |
| 新 agent | 验证为主 | 单测/手工；发现问题才修（§3.3） |
| 持久化回归 | 渲染层 reconciliation 测试 | 字段回归后保留（§3.1） |

### 2.2 明确不做（防蔓延）

sentAt「已发送」保留/徽标与已解决状态模型；PR 评论/hosted review/gh 域；远端 runtime 的 `terminal.*`（无 host 实现）；diff 面键盘加注释（参照版亦无）；文件级注释（lineNumber 0）创建入口；`window.api.shell.openUrl`（PR 评论链接用，潜伏）；浏览器注释面；`mobileDiffReview` 投影（移动端不在本应用）；Rust 端 DiffComment schema 校验（保持 opaque，渲染层已有 `normalizeDiffComment`）。

## 3. 架构

### 3.1 持久化修复

- `ade-core::Worktree`（`worktree.rs:11-32`）增：`#[serde(skip_serializing_if = "Option::is_none")] pub diff_comments: Option<serde_json::Value>`（`rename_all = "camelCase"` 已在该结构上）。opaque Value 与存储层/folder 路径一致，避免在 Rust 重复 TS schema；specta 映射 `Json`。
- `apply_worktree_meta`：`meta.get("diffComments")` 为数组时写入投影（非数组忽略，不报错）。
- `WorktreeMetaStore` 白名单已含该键（`worktree_meta_store.rs:52`），无需改动；`worktrees_update_meta` 返回值随之带上。
- bindings 重生成；`bindings_are_fresh` 保持绿。
- 渲染层：`worktree-catalog-reconciliation` 行合并因字段回归自然保留（补测试锁定）；`fetchWorktrees`/`updateMeta` 往返测试。
- folder workspace：仅补回归测试（现状已通）。

### 3.2 本地终端 RPC 适配器

- 新模块 `local-terminal-rpc.ts`；`callRuntimeRpc` 的 local 分支改调 `callLocalRuntimeRpc({method, params})`；未覆盖方法回退 `window.api.runtime.call`（现状语义不变）。直接 `window.api.runtime.call` 的本地调用方（如 `worktree-live-terminal-surface-owners.ts`）不在此接缝内，维持现状。
- 方法表：
  - `terminal.list`：`pty.listSessions`（真实命令）为活性来源；store `ptyIdsByTabId` + `terminalLayoutsByTabId.ptyIdsByLeafId` 补 tabId/leafId；worktree 过滤 + limit；handle = ptyId；`worktreePath`/`branch` 取 store worktree 行；`preview` 空串、`lastOutputAt` null、`visualLayouts` 省略；`totalCount`/`truncated` 如实。发送路径只消费 `tabId`/`leafId`/`handle`。
  - `terminal.agentStatus`：ptyId → paneKey（`${tabId}:${leafId}`）→ `agentStatusByPaneKey`（沿用 freshness 规则）→ 状态映射（§4.1）；`isRunningAgent` = 新鲜 hook 条目 ∨ 标题证据（`isRecognizedAgentTitle`/`classifyTitleActivity`）。
  - `terminal.isRunningAgent`：同上（legacy 兜底路径）。
  - `terminal.wait`（tui-idle）：轮询（~250ms）状态映射；`idle` → `satisfied:true`；`working` → 等到 timeout（`satisfied:false`，不抛）；`permission` → `blockedReason:'agent-approval-prompt'`；pty 消失 → `status:'exited'`；无 hook → 1500ms 输出静默窗（复用 `subscribeToPtyData` sidecar）+ 标题 idle。
  - `terminal.send`：`requireAgentStatus` 先查映射，不满足 → `{accepted:false, refusedReason:'no-agent'|'permission'}`；文本经 `pty.writeAccepted` 直写；粘贴标记/延迟 Enter 由调用方既有逻辑负责。
- 错误码（`RuntimeRpcCallError`，对齐渲染层已识别集合）：handle 未知 → `terminal_handle_stale`；会话存在但退出 → `terminal_exited`；列表无此终端 → `terminal_gone`；worktree 无终端 → `no_active_terminal`。

### 3.3 新 agent 路径

验证 `ReviewNotesSendMenuContent` 的「新 agent」项：`launchAgentInNewTab(promptDelivery:'submit-after-ready')` → `pasteDraftWhenAgentReady`（本地 `pty.onData` + `pty.write`）。单测 + 手工验收；仅在验证失败时修复（修复范围另行报批）。

### 3.4 发送后语义

沿用桌面参照版：投递成功 → `clearDeliveredDiffComments` 删除已投递快照（持久化队列同步删除）。`markDiffCommentsSent`/`sentAt` 徽标不在本阶段（§2.2）。

## 4. 状态映射与数据流

### 4.1 映射表

| `AgentStatusState`（hook） | `RuntimeTerminalAgentStatusState` | 说明 |
|---|---|---|
| `working` | `working` | wait 轮询等待 |
| `blocked` | `permission` | wait.blockedReason / send refusedReason |
| `waiting` | `idle` | 可输入 |
| `done` | `idle` | 可输入 |
| 无条目/不新鲜 | `null` | 标题证据 + 输出静默窗兜底 |

### 4.2 数据流

- 发送：菜单过滤未发送 → 目标枚举（store）→ `sendNotesToActiveAgentSession` → `callRuntimeRpc(local)` → 适配器 → `pty.writeAccepted` → 成功 → `clearDeliveredDiffComments` → 持久化队列删除。
- 持久化：`addDiffComment` → 乐观 store → 串行队列 → `worktrees_update_meta`（修复后回读）→ 刷新/重启 → reconciliation 保留。

## 5. 错误处理与边界

| 场景 | 行为 |
|---|---|
| handle 未知 / pty 已更换 | `terminal_handle_stale` |
| 会话存在但进程退出 | `terminal_exited` |
| 列表无此终端 | `terminal_gone` |
| worktree 无终端 | `no_active_terminal` |
| wait 超时 | 返回 `satisfied:false`（不抛） |
| send 时状态丢失 | `refusedReason`（`no-agent`/`permission`） |
| 非 hook agent（codex 等） | 标题/静默窗兜底；无 bracketed 握手时走 legacy 组合发送 |
| 持久化失败 | 沿用现有回滚（`diff-comment-persistence.ts`） |
| 方法未覆盖 | 回退 `window.api.runtime.call`（现状不变） |

## 6. 测试与门禁

### 6.1 Rust

- `worktrees_update_meta` 带 `diffComments` → `worktrees_list` 回读；store reload（重启）持久；非数组忽略。
- folder workspace `diffComments` 回归。
- bindings 新鲜度。

### 6.2 TS

- `local-terminal-rpc`：5 方法 happy path + 错误码 + 映射表 + 无 hook 兜底 + wait 超时/blocked/exited。
- 发送栈 local target 集成（mock pty）：sendable/permission/no-agent/refused 路径。
- reconciliation/merge 保留 `diffComments`。
- 既有测试全绿（`diffComments.test.ts`、`folder-workspace-diff-comments.test.ts`、发送栈系列）。

### 6.3 手工验收

diff 注释 → 重启仍在；发给运行中 claude（出现在输入框并提交）；发给新 agent；markdown 注释；folder workspace；复制/清除/定位回归。

### 6.4 门禁

`cargo test --workspace`、`pnpm typecheck`、`pnpm build:web`、`pnpm test`（已知 performance-budget 抖动单独复跑）。

## 7. 风险与偏差备案

1. **wait/liveness 近似**：宿主级输出静默/进程检查（`inspectProcess` 为 Phase 1C 故意 stub）不可用，tui-idle 判定依赖 hook 状态 + 标题 + 渲染层静默窗；语义偏差在收尾记录中披露。
2. **handle 生命周期**：handle = ptyId；pty 重建后旧 handle 报 `terminal_handle_stale`（对齐宿主 stale 语义）。
3. **标题证据误报**：非 hook agent 的 `isRunningAgent` 可能偏保守/偏乐观；guarded 发送仅在 bracketed 握手或 hook 状态成立时进行，否则走 legacy 组合发送。
4. **opaque 投影**：`diffComments` 在 Rust 为 `Value`，TS 共享类型仍是唯一 schema；specta 仅做新鲜度校验。
5. **`mobileDiffReview` 仍不投影**（与 2B 现状一致）。
