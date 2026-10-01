# Phase 1 子项目 C：终端/PTY + agent（最小纵切）设计规格

- 日期：2026-09-28
- 状态：brainstorming 输出（数据面架构方案 A 与 agent 最小纵切范围已由用户批准；本规格为实现依据）
- 基线：`main@c88ae2c`（Phase 1A 及验收修复已合入；子项目 B `phase1b-worktree-git` 并行进行中，C 与 B 的预期冲突面仅为 `src/bridge/create-api.ts`、`src-tauri/crates/ade-bridge/src/{lib.rs,specta_export.rs,state.rs}` 与 parity 测试清单，合并时按域解冲突）
- 行为 oracle：`/Users/itsuka/CodeSpace/orca`（Electron 版，只读参照；下文 `orca:` 前缀均为其内路径）
- 上游规格：`docs/superpowers/specs/2026-09-14-ade-design.md`（§4 决策 2、§5 crate 表、§8 Phase 1）、`docs/superpowers/specs/2026-09-23-phase1a-open-project-design.md`（§2.3 C 范围）
- 前置事实：`docs/spikes/2026-09-14-pty-throughput.md`（数据通道决策 + 宿主实现要求 1–5）；渲染层 pty 消费面调查（2026-09-28 本会话，关键结论已内嵌为 §2/§6 的依据，文件行号以调查时基线为准）

## 1. 背景与目标

Phase 1 拆 A（打开项目，已完成）、B（worktree + git，并行进行中）、C（终端/PTY + agent）。C 的目标是把终端从「mock 空转」推进到真实可用，并在其上以最小方式跑起 Claude Code/Codex：

**验收（自动 + 手工）**：`pnpm dev` 下 —— 新建终端 tab 跑真实 shell（macOS 登录 zsh / Windows PowerShell）→ 键入交互（回显、光标、颜色、Ctrl-C 正常）→ split 出第二 pane（一 leaf 一 PTY，两会话独立互不串扰）→ 在 worktree 中点选 Claude Code 启动新会话（TUI 正常渲染、可交互对话、能完成一次真实提交）→ Codex 同验 → 关闭 tab（会话回收、`onExit` 触发、无僵尸进程）→ `cat` 大文件（≥16 MiB）不丢字节、不卡死 UI。自动化门禁：`cargo test --workspace` 全绿 + `pnpm test` 全绿（既有套件 + 新增）+ `pnpm typecheck && pnpm build:web` exit 0。

**已知缺口（记录在案，Phase 2 收口）**：app 重启 / webview reload / cold-park 休眠唤醒后终端画面空白（无 scrollback 回放；渲染层 eager buffer 仅覆盖 spawn→attach 窗口）；agent resume 按钮不可用（`coldRestore` 缺省）；delivery-health、隐藏投递门、pane 序列化器机械不实现（WS 数据面无此需要，见 §2.2/§2.3）。

## 2. 范围

### 2.1 C 实现（真实）

| 面 | 方法 | 说明 |
|---|---|---|
| 会话创建 | `spawn` | 控制面命令；返回**最小合法响应 `{id}`**（渲染层 `ipc-pty-connect-result.ts` 对其余字段全部容忍缺省，`isReattach` 仅在 `sessionId` 命中活会话时返回 true） |
| 数据面（流） | `write` `writeAccepted` | 上行走 `pty_write`/`pty_write_accepted` 命令（§3.2 修订版）；`writeAccepted` 发出即回 `true` |
| 数据面（流） | `onData` | 流式下行（自定义协议 fetch ReadableStream）直喂渲染层 dispatcher（**不经 Tauri event**），载荷 `{id, data, rawLength}` |
| 几何 | `resize` `getSize` | resize 写 PTY 尺寸（ConPTY reflow 依赖）；getSize 回最近 resize/spawn 值 |
| 生命周期 | `kill` `signal` `clearBuffer` `onExit` `onSpawned` | kill（`keepHistory` 参数忽略）= child kill + 注册表摘除 + `pty:exit {id, code}`；signal 透传（unix `kill -<sig>`，Windows 仅支持有限信号，透传失败返回错误）；clearBuffer 清空宿主侧 pre-attach 缓冲；`pty:spawned {id}` 事件 |
| 会话读取 | `getCwd` `hasPty` `listSessions` | getCwd 回 spawn cwd（不做 OSC7 追踪）；hasPty 查注册表；listSessions 按 `PtyListedSession`（`src/shared/pty-listed-session.ts`，normative）投影注册表活会话；带 `sessionId` 的 spawn 命中活会话时按 reattach 处理（webview reload 后 tab 重挂路径） |
| 进程检查 | `inspectProcess` `getForegroundProcess` `confirmForegroundProcess` `hasChildProcesses` | **web-stub 同形缺省**（见 §2.2）；agent 身份识别属 Phase 2 |
| management | `listSessions` `killAll` `killOne` `restart` `macTccAttribution` | 前三者对注册表真实操作（killAll 逐会话 kill 并聚合 `{killedCount, remainingCount, killedSessionIds}`）；restart 返回 `{success:true}`（无 daemon）；macTccAttribution 恒 `'unknown'` |
| preflight | `refreshAgents` `detectAgents` | 登录 shell PATH 水合 + agent CLI 探测（§7）；detectAgents 复用同命令取 agents（渲染层自动探测路径消费） |

### 2.2 web-stub 同形缺省（权威清单：`src/renderer/src/web/preload-api/web-terminal-api.ts`）

`getForegroundProcess`/`confirmForegroundProcess` → `null`；`hasChildProcesses` → `false`；`getMainBufferSnapshot` → `null`；`getAuthoritativeBufferSnapshotCapabilities` → 逐 id `{id, authoritative:false}`；`inspectProcess` → reject `Error('terminal_liveness_unavailable')`；`reportRendererDeliveryState` → `{inFlightTotalChars:0, inFlightPtyCount:0, msSinceLastAck:null}`（watchdog 保持 idle）；`getRendererDeliveryDebugSnapshot` → web stub 的零对象（逐字照抄）；`getCwd` 见 §2.1；`getPtyDataListenerCount` → 真实返回 pty.ts 内部 emitter 的监听数（比 stub 的 0 更诚实，语义等价）。

### 2.3 noop 订阅 / no-op 方法（沿用 mock 现状，订阅返回 no-op 退订）

`onReplay` `onModelRestoreNeeded` `onSideEffect` `getSideEffectSnapshot` `onDeliveryResyncRequest` `respondDeliveryResync` `onSerializeBufferRequest` `onClearBufferRequest` `sendSerializedBuffer` `onWriteUnavailable` `declarePendingPaneSerializer` `settlePaneSerializer` `clearPendingPaneSerializer` `reportRendererSerializerReady` `setActiveRendererPty` `setHiddenRendererPty` `setRendererPtyVisible` `setPtyDeliveryInterest` `publishTerminalViewAttributes` `ackData` `ackColdRestore` `claimViewport` `reportGeometry` `rendererDispatcherReady` `resetRendererDeliveryDebug`。终端标题由渲染层 outputProcessor 本地抽取 OSC（`pty-output-processor.ts`），无需宿主支持。

### 2.4 spawn 参数处置

- **生效**：`cols` `rows` `cwd` `cwdFallback:'worktree'`（按 `worktreeId` 解析路径，复用 projects 注册表；folder 仓库取 `repo.path`）`env` `envToDelete` `command`（§4.5）`shellOverride` `sessionId`（reattach 判定）`worktreeId`。
- **忽略（记录在案）**：`commandDelivery` `launchConfig` `resumeProviderSession` `launchToken` `launchAgent` `startupCommandDelivery`（`fast`/`shell-ready` 统一为 spawn 即投递，§10 偏差 2）`telemetry` `projectRuntime` `terminalColorQueryReplies` `initiallyHidden` `connectionId` `tabId`/`leafId`（绑定同步，Phase 2）。

### 2.5 明确不做（Phase 2+）

scrollback 快照与重启恢复（`getMainBufferSnapshot`/`coldRestore`/`replay`/`snapshot*` 字段族）、agent 状态识别与 hook server（`onSideEffect` 事实族）、delivery-health 机械（resync/ack/debug 面板）、隐藏投递门、pane 序列化器、daemon（`management` 的 daemon 语义）、resume、OSC7 cwd 追踪、shell 集成注入（orca 的 wrapper/wrapper-fileset 族）、`terminal-preview` 域（渲染层零消费）、`shellReadyArmed`。

## 3. 架构

### 3.1 crate 布局

```
src-tauri/crates/
├── ade-pty/                 # 由 spike orcinus-pty 更名（吞吐 bench/sink bins 保留）
│   └── src/
│       ├── lib.rs           # PtyHost：会话注册表、spawn/resize/write/signal/kill、SessionHandle
│       ├── cpr.rs           # CPR 扫描器（自 spike reply_to_cursor_query 平移：跨块 tail 保留 ≤3 字节）
│       ├── session.rs       # 单会话：portable-pty pair、child、reader 线程、有界队列、pre-attach 环形缓冲
│       ├── server.rs        # 数据面会话路由（§3.2 修订版：进程内流式接管/替换/退出收尾；原 WS accept/鉴权层随偏差 4 移除）
│       ├── supervisor.rs    # reader 线程 join（2s 超时）+ Reaper 有界回收列表
│       └── shell.rs         # 默认 shell 解析（unix $SHELL -l / Windows $COMSPEC、shellOverride）
└── ade-bridge/src/commands/pty.rs   # 控制面命令 + AppState 挂 PtyHost + 事件广播
```

- **`ade-pty` 不依赖 tauri**：WS server 与宿主逻辑自包含；`PtyHost::new(handle: tokio::runtime::Handle)` 注入运行时（ade-bridge 传 `tauri::async_runtime::handle()`，测试传 `#[tokio::test]` 的 handle）——Phase 2 提取 sidecar 时本 crate 原样搬走，WS 帧协议不变（§10 偏差 1 的接口缝）。
- 新增依赖：`tokio`（net/rt/sync/time/macros）、`tokio-tungstenite`、`uuid`（v4）、`rand`、`portable-pty = 0.9`（既有）、`thiserror`、`serde`。
- `orcinus-app` setup 中初始化 `PtyHost` 并 `manage`；app 退出时 `shutdown_all`（逐会话 kill + join + WS server 关闭）。

### 3.2 数据面协议（修订版：自定义协议流式；原 WS 环回方案被 2026-10-01 手工闸门否决，见 §10 偏差 4）

- **传输**：Tauri 自定义 URI scheme（`register_asynchronous_uri_scheme_protocol`），协议名 `orcinus-pty`；URL 形态 `orcinus-pty://localhost/stream/<sessionId>`。处理函数运行在**应用进程内**，不经过系统网络栈——免疫 ATS / 本地网络隐私 / 防火墙（macOS 26 对 Tauri app 网络子进程的环回 WebSocket 静默丢包，实测证据见 §10 偏差 4）。
- **下行（host→client）**：`fetch('orcinus-pty://localhost/stream/<id>')` 的响应体为**无限流**：处理函数先排空会话 pre-attach 缓冲（256 KiB 环形，原 §3.2 语义不变），再持续转发 outbound 通道的字节块；会话退出/摘除时流正常结束（body 终止）。未知 id → 404；流式响应以 body 终止表达「会话数据面关闭」。
- **上行（client→host）**：键盘输入走既有 `pty_write` / `pty_write_accepted` 命令（每次几字节；oracle 的输入本就经主进程 IPC）。`writeAccepted` = 发出即回 `true`。
- **post-attach 不缓冲**：流中断后不重放（进程内直连无网络层断线；webview reload 后由 sessionId reattach 路径重建，画面空白为 §1 已知缺口）。中断即会话死亡判定：fetch 流异常终止且无 `pty:exit` 跟进 → 本地广播 `exit(-1)`（原语义保留）。
- **鉴权/端口/token：全部取消**——自定义协议仅本 webview 可达，外部进程无法触碰；`pty_data_endpoint` 命令与 token 机制整体移除（§5 同步修订）。
- 连接替换与退出收尾语义保持：同 id 重新 fetch = 旧流取消（AbortController）后新建；会话退出时排空 → 流终止 → `pty:exit` 事件（顺序保证：流先终止、事件后发）。

### 3.3 控制面

沿用 1A 模式：每方法一命令、参数 `{args}` 包裹、Rust 结构体 `#[serde(rename_all = "camelCase")]` 与 `src/shared/preload-api/api/pty-api.ts` 逐字对齐、specta 登记、契约测试锁命令名。事件用 Tauri `emit`：`pty:exit`、`pty:spawned`（全局事件，载荷即契约 payload）。

### 3.4 启动引导

无 bootstrap 变更（`pty_data_endpoint` 按需调用 + 渲染层缓存）。`VITE_ADE_BRIDGE=mock` 全量回退保持可用（终端不可用但 app 可跑，测试依赖）。当前 CSP 为 null 不拦截 `ws://127.0.0.1`；若未来启用 CSP，`connect-src` 须含 `ws://127.0.0.1:*`（记录在案）。

## 4. ade-pty 宿主语义

### 4.1 会话生命周期

- `spawn`：uuid id → openpty(cols, rows) → 解析 shell（§4.5）→ env 组装（§4.6）→ `spawn_command` → 注册表插入 → pre-attach 缓冲就位 → 若有 `command` 立即写入一行（§4.5）→ 回 `{id}` + emit `pty:spawned`。
- `kill`：`child.kill()` → drop master writer/reader（断管道触发 reader EOF）→ 注册表摘除 → supervisor join（超时进 Reaper）→ `pty:exit {id, code}`（code 取 `wait()`；kill 路径取得到的退出码，取不到回 -1）。
- 自然退出：reader EOF → child.wait() 拿 code → 同上收尾。exit 与末帧的顺序：先排空会话出站队列再发事件。
- app 退出：`shutdown_all` = 逐会话 kill + join + WS server close（orcinus-app exit handler 调用）。

### 4.2 背压与读取循环（spike 要求 2）

- reader 线程 64 KiB 块阻塞读 → CPR 扫描（§4.4）→ `tokio::sync::mpsc` 有界通道（128 × 64 KiB ≈ 8 MiB 上限）→ 流式下行（§3.2 修订版：协议处理任务从 outbound 通道取块写入响应体）。
- **队列满即停止 `read` 调用**（通道 `try_send` 失败则睡 5–10ms 重试，不丢块）；背压经 ConPTY 管道/posix tty 缓冲传导到子进程——终端语义本该如此。绝不 drop 字节。

### 4.3 supervisor 与回收（spike 要求 3）

- kill/退出路径：先 drop master（断管道）再 `join()`，join 带超时 2s；超时则线程移入 `Reaper`（`Mutex<Vec<JoinHandle>>`，上限 64，超限丢弃最旧——进程级回收，不挂起不泄漏）。
- 单测：kill 后 join 正常返回；模拟不配合的 reader（假管道）验证 Reaper 兜底路径。

### 4.4 CPR（spike 要求 1/5）

- `cpr.rs` 平移 spike 的 `reply_to_cursor_query`：扫描窗跨块（tail 保留 ≤3 字节，spike 已知局限——命中后 `clear()` 丢同块后续半条查询——在本版修正为保留末尾 ≤3 字节）；命中即向 master writer 回 `\x1b[1;1R`。
- 仅 ConPTY 需要（Windows 启用）；unix 路径旁路（`#[cfg]` 或运行时平台判定，实现取后者以便测试）。
- 单测沿用 spike 两条（跨块、tail 保留）+ 新增「同块双查询」修正用例。

### 4.5 shell 与命令投递

- **PTY child 恒为用户 shell**：unix `$SHELL`（缺省回退 `/bin/zsh` → `/bin/bash`），参数 `-l`；Windows `$COMSPEC`；`shellOverride` 非空时覆盖二进制（Windows 场景 `powershell.exe`/`wsl.exe` 由渲染层盖章，照透传）。agent 在 shell 内以前台子进程运行，退出后回 prompt（对齐 orca UX 与 `getForegroundProcess` 语义族）。
- **命令投递**：`command` 非空时，spawn 完成后立即向 master writer 写 `<command>\n`（tty 输入缓冲保证 shell 初始化完成后才消费；codex `shell-ready` 统一降级，§10 偏差 2）。
- PATH：登录 shell 自行 source profile（unix `-l` 已覆盖 GUI 应用 PATH 缺失问题）；Windows 依赖父进程 PATH + `shellOverride`。

### 4.6 cwd 与 env

- cwd：`cwd` 优先；否则 `cwdFallback:'worktree'` 且 `worktreeId` 可解析时取 worktree 路径（folder 仓库取 `repo.path`；解析失败回 home，记录 warn）；再否则 home。
- env：进程 env 继承 → `envToDelete` 删除 → `env` 覆盖 → unix 追加 `TERM=xterm-256color`、`COLORTERM=truecolor`（已存在则不覆盖）。Windows 交由 ConPTY。

### 4.7 鉴权（修订：随 WS 方案取消）

- 自定义 URI scheme 仅本 webview 可达（其他进程无法构造可被处理的 `orcinus-pty://` 请求），**无需端口/token/鉴权**；`pty_data_endpoint` 命令与 token 机制整体移除。
- 处理函数仍校验路径形态与会话在册性：未知 id → 404 状态。

## 5. 命令面（清单）

`ade-bridge/src/commands/pty.rs`（命名 `<域>_<方法 snake_case>`）：

- 会话：`pty_spawn` `pty_write` `pty_write_accepted` `pty_resize` `pty_signal` `pty_clear_buffer` `pty_kill` `pty_get_cwd` `pty_get_size` `pty_has_pty` `pty_list_sessions`（§3.2 修订：`pty_data_endpoint` 移除）
- 进程检查（stub 同形）：`pty_inspect_process` `pty_get_foreground_process` `pty_confirm_foreground_process` `pty_has_child_processes`
- 快照/投递（stub 同形）：`pty_get_main_buffer_snapshot` `pty_get_authoritative_buffer_snapshot_capabilities` `pty_report_renderer_delivery_state` `pty_get_renderer_delivery_debug_snapshot`
- management：`pty_management_list_sessions` `pty_management_kill_all` `pty_management_kill_one` `pty_management_restart` `pty_management_mac_tcc_attribution`
- preflight：`preflight_refresh_agents`（§7；现 `preflight.ts` 的 `check` 已有真实实现，不动）

stub 同形方法的 Rust 侧即常量回复（无会话逻辑）；`getPtyDataListenerCount` 等纯渲染层同步方法不过 IPC，由 pty.ts 本地实现。

## 6. 渲染层桥接

- 新增 `src/bridge/real/pty.ts`：实现 §2.1/§2.2/§2.3 处置表的全部方法；`src/bridge/create-api.ts` 把 `pty` 加入 `RealDomains` 与 `createRealDomains`；mock 域保留（`VITE_ADE_BRIDGE=mock` 回退与既有 mock 测试不受影响）。
- 新增 `src/bridge/real/pty-stream.ts`（由 pty-socket.ts 演化）：流式数据面客户端——按会话 `fetch('orcinus-pty://localhost/stream/<id>')` 读 ReadableStream（spawn 后建流、流异常终止且无 pty:exit 跟进→判定会话死亡并触发 onExit 本地广播、reload 后按 sessionId reattach）、AbortController 取消、`onData` 分发到 pty.ts 内部 emitter。
- 事件映射：Tauri `pty:exit`/`pty:spawned` → 契约回调载荷。
- 契约/parity 测试：`real/pty.test.ts`（mock `invoke`/`listen`/WebSocket，断言命令名、参数包裹、事件映射、退订、stub 同形返回值）；`parity.test.ts` 对应方法从「未实现」清单迁出。

## 7. preflight.refreshAgents（agent 最小纵切的探测半边）

- 契约：`refreshAgents: (args?: PreflightRuntimeContext) => Promise<RefreshAgentsResult>`（`src/shared/preload-api/api/preflight-api.ts:49`）；`args` 为远端 runtime 语境，1C 本地实现**忽略参数**。
- 语义：
  1. **PATH 水合**：`$SHELL -l -c 'echo $PATH'`（Windows 跳过水合，`pathSource:'sync_seed_only'`，`pathFailureReason:'no_shell'`——Windows PATH 已含用户目录）；2s 超时；成功 → 水合 PATH 与 app seed PATH 的**新增段**记入 `addedPathSegments`，`pathSource:'shell_hydrate'`、`pathFailureReason:'none'`；失败/超时/空 → seed PATH 降级，`pathFailureReason` 按 `ShellHydrationFailureReason`（`'timeout' | 'spawn_error' | 'empty_path' | 'no_shell'`）归类。
  2. **探测**：探针清单 `['claude', 'codex']`（1C 最小集；实现在 `ade-git` 或 `ade-bridge` 内做成可参数化函数，Phase 2 扩全表）；unix 在水合 PATH 各目录探测可执行文件；Windows 按 PATHEXT（`.com/.exe/.cmd/.bat`）解析。
  3. 水合结果**进程内缓存**（首调后复用；设置域未来提供刷新入口时再失效）。
- 消费：渲染层 `detected-agents` store 已接 `api.preflight.refreshAgents`，接真后 agent 选择器展示可用 agent。

## 8. 测试策略

- **Rust（ade-pty）**
  - 单测：cpr（跨块命中、tail 保留、同块双查询修正）；背压（通道满→暂停读→恢复→零丢失，用假 reader 验证）；supervisor（正常 join、超时进 Reaper、Reaper 上限淘汰）；shell 解析（unix/windows 参数与 override）；注册表（spawn/kill/reattach 幂等、exit 后摘除）。
  - 集成（unix CI 可跑；Windows 本机跑）：真 spawn `/bin/sh`——echo 回显经流到达（进程内 subscribe，无需网络栈）、exit 码透传、resize 后 `tput cols` 生效、并发 4 会话互不串扰、`yes` 大输出 8 MiB 计数不丢、command 行投递（`echo done` 于 shell 就绪后输出）、未知 id 流 404。
- **Rust（ade-bridge）**：命令契约测试（命令名、`{args}` 反序列化、错误形状、事件载荷）+ specta bindings 新鲜度（沿用 1A 机制）。
- **TS**：`real/pty.test.ts`（方法全集断言：真实方法走 invoke/WS、stub 同形逐字对齐 web stub、noop 订阅返回退订函数）；`real/preflight.test.ts` 扩 refreshAgents；`parity.test.ts` 迁移；`create-api` 组装测试更新。
- **手工验收**：§1 场景清单；另验 webview reload（dev 下 Cmd+R）后 tab 重挂不崩、死亡 tab UI 可关闭重开。

## 9. 风险

1. ~~**WKWebView/WebView2 对 `ws://127.0.0.1` 的兼容性**（macOS ATS/`NSAllowsLocalNetworking`）→ 实现计划第一个任务先做最小连接验证；fallback：数据面退回 Tauri Channel 分块投递（下行单向，上行仍走命令），接口（onData/write）不变。~~ **已按 §10 偏差 4 处置**：macOS 26 实测 Tauri app 的 WKWebView 网络子进程对环回 WS 静默丢包（Safari 同页同端口正常，排除服务端/OS 网络层），WS 路线否决，数据面改为自定义协议流式（§3.2 修订版）——自定义协议不经网络栈，该风险整体消除。
2. **ConPTY 背压行为**：暂停读后 conhost 缓冲有限，极端输出下子进程被阻塞（预期语义），但需验证恢复后无字节错序/丢失（集成测试 `yes` 用例覆盖）。
3. **登录 shell 水合的边缘**（zsh profile 挂起/慢）：2s 超时 + seed PATH 降级已内置；水合输出可能含 shell 警告噪声 → 取最后一行非空输出并验证含路径分隔符。
4. **`listSessions` 投影形状漂移**：`PtyListedSession` 字段以 `src/shared/pty-listed-session.ts` 为准，契约测试锁形状；dead-session reconcile 对未知字段的容忍度在手工验收确认。
5. **流中断即会话死亡的语义**：进程内直连无网络层断线；若实测出现误杀（如系统睡眠唤醒后的 fetch 流终止），Phase 2 的快照/重放机制顺带解决。

## 10. 相对已批准设计的偏差（需审阅确认）

1. **「独立 PTY 宿主进程」推迟至 Phase 2**：spike 决策（2026-09-14）与上游 §4 决策 2 字面要求独立长驻进程；1C 以进程内 `ade-pty` 实现（不依赖 tauri、注入 runtime handle、WS 协议自包含），Phase 2 重启恢复需要时整体提取为 sidecar，渲染层协议不变。理由：1C 无重启恢复交付物，双进程生命周期/协议版本/崩溃检测的复杂度前置无收益。
2. **shell-ready 命令投递降级为 spawn 即投递**：orca 的标记机制依赖 shell 集成注入（`orca:src/main/zsh-startup-wrapper-builder.ts` 等）；1C 的 tty 输入缓冲方案语义等价（shell 初始化完成后才消费输入），代价是命令回显可能出现在 prompt 之后（外观差异，记录）。
3. **`orcinus-pty` 更名 `ade-pty`**：对齐上游 §5 crate 表；spike 的吞吐 bench/sink 保留为 bins，`docs/spikes/2026-09-14-pty-throughput.md` 的复现命令本就写作 `-p ade-pty`，更名后文档与实现一致。
4. **数据面「本地 socket/WS」改为「自定义协议流式」（2026-10-01 修订）**：spike 决策与原 §3.2 要求终端数据走本地 socket（WS 环回落地）。手工闸门实测（macOS 26 / Darwin 25）：服务端在册、普通进程 `nc` 秒连、Safari 同页同端口 WS 握手正常（OPEN → close 1008），唯独 Tauri app 的 WKWebView 网络子进程报 `The network connection was lost`——本地网络隐私对无 bundle Info.plist 的 ad-hoc dev 二进制静默丢包且无法归因授权（系统设置无条目）；Info.plist 嵌入（`__TEXT,__info_plist`：NSLocalNetworkUsageDescription + ATS NSAllowsLocalNetworking）未能解除。**决策**：下行改 `orcinus-pty://` 自定义协议无限流（处理函数在应用进程内，不经网络栈），上行沿用 `pty_write` 命令；端口/token/鉴权整体取消。spike 决策的意图（不经 Tauri 事件通道、原始字节、背压分片）全部保留；「本地 socket」字面被进程内直连取代。若未来 Windows/其他 macOS 版本实测 WS 可用，可在 §3.2 两形态间选择（数据面接口 onData/write 不变）。
