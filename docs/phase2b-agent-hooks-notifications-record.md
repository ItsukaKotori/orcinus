# Phase 2 子项目 B：agent hook server + 状态接真 + 完成通知 收尾记录

- 日期：2026-10-06（实施）～ 2026-10-07（终态自动门禁）
- 分支：`phase2b-agent-hooks`（基线 `main@3af79a6`，Phase 2A 已合入）
- 规格：`docs/superpowers/specs/2026-10-06-phase2b-agent-hooks-notifications-design.md`
- 计划：`docs/superpowers/plans/2026-10-06-phase2b-agent-hooks-notifications.md`（12 任务；Task 12 手工验收节保留未勾选，见 §5）
- 方法：subagent-driven-development（每任务 TDD 实现 + 双向评审 + fix 轮；终态门禁 + guard 修复 + 本记录）
- 手工验收状态：**已验收**（用户 2026-10-07 确认六步全通过，无偏差补记；自动化证据见 §3）

## 1. 交付概览

| 项 | 内容 |
|---|---|
| `ade-hooks` 新 crate | std `TcpListener` 绑 `127.0.0.1:0` + token（无 tokio/HTTP crate）；`POST /hook/claude`；meta 头合并 + form 回退；1MB 超限 413、slowloris 超时断连、非 POST 403、未知路由 404；paneKey 归因（空丢弃）；spool 启动重放（逐行消费，坏行保留到下轮） |
| claude 安装器 | `~/.claude/settings.json` 12 事件托管条目（SessionStart/UserPromptSubmit/PreToolUse/PostToolUse/PostToolUseFailure/PermissionRequest/Stop/StopFailure/SubagentStart/SubagentStop/TeammateIdle/PostCompact；Notification/PreCompact 不装）；只增删托管条目、rolling backup、symlink 解引用、CLI 缺失 skip、settings/hooks 非对象记 Error 不 panic |
| 共享脚本 | `~/.ade/agent-hooks/claude-hook.sh`（0755）：neutral stdout `{}` + stdin 捕获 + source endpoint + curl raw/form 双通道 + 失败落 spool |
| endpoint / PTY env | `app_data/agent-hooks/endpoint.env` 0600 原子写；spawn 注入 `ORCA_AGENT_HOOK_*` + `ORCA_PANE_KEY=${tabId}:${leafId}`（2A 透传的 tabId/leafId 接真）+ `ORCA_TAB_ID`/`ORCA_WORKTREE_ID` |
| 状态缓存 | `last-status.json` 250ms 防抖写 + 7 天 TTL + hydrate（`restored` 仅内存，`skip_serializing` 不落盘） |
| bridge 接线 | hook server 启动/退出接线；`agent-hook:raw` 事件（`{source,payload,paneKey,tabId,worktreeId,launchToken,receivedAt,restored}`）；`agent_status_get_snapshot` 回放命令；`agentStatusHooksEnabled` 运行时 reconcile |
| 新命令（3） | `agent_status_get_snapshot`、`notifications_open_system_settings`、`notifications_read_sound`——三处登记点（`collect_commands!`/命令清单测试/generated bindings）一致 |
| TS 域 `real/agent-status.ts` | 订阅 `agent-hook:raw` → 复用 `shared/agent-hook-listener.ts` 归一化 → `AgentStatusIpcPayload`；`getSnapshot()` 走独立 replay 状态回放；`onClear` 为 noop 订阅（宿主不产 clear） |
| TS 域 `real/notifications.ts` | `tauri-plugin-notification`：dispatch（设置门控/冷却/焦点抑制）、dismiss（FNV-1a 哈希id）、getPermissionStatus、probeDelivery（`authoritative:false`）、playSound（custom 音效经 `notifications_read_sound` → Blob/Audio）、openSystemSettings |
| mock / parity | `src/bridge/mock/agent-status-api.ts`、`notifications-api.ts` 保持既有空态；`create-api.ts` RealDomains + parity 键集门禁覆盖两域（含 fallback noop 集合） |
| renderer shim + vite alias | `src/renderer/src/lib/browser-node-shims.ts`（buffer/crypto/fs/fs-promises/os/path/http 浏览器替换）+ 根 `vite.config.ts` alias；归因图零 node 外置告警 |
| 通知插件 | Rust `tauri-plugin-notification = "2"`（连带 tauri 2.11.5→2.12.1，均在 tauri 闭包内）+ TS `@tauri-apps/plugin-notification ^2.5.1` |

## 2. 任务与提交

| 任务 | 交付 | 提交 |
|---|---|---|
| T1 ade-hooks 基座 | endpoint.env 原子发布 + pty env 映射 | 6cf47a5 |
| T2 状态缓存 | 250ms 防抖 + hydrate + `restored` 仅内存裁定 | e92754e..b1f987b |
| T3 共享脚本 | claude-hook.sh + spool 兜底 | 5655ee4 |
| T4 安装器 | 12 事件 + backup/symlink/skip + 非对象防 panic 修复 | 5c4c0a6..e42fe21（+ c92e860 计划修订） |
| T5 HTTP server | token/路由/限长/slowloris/归因/spool 重放 + 字节安全修复 | 2368b74..40449ff |
| T6 bridge 接线 | server 启停 + `agent-hook:raw` + snapshot 命令 + bindings | c646eb4 |
| T7 pty env 注入 | `ORCA_*` 全套 + tabId/leafId 接真 | 24cacb8 |
| T8 开关 + 通知宿主 | runtime reconcile + 两宿主命令 + 插件接线 | dfc1595 |
| T9 shim + alias | 7 specifier 浏览器 shim + vite alias + transcript-reader Buffer 导入修复 | 7d7fec6 |
| T10 agentStatus real | raw→归一化→payload + snapshot 回放隔离修复 | 9c04a35..7a2995f |
| T11 notifications real | plugin 通知/权限/自定义音效/dismiss 哈希映射 | ba4b8cc |
| T12 门禁 + 记录 | guard 窄豁免修复 + 三组门禁 + 本记录 | 3c48dfa + 本记录提交 |

## 3. 验收证据（自动化）

三组门禁在终态 HEAD（分支 `phase2b-agent-hooks`）顺序执行，全部 exit 0：

| 门禁 | 结果 |
|---|---|
| `cargo test --workspace`（`src-tauri/`） | **exit 0**；40 个 suite（含 doc-tests）累计 **674 passed / 0 failed**（含 ade-hooks endpoint/cache/script/installer/server、bridge 命令清单与 bindings 新鲜度） |
| `pnpm test`（repo 根） | **exit 0**；Test Files **3857 passed / 8 skipped (3865)**；Tests **34330 passed / 122 skipped (34452)**；Duration 591.06s |
| `pnpm typecheck && pnpm build:web` | **exit 0**；`tsc` 零错误；`✓ built in 6.09s`；构建日志中 `externalized` / `browser compatibility` / `MISSING_EXPORT` **0 命中**；仅既有 >500kB chunk-size 警告 |

**先前 guard 修复记录（3c48dfa）**：上一轮全量 `pnpm test` 曾在既有 `src/renderer/src/renderer-node-builtin-boundary.test.ts` 失败——BFS 源图发现 8 条链、6 个 `node:*` 内置（buffer/crypto/fs/fs-promises/os/path）经 `bridge/real/agent-status.ts` 进入 renderer。裁决：不重构 shared 归一化图（遵守规格「TS 复用」），把门禁升级为对 agent-hook 链的**窄豁免**：精确 allowlist（8 个 importer 文件 × 6 个允许 specifier × 链必经 agent-status.ts），并新增断言要求 `vite.config.ts` 确实为每个允许 specifier 配 alias（删 alias 即测试失败）。红绿探针（移除一个 importer → 定向失败；恢复 → 绿）通过，scoped review Approved；修复后 `pnpm test` 34330 passed / 0 failed。Minor 留档：alias 断言为子串匹配，注释掉的 alias 行仍可能通过。

## 4. 手工验收（已验收 2026-10-07）

前置（已确认）：`pnpm dev` 启动；Settings → Agents 确认 `agentStatusHooksEnabled` 开；`~/.claude/settings.json` 出现 12 个托管事件；`~/.ade/agent-hooks/claude-hook.sh` 存在且 0755。

1. **已验收**：终端 pane 启动 `claude`，发 prompt → tab/侧栏状态变 working（`UserPromptSubmit`）。
2. **已验收**：触发权限请求 → waiting + 桌面通知 + 未读徽标/高亮。
3. **已验收**：完成回复 → done；点进 pane → 自动已读、徽标清除。
4. **已验收**：Settings 关 `agentStatusHooksEnabled` → `~/.claude/settings.json` 托管条目被移除 → 新对话不再驱动状态；再打开 → 条目回来。
5. **已验收**：重启 app（不重启 claude 进程）→ 状态经 `agent_status_get_snapshot` 回放（restored 行显示为未确认、不重新通知）。
6. **已验收**：断网/服务降级兜底——server 停用时 2A transcript 扫描仍能捕获休眠记录。

计划文件 Task 12 Step 2 已勾选；手工项全通过，无偏差补记。任一项失败时的补记流程保留在下：失败 → 补记到本文件偏差节并修复后重跑对应自动门禁。

## 5. 规格偏差备案

1. **dismiss 在 2B 为确定 no-op**（`notifications.ts:63`，修正先前记录）：已核实 `tauri-plugin-notification` 2.5.1 desktop 的 `sendNotification` 忽略 `id` 选项，且 `remove_active` 未注册（实现仅存在于 mobile crate 侧），故 macOS 上 `removeActive` 调用必然失败并被兜底为 `{dismissed:0}`——与字符串 id 的 FNV-1a 哈希映射无关，接受该行为为 2B 终态。desktop 精确 dismiss 需换机制（如原生 API），留 2B.1 跟进。属计划内 R4 裁定，dismiss 消费方（`ui-slice-activity-actions`）在无匹配时的行为等价于未取消。
2. **playSound 仅支持 custom 音效**：仅当 `customSoundId === 'custom'` 且 `customSoundPath` 存在时经 `notifications_read_sound` 播放；其余 9 个内置音效资产不在 2B 范围，返回 `missing-path`。属计划内 R4 裁定。
3. **开关语义裁定**：启动期 reconcile 时关闭 = **skip 不删**（防多 profile 互删）；用户显式 toggle 关闭 = **移除托管条目**（对齐 oracle `applyAgentStatusHooksEnabled`，保证 §1 验收第 4 步「关开关 → 状态停驱」可达）。解决规格 §4 与 §1 的内部张力，Task 5/8 测试钉死。
4. **413 / 非 POST 403**：body 超限回 **413**（不采用 oracle 的 fail-open 204）、非 POST 回 **403** 并计数（不采用 oracle 的 404）——按规格 §4 表覆盖 oracle；未知路由仍 404。Task 5 测试钉死。
5. **macOS 桌面权限限制**：`tauri-plugin-notification` desktop 权限恒为 Granted → `blocked-by-system` 回退在 macOS 上不可达；`getPermissionStatus.requested` 实际表示「已授权」而非「询问过」。任务内无法接真（缺原生授权读口/探针），留 2B.1 跟进；`probeDelivery` 因此恒返回 `authoritative:false`（计划要求备案）——探测结果只作参考、不作权威结论。用户拒绝系统通知时无 in-app 回退提示（静默无通知）。
6. **agentStatus snapshot 回放使用独立 replay 状态**：每次 `getSnapshot()` 新建 replay normalizer/epoch，不触碰实时 `listenerState`/epoch map；replay epoch 仅在实时 map 无该 pane 时合并（首次 hydration 采纳，绝不覆盖实时 epoch）。修复「回放与实时事件交错污染」与「重复 getSnapshot 被一次性守卫吃条目」两问题，交错回归测试钉死（`9c04a35..7a2995f`）。
7. **renderer node-builtin 边界门禁的 agent-hook 链窄豁免**：豁免仅在 allowlist importer × 别名 specifier × BFS 链必经 `bridge/real/agent-status.ts` 三者同时成立时生效；其余 node 引用照旧失败，并有 vite alias 配置断言兜底（见 §3 guard 修复记录）。

**其他已裁定小项**（详见 `.superpowers/sdd/2026-10-06-phase2b-agent-hooks-notifications/progress.md`，git 历史为最终依据）：`CachedHookEvent.restored` 改 `skip_serializing`（磁盘永不携带，hydrate 重标）；`read_settings_json` 对非对象 settings/hooks 返回 Error 防安装路径 panic；`remove_claude_hooks` 目标缺文件不凭空创建；`percent_decode` 字节安全防 token 持有者触发 panic；spool 逐行保留未解析行；`serde_json::Value` 无法 specta 导出 → 既有 `crate::json::Json` 透明包装；计划中陈旧的 `hooks_installation_present` 接口声明删除（无消费者）；Task 9 三处 brief 偏差（补 shim 导出、transcript-reader 显式 `Buffer` 导入、`Buffer.from` 签名放宽）。

## 6. 已知边界与后续

- **2B.1 跟进**：macOS 通知权限真探测（blocked-by-system 回退 + requested 语义）；desktop 精确 dismiss 换机制（2.5.1 `sendNotification` 忽略 `id`、`remove_active` 仅 mobile 未注册，hash 映射在 macOS 不可达）。
- **后续 Phase**：2C/2D 按上游路线图；其余 17 个 agent 源（codex hooks.json 等）按需扩展（2B 为 claude-only 指令）；Windows hook 脚本分支（先 POSIX）顺延；9 个内置通知音效资产。
- **明确不做（规格 §2.2 防蔓延）**：Notification/PreCompact 事件安装（避免误报）、statusline 用量路径、通知设置页扩展、归一化移植 Rust、drop 系/migration 维持 fallback noop。
- **Cargo.lock 刷新范围**：本次锁变更保持在 tauri/plugin 闭包内（不引入闭包外新依赖），但移动了若干 tauri 家族传递依赖（tao/wry/brotli/html5ever 等），已由三组绿色门禁验证。
- **留档 minor**：slowloris 为 per-read 非 per-connection、handler 线程无上限；`read_sound` 无路径授权（≤10MiB 任意扩展名文件可读，需 conscious sign-off）；read_sound TOCTOU；open 后不 wait 的僵尸；alias 断言子串匹配；snapshot epoch map 无逐出路径（随 pane 数有界）；Task 11 报告「dismiss 无消费方」不实（`use-notification-dispatch` 传 id、`ui-slice-activity-actions` 调 dismiss，已记录）；tauri 2.12.1 lock bump 均在本项目 tauri 闭包内（final review 已复核）。
