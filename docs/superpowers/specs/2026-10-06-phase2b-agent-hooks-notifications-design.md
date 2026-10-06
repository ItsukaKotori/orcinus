# Phase 2 子项目 B：agent hook server + 状态接真 + 完成通知 设计规格

- 日期：2026-10-06
- 状态：brainstorming 输出（范围 = 状态+通知一体、归一化 = TS 复用、安装策略 = orca 语义沿用、claude-only——用户既有指令，均已批准；本规格为实现依据）
- 基线：`main@173248c`（Phase 2A 已合入）
- 行为 oracle：`/Users/itsuka/CodeSpace/orca`（Electron 版，只读参照；`orca:` 前缀均为其内路径）
- 上游规格：`docs/superpowers/specs/2026-09-14-ade-design.md`（§2.1 A 项、§8 Phase 2）、2A 规格（transcript 扫描兜底与 quit 记录语义沿用）
- 前置事实：hook 参照调查（2026-10-06 本会话探索代理）：orca hook server = `orca:src/main/agent-hooks/`（127.0.0.1 随机端口 + token + 18 源路由 + PTY env 注入 + endpoint 文件 + settings 托管条目写入器 + last-status.json 回放）；ade renderer 消费端完整（agent-status-ipc-bridge + 55 slices + 完成通知协调器 + attention/未读面），宿主侧空白；`agent-hook-api` 的 HooksApi 契约是 orca.yaml 工作区 hooks 的另一套东西，与本子系统无关、不动

## 1. 背景与目标

2A 交付了 resume 链路，但 agent 状态识别只有 transcript 扫描兜底（休眠记录捕获），运行时状态（working/waiting/done）与完成通知全空。2B 接通 claude hook 回调链路，点亮 agent 状态面与通知/未读：

- **hook server**（Rust 新 crate `ade-hooks`）：接收 claude hook 回调，归因到 pane，转发 renderer
- **状态接真**：`agentStatus:set/clear/getSnapshot` 三通道，点亮 55 个既有 store slices 与 tab/侧栏状态面
- **完成通知/未读**：`notifications` 宿主域接真，点亮既有完成通知协调器与未读/徽标面

**验收（自动 + 手工）**：claude 对话 → 状态实时变化（working→waiting→done）→ waiting 时桌面通知 + 未读徽标 → 点进 pane 自动已读 → Settings 关 hook 开关 → 对话不再驱动状态 → 重启后 spool/缓存正确。自动化门禁：`cargo test --workspace` 全绿 + `pnpm test` 全绿 + `pnpm typecheck && pnpm build:web` exit 0。

## 2. 范围

### 2.1 实现面

| 面 | 载体 | 说明 |
|---|---|---|
| hook HTTP server | `ade-hooks`（新 crate） | 127.0.0.1 随机端口 + token、POST only、1MB 限长 + slowloris 超时、`/hook/claude` 路由、meta 头合并（§3.2） |
| claude hook 安装器 | `ade-hooks` | `~/.claude/settings.json` 托管条目写入器（§3.3）+ 共享脚本 + spool 兜底（§3.4） |
| endpoint 发布 | `ade-hooks` | `app_data/agent-hooks/endpoint.env`（0600 原子写）+ PTY env 注入（§3.5） |
| 状态缓存 | `ade-hooks` | `last-status.json`（250ms 防抖）+ 重启回放（§3.6） |
| 状态桥 | `src/bridge/real/agent-status.ts`（新） | raw 事件 → TS 归一化 → `AgentStatusIpcPayload` → 既有 ipc-bridge 消费者（§3.7） |
| 通知域 | `src/bridge/real/notifications.ts`（新）+ `tauri-plugin-notification` | dispatch/dismiss/getPermissionStatus/probeDelivery/playSound（§3.8） |
| 设置开关 | `ade-store` settings 扩展 | `agentStatusHooksEnabled`（默认 true；关闭 = skip 不删） |

### 2.2 明确不做（防蔓延）

codex hooks.json 与其余 17 agent 源（claude-only 指令，2B 后按需扩展）；SSH transient clear；mobile/web 的 drop 系通道（drop/dropPersisted/dropByTabPrefix/retire/restore/transferPaneAuthority 留 fallback noop——单机无此清理面）；statusline 用量路径；Notification/PreCompact 事件安装（orca 注释：避免误报/误报 working）；通知设置页扩展（沿用现有设置面）；Windows hook 脚本分支（先 POSIX；Windows cmd 包装收尾或顺延）；归一化移植 Rust（TS 复用裁定）。

## 3. 架构

### 3.1 crate 与启动

- 新 workspace 成员 `crates/ade-hooks`：HTTP server（`std::net::TcpListener` 绑 `127.0.0.1:0` + 线程池或 `tiny_http`）、claude settings 安装器、endpoint 发布、状态缓存。依赖最小（不自起 tokio——ade-pty 先例为 std + 线程）
- 启动序（`src-tauri/src/lib.rs` setup）：`PtyHost::start` 后 `AgentHookServer::start(app_data, callback)`：绑定 → token 生成（`randomUUID` 同源 `ade-core::ids`）→ endpoint 发布 → settings reconcile → 缓存加载；端口换绑重试 ≤3 次，仍失败 → 服务降级停用（日志留痕，状态回退 2A 兜底）
- ade-bridge：`agent_status_get_snapshot` 命令 + `agent-hook:raw` 事件转发 + spawn 面传 endpoint env

### 3.2 HTTP 接收与归因

- `POST /hook/claude`；头 `x-orca-agent-hook-token` 必须等于启动 token（否则 403）
- 载荷两形态（对齐 orca `hook-post-command.ts`）：raw JSON（默认，body = hook stdin 原文）+ form 回退（`payload` 字段 = stdin 原文）；归因元数据在 `X-Orca-Agent-Hook-Meta`（base64，`\x1f` 分隔 paneKey/tabId/launchToken/worktreeId/env/version）
- 归因：paneKey 来自 PTY env `ORCA_PANE_KEY`（spawn 时注入，随 hook 进程继承）；空 paneKey 计数丢弃
- 转发：`agent-hook:raw` Tauri 事件 `{source:'claude', payload, paneKey, tabId, worktreeId, launchToken, receivedAt}`——**归一化在 renderer**（TS 复用 `shared/agent-hook-listener.ts` 纯函数；Rust 不解析 hook 语义）

### 3.3 claude settings 托管条目写入器

- 事件集（orca `hook-settings.ts` 全集）：SessionStart、UserPromptSubmit、PreToolUse/PostToolUse/PostToolUseFailure（matcher `*`）、PermissionRequest（matcher `*`）、Stop、StopFailure、SubagentStart、SubagentStop、TeammateIdle、PostCompact；**不装 Notification/PreCompact**
- 命令行：每事件一条 `hooks:[{type:'command', command: <wrapper>, timeout: 10}]`，wrapper 指向 `~/.ade/agent-hooks/claude-hook.sh`（按脚本文件名识别托管条目）
- 写入策略：只增删托管条目（脚本名匹配），用户已有 hook 不动；写前 rolling backup；symlink 解引用（防 dotfiles 管理器断链）
- 开关 `agentStatusHooksEnabled`（settings，默认 true）：启动 reconcile 时关闭 = skip **不删**（user-global 文件，防多 profile 互删）；开启 = 安装/更新
- claude CLI 不在 PATH：skip（`cli_not_found` 语义），server 照常运行

### 3.4 共享脚本与 spool 兜底

- `~/.ade/agent-hooks/claude-hook.sh`（prod/dev 共用）：`printf '{}\n'`（PermissionRequest 需非空 stdout）+ stdin 捕获 + source endpoint 文件 + curl POST（raw JSON 优先，失败落 spool 目录）
- 启动时 drain spool 重放；重放失败保留文件下轮再试

### 3.5 PTY env 注入

ade-pty spawn env 面追加：`ORCA_AGENT_HOOK_PORT/TOKEN/ENV/VERSION/ENDPOINT` + `ORCA_PANE_KEY`（= `${tabId}:${leafId}`，2A 时被忽略的 spawn args `tabId`/`leafId` 接真）+ `ORCA_TAB_ID` + `ORCA_WORKTREE_ID`。值来源：renderer spawn args（既有透传面）+ endpoint 发布物。

### 3.6 状态缓存与回放

- `app_data/agent-hooks/last-status.json`：每 hook 事件后 250ms 防抖写（paneKey → 原始 payload 快照 + receivedAt）
- 重启：加载缓存供 `agent_status_get_snapshot` 回放；归一化同样在 renderer（回放走与实时相同的 raw 通路语义）

### 3.7 renderer 状态桥

- `src/bridge/real/agent-status.ts`（新域，接真 `agentStatus`）：
  - `onSet(cb)`：订阅 `agent-hook:raw` → `normalizeHookPayload`（`shared/agent-hook-listener.ts` 复用）→ 组装 `AgentStatusIpcPayload` → cb；归一化失败丢弃计数
  - `onClear(cb)`：ade 宿主不产生 clear 事件（done 语义在归一化产物内，pane 清除由 store 机械处理）——通道保留、实现为立即返回退订的 noop 订阅
  - `getSnapshot()`：`agent_status_get_snapshot` 命令 → 归一化 → 返回
  - 其余契约方法（drop 系、migration）维持 fallback noop（2.2）
- 接入 `create-api.ts` RealDomains + parity 门禁

### 3.8 notifications 域

- `tauri-plugin-notification`（workspace 新依赖）+ `src/bridge/real/notifications.ts`：
  - `dispatch(args)` → 插件通知（标题/正文/ID 对齐 fork `buildAgentNotificationId` 稳定 ID 语义）
  - `dismiss(id)`、`getPermissionStatus()`（插件权限态映射 fork 枚举）、`probeDelivery()`、`playSound()`（对齐 orca main 实现，细节计划阶段核实）
- 接入 RealDomains + parity

## 4. 错误处理与边界

| 场景 | 行为 |
|---|---|
| 端口绑定失败 | 换绑重试 ≤3，仍失败 → 服务降级停用（2A 兜底语义接管），日志留痕 |
| token 校验失败 / 非 POST | 403 计数 |
| body 超限 / slowloris | 413 / 超时断连 |
| settings 写失败 | 安装状态 error + 日志；server 照常运行 |
| 归一化失败 / 未知事件 | renderer 丢弃计数，不阻塞后续 |
| claude CLI 不存在 | 安装 skip，server 照常 |
| spool 重放失败 | 保留文件下轮再试 |
| 迟到 hook（pane 已关） | renderer pending 队列 15s TTL 丢弃（既有机械） |
| 开关关闭时的残留 hook | skip 不删（user-global，防多 profile 互删）；hook server 转发照常——开关只管本 app 是否安装/更新 hook，杀转发会静默丢其他 profile 的事件 |

## 5. 测试与门禁

### 5.1 Rust

- 安装器：fixture settings.json（保留用户条目 / 托管条目增删 / rolling backup / symlink 解引用 / CLI 不存在 skip / 开关关闭 skip）
- HTTP server：token 403、1MB 限长 413、meta 头合并、form 回退、/hook/claude 路由
- endpoint 发布：0600 原子写、内容
- 缓存：防抖写、重启回放
- env 注入：spawn env 全套 ORCA_* 变量
- bindings 新鲜度 + 命令清单更新（`agent_status_get_snapshot`）

### 5.2 TS

- `real/agent-status.ts`：raw fixture（working/waiting/done/PermissionRequest）→ 归一化 → payload 组装；归一化失败丢弃
- `real/notifications.ts`：dispatch/dismiss 信封
- parity 门禁：agentStatus + notifications 两域键集（含 fallback noop 集合）
- mock 同步：agent-status mock 保持（mock 模式仍空态）

### 5.3 手工验收

见 §1 验收链：状态实时变化 → waiting 通知 + 未读徽标 → 自动已读 → 开关关闭停驱 → 重启 spool/缓存正确。

## 6. 风险与偏差备案

1. **timer 节流教训的镜像面**：hook 回调是 curl 进程（非页面 timer），不受 WKWebView 节流影响——2A 的 WriteBuffer 教训不适用于此链路。
2. **orca 语义移植的保真度**：托管条目判定/备份/symlink 解引用按 orca 实现逐字对齐，测试 fixture 覆盖；orca 侧后续演进不在同步承诺内。
3. **claude hook stdin 字段集**（`hook_event_name`/`tool_name`/`agent_id`/…）以 orca `providers/claude-events.ts` 现读取字段为准；claude 版本演进可能增字段——未知字段天然忽略（TS 归一化宽松）。
4. **`ORCA_PANE_KEY` 归因的信任边界**：endpoint/token 仅回环 + 随机 token，伪造需本机权限——与 orca 同等安全水位。
5. **通知插件平台差异**（macOS 权限对话框时序）：`getPermissionStatus/probeDelivery` 的枚举映射以 fork 契约为准，计划阶段核实 orca main 对应实现后钉死。
