# Phase 2 子项目 A：持久化基座 + 终端会话恢复 收尾记录

- 日期：2026-10-06
- 分支：`worktree-phase2a-persistence-session-restore`（基线 `main@93bd249`）
- 规格：`docs/superpowers/specs/2026-10-01-phase2a-persistence-session-restore-design.md`（含 §0 修订 R1-R5）
- 计划：`docs/superpowers/plans/2026-10-01-phase2a-persistence-session-restore.md`
- 方法：subagent-driven-development（每任务独立实现者 + 双向评审 + 终审 + 修复波）

## 1. 交付概览

| 项 | 内容 |
|---|---|
| SQLite 基座 | `ade-store/sqlite.rs`：rusqlite bundled、`user_version` 迁移框架（v1 = `workspace_sessions` 单表，顶层字段一行 JSON）、损坏库改名 `*.corrupt-<ms>` 隔离重建、WAL + 双 checkpoint |
| session 域 | `session_get/set/patch/flush`——Rust 侧不透明 JSON 文档存储（顶层键整键替换；set 整状态替换含缺键删除）；渲染层 `real/session.ts` 读侧合并规范默认形状 + 读侧休眠记录按 providerSession.id 去重 |
| 退出 flush 协议 | `WindowEvent::CloseRequested`（webview 存活期）→ `prevent_close`（一次性 `CloseFlushLatch` 防 `close()` 重入死循环）→ emit `session:flush-requested` → 渲染层确定性捕获 + 全量 patch + flush → `session_flush_ack` → 宿主后台线程 2s 超时放行关闭；`RunEvent::Exit` 兜底 flush + `wal_checkpoint(TRUNCATE)` + shutdown_all |
| transcript 扫描 | `agent_sessions_resolve_capture`——claude（`~/.claude/projects/<munged-cwd>/` 时窗内最新 jsonl）/ codex（`~/.codex/sessions/` 递归 + 头部 cwd 匹配 + 文件名末 5 段 dashed UUID）；500ms 预算；未命中降级普通 shell |
| scrollback 持久化 | 序列化文本内联 `TerminalLayoutSnapshot.buffersByLeafId`（≤512KiB 截尾，fork 机械）随 session state 落 SQLite；本地 repo 保留翻转（ade 无 daemon，渲染层捕获是唯一持久化）；60s 间隔 + visibilitychange 隐藏触发 + 关停/休眠点强制 |
| resume 链路 | OSC 身份门废弃（无 shell 集成下为死信号）→ transcript 存在性扫描即证据；按 providerSession.id 捕获期去重 + 读侧去重（历史脏数据自愈）；消费复用 fork 冷恢复机械（`bindBuildColdRestoreAgentResumeStartup`），quit 记录 pane 缺失时不新开 tab（gating 修复） |
| reload 首绘 | 根因 = WKWebView reload 后 timer 节流饿死 xterm WriteBuffer（回放字节停在 depth>0，探针 10s 超时）；修复 = 回放写同步冲刷（`_writeBuffer.flushSync()`，CoreTerminal.resize 同款原语）+ settled 帧 fit + 全视口 present |

## 2. 任务与修复轮

| 任务 | 交付 | 提交 |
|---|---|---|
| T1 SQLite 基座 | 迁移框架 + CRUD + 损坏隔离（1 修复轮：with_extension 碰撞→OsString 追加） | 5f57c28..96dfbe6 |
| T2 session 命令 | 四命令 + AppState 集成 + bindings（1 已裁定偏差：E0308 → `Ok(?…)`） | a5c6ef2 |
| T3 transcript 扫描 | claude/codex 扫描 + specta 类型（1 修复轮：codex id 取整段 UUID） | bc72ea2..9a1a786 |
| T4 退出 flush 协议 | 事件 + ack 握手 + ExitRequested 编排（1 已裁定偏差：wait_timeout_for nightly-only → wait_timeout_while） | 80eda11 |
| T5 渲染层 bridge | session real + flush 注册面 + preflight 捕获契约（扁平类型前提修正） | a8f0d88 |
| T6 buffers 保留翻转 | 本地 repo 纳入持久化（R2） | 9c30300 |
| T7 R3 捕获调度 | 60s + visibilitychange（happy-dom 环境标注 + 过时注释修正） | 33abfb6 |
| T8 transcript 捕获模块 | OSC 身份 + 扫描 → 注册表（后续 3 轮验收修复：棘轮 fixture / 去标题门 / providerSession 去重） | 59895de..2b5bbf6 |
| T9 flush 接线 | 确定性捕获 + 全量 patch + flush → ack | 12dc77c |
| T10 门禁 + 终审 + 修复波 | 见 §3/§4 | af54f20..5160141 |

## 3. 终审与验收期修复（全部经定向复审）

| 轮 | 发现 | 修复 | 提交 |
|---|---|---|---|
| 终审 Critical | R2 握手不可达：`ExitRequested{code:None}` 在 webview 销毁后发出 | 迁移 `CloseRequested` + `CloseFlushLatch` 防重入 | aa98af9 |
| 终审 Important | `captureTranscripts` 拒绝中止 flush 而 ack 仍流转 | catch-and-continue，patch/flush 必达 | c94e4a6 |
| 验收轮 2 | 裸 `{}` 违反 WorkspaceSessionState 契约 → 水合崩溃 → 写入门控永久关闭 | get() 读侧合并规范默认 | 3366da7 |
| 验收轮 3 | OSC 身份门在无 shell 集成下永不打开（标题恒 'Terminal 1'） | transcript 存在性扫描即证据（R5） | ab0cb6f |
| 验收轮 4 | split 同 cwd 双 pane 认领同一 providerSession | 按 id 捕获期去重（R5 补充） | 2b5bbf6 |
| 验收轮 5 | 历史重复记录读侧复活 | get() 读侧按 id 去重（最早 capturedAt 保留） | 74dd54c |
| 验收轮 6 | reload 后回放不绘制（timer 节流饿死 WriteBuffer） | 写缓冲同步冲刷 + settled 帧 fit/present | 45d43c1..ae67d90 |
| 验收轮 7 | quit 记录在 pane 缺失时仍被消费开新 tab resume | activation 扫描三门 gating（R5 语义收口） | 0a71f83 |
| 收尾 | 诊断剥离 + 孤儿记录注释修正 | 17 文件剥离 + 删 1 文件，修复代码字节不变 | 5160141 |

## 4. 测试与门禁（最终态）

- Rust：`cargo test --workspace` 632 passed / 0 failed（含 bindings 新鲜度、命令名锁定、迁移/隔离重建/扫描 fixture/信号握手）
- TS：`pnpm test` 34301+ passed / 0 failed；`pnpm typecheck` exit 0；`pnpm build:web` exit 0
- speca/bindings：五新命令（session_get/set/patch/flush/flush_ack + agent_sessions_resolve_capture）双清单登记，`bindings_are_fresh` 绿
- 手工验收：终版 4 场景全过——①关 claude tab → 重启不复活不误 resume；②claude tab 保留 → 重启自动 resume（split 只 resume 一个）；③reload 立即回放；④SQLite 损坏演练降级启动 + corrupt 隔离

## 5. 已知边界与延后项

**边界（留档）**：退出握手覆盖窗口关闭路径，Cmd+Q / `AppHandle::exit` 为防抖兜底（R4）；reload 恢复为尽力语义（hidden 触发 + 防抖竞态，多数落地）；退出瞬间最后一批序列化产物可能丢失（≤60s，R3 兜底）；误报语义（跑过并退出的 claude pane 会被自动 resume）在已批准 UX 内，2B hooks 恢复精度。

**延后项（2B+ / 后续）**：
- hook server + agent TUI 状态识别（2B，恢复 resume 精度与状态面）
- `layoutTabs=0` 时已关 tab 的 `tabsByWorktree` 行仍留文档（行为符合规范，值得归零）
- mergeSleepingAgentSessionRecords 动作单测；captureTranscripts 拒绝已免疫但 husk 例外缺 quit 用例
- includeLocalBuffers 选项名翻转后语义误导（3 调用点，改名超范围）
- session 域四命令 async fn 内同步 SQLite IO（微秒级可容忍）；apply_patch 非原子（自愈）
- 孤儿休眠记录无限保留（安全方向；陈旧清理对 quit 记录不生效——注释已修正）
- codex transcript 目录布局无官方契约（宽松匹配，失败降级）
- session.get 读侧深归一化不深入 tabsByWorktree 内部（顶层键整键替换语义一致）

## 6. 裁定记录（摘要）

全程裁定逐条见 `.superpowers/sdd/2026-10-01-phase2a-persistence-session-restore/progress.md`（收口后删除，git 历史为本记录）。要点：PreloadApi session 扁平化适配；退出握手迁移 CloseRequested（Tauri webview 生命周期约束，spec R4）；捕获门改 transcript 存在性扫描（R5）；providerSession 去重两处（捕获期 + 读侧）；session.get 读侧合并规范默认。验收期两次「旧构建」误报已流程性纠正（冷启动 + 构建新鲜度标志）。
