# Phase 2 子项目 C：diff 注释与评审（本地注释全链路）收尾记录

- 日期：2026-10-07（实施）～ 2026-10-07（终态自动门禁）
- 分支：`main`（spec 基线 `main@5ec74c1`；规格 `9f015252`、计划 `6f49039e`；特性提交 `a6cf52c`…`3ae690ec`）
- 规格：`docs/superpowers/specs/2026-10-07-phase2c-diff-annotations-review-design.md`
- 计划：`docs/superpowers/plans/2026-10-07-phase2c-diff-annotations-review.md`（8 任务；Task 8 手工验收清单保留 `待用户复核`，见 §4）
- 方法：subagent-driven-development（每任务 TDD 实现 + 评审 + fix 轮；终态三组门禁 + 本记录）
- 手工验收状态：**待用户复核（2026-10-07 自动化门禁已全绿；6 项手工清单见 §4）**

## 1. 交付概览

| 项 | 内容 |
|---|---|
| Rust 投影（断点 1） | `ade-core::Worktree` 增 `diff_comments: Option<serde_json::Value>`（camelCase、`skip_serializing_if none`、specta `Option<Json>`）；`apply_worktree_meta` 仅在 `diffComments` 为数组时写入；bindings 重生成；`update_meta_round_trips_diff_comments` 覆盖 update→list→store reload 全链 |
| 渲染层回归 | `worktrees-fetch-listing-merge.test.ts` 新增用例锁定 fetch 合并链路不剥离 `diffComments` |
| 本地终端 RPC 适配器（断点 2） | 新 `src/renderer/src/runtime/local-terminal-rpc.ts`；`callRuntimeRpc` 的 local 分支经 `isLocalTerminalRpcMethod` 挂接缝；未覆盖方法回退 `window.api.runtime.call`（现状语义不变）；错误码 `terminal_handle_stale` / `terminal_exited` / `method_not_found` |
| `terminal.list` | `pty.listSessions` 为活性源；store `ptyIdsByTabId` + `terminalLayoutsByTabId` 补 tabId/leafId；worktree 过滤 + limit；handle = ptyId；`totalCount`/`truncated` 如实 |
| `terminal.agentStatus` / `isRunningAgent` | ptyId→paneKey `${tabId}:${leafId}`→hook 新鲜条目；映射表 working/permission/idle；无 hook 条目→标题证据（`isRunningAgent` = hook ∨ 标题，`status:null` 无证据） |
| `terminal.wait` | tui-idle：250ms 轮询 + 1500ms 输出静默窗 + 标题 idle 兜底；`permission`→`blockedReason:'agent-approval-prompt'`；pty 消失→`exited`；working 到超时→`satisfied:false`；`for:'exit'` 支持 |
| `terminal.send` | `requireAgentStatus:'sendable'` 拒绝语义（`no-agent`/`permission`）；`pty.writeAccepted` 直写文本 + Enter；写入失败→`accepted:false` + 已写 `bytesWritten` |
| 发送栈 | 复用零改动（local target 走适配器）；发送成功→既有 `clearDeliveredDiffComments` 删除已投递快照（§3.4 语义未改代码） |
| 新 agent 路径 | 既有 `launchAgentInNewTab → pasteDraftWhenAgentReady`（本地 `pty.onData` 就绪 + `pty.write`）验证通过，未改代码 |
| fix 轮 | `f7d67e6f` 断开 `runtime-rpc-client → local-terminal-rpc → @/store` 静态环（惰性 `import('@/store')`）；`3ae690ec` pty sidecar 惰性加载 + pane 身份清单补齐（详见 §5.7） |
| 依赖 | 无新增 npm / cargo 依赖；bindings 仅按既有 `export-bindings` 重生成 |

## 2. 任务与提交

| 任务 | 交付 | 提交 |
|---|---|---|
| T1 | Worktree 投影 `diffComments` + `apply_worktree_meta` + bindings + round-trip/reload 测试 | a6cf52cf |
| T2 | fetch 合并保留 diffComments 回归锁定 | 8c049865 |
| T3 | local-terminal-rpc 骨架 + 路由接缝 + `terminal.list` | c182a1e2 |
| T4 | `terminal.agentStatus` / `terminal.isRunningAgent`（hook 映射 + 标题证据） | 49b9ba27 |
| T5 | `terminal.wait`（tui-idle：状态轮询 + 输出静默窗） | 6a039910 |
| T6 | `terminal.send`（直写 + `requireAgentStatus` 拒绝语义） | 9ecf04ec |
| T7 | 发送栈 local 端到端集成（真实适配器 + mock pty）+ 新 agent 路径既有测试验证 | c64865a4 |
| T3 fix 1 | 断开 `@/store` 静态环（惰性 import） | f7d67e6f |
| T3 fix 2 | pty sidecar 惰性加载 + pane 身份 inventory 清单补齐（门禁修复） | 3ae690ec |
| T8 | 三组门禁复跑 + 本记录 + 计划勾选 | 本记录提交 |

## 3. 验收证据（自动化）

三组门禁在终态 HEAD（`3ae690ec`，分支 `main`，`git status` 干净）顺序执行：

| 门禁 | 结果 |
|---|---|
| `cargo test --workspace`（`src-tauri/`） | **exit 0**；40 个 suite（含 doc-tests）累计 **682 passed / 0 failed / 0 ignored**（2B.1 基线 681 + Task 1 新增 round-trip 1 条；含 `bindings_are_fresh`） |
| `rm -f tsconfig.tsbuildinfo && pnpm typecheck && pnpm build:web` | **exit 0**；`tsc --noEmit` 零错误；`✓ built in 6.04s`；`externalized` / `MISSING_EXPORT` / `browser compatibility` **0 命中**；仅既有 >500kB chunk-size 警告 |
| `pnpm test`（repo 根） | **exit 0，全绿**：`Test Files 3859 passed / 0 failed / 8 skipped (3867)`；`Tests 34363 passed / 0 failed / 122 skipped (34485)`；Duration **903.29s**。**本轮无性能抖动失败**（`browser-history-match.performance.test.ts` 与 `palette-match-performance.test.ts` 均通过，无需隔离复跑） |

过程说明：终态前曾有两轮门禁红（`createRepoSlice is not a function` 环、launch-agent mock TDZ 对与 inventory ratchet），对应两次 fix 轮（`f7d67e6f`、`3ae690ec`），修复后本表为最终证据。

## 4. 手工验收清单（待用户复核）

前置：`pnpm dev` 或打包产物均可（本链路不依赖系统权限/签名）。逐项执行并回填结果：

1. 在 worktree diff 上添加注释 → 完全重启 app → 注释仍在原文件/行 —— **待用户复核**
2. 添加未发送注释 → 发送菜单选运行中的 claude → 文本出现在输入框并提交 —— **待用户复核**
3. 发送菜单选「新 agent」→ 新终端启动后提示词自动粘贴并提交 —— **待用户复核**
4. markdown 文件的注释（源码/预览任一）添加 → 重启仍在 → 可发送 —— **待用户复核**
5. folder workspace 注释添加 → 重启仍在 —— **待用户复核**
6. 侧栏 Notes 架：点击定位、复制、清除（单个/全部）回归 —— **待用户复核**

任一项失败：补记到本文件 §5 并修复后，重跑对应自动门禁。

## 5. 规格偏差与边界备案

1. **wait/liveness 近似（spec §7.1）**：宿主级输出静默/进程检查（`inspectProcess` 为 Phase 1C 故意 stub）不可用，tui-idle 判定依赖 hook 状态 + 标题 + 渲染层静默窗；语义偏差在此披露。
2. **handle 生命周期（spec §7.2）**：handle = ptyId；pty 重建后旧 handle 报 `terminal_handle_stale`（对齐宿主 stale 语义）。
3. **标题证据误报（spec §7.3）**：非 hook agent 的 `isRunningAgent` 可能偏保守/偏乐观；guarded 发送仅在 bracketed 握手或 hook 状态成立时进行，否则走 legacy 组合发送。
4. **opaque 投影（spec §7.4）**：`diffComments` 在 Rust 为 `Value`，TS 共享类型仍是唯一 schema；specta 仅做新鲜度校验。
5. **`mobileDiffReview` 仍不投影（spec §7.5）**：与 2B 现状一致。
6. **Task 1：`Json` wrapper 迁移**：`Json` 从 ade-bridge 移到 ade-core（`ade_core::json`），`Worktree.diff_comments` 用 `#[specta(type = Option<Json>)]` 以通过 Specta 导出（内联 `serde_json::Value` 的 specta 定义是 inline 类型，specta-typescript 在命名导出中拒绝/溢出）；无行为变化，bridge 侧 `pub use` 重导出保持 `crate::json::Json` 引用不变。
7. **Task 3 fix 轮 1–2（渲染层静态 import 边教训）**：
   - fix 1（`f7d67e6f`）：`runtime-rpc-client → local-terminal-rpc → @/store` 静态环使真实 store 入口的测试收集失败（`createRepoSlice is not a function`，全量运行中 ~220 个文件 collect-fail、套件 1 小时不完成）→ 改为调用时惰性 `import('@/store')`（缓存）。
   - fix 2（`3ae690ec`）：`local-terminal-rpc` 静态 import `pty-data-sidecar-subscriptions` 使两个既有 launch-agent 测试的 `vi.mock` 工厂在加载期执行、命中非提升顶层变量 TDZ（`Cannot access 'mockSubscribeToPtyData' before initialization`）；且 Task 4 新增的 3 处 `classifyTitleActivity` 未登记进 `pane-agent-identity-inventory` ratchet → `waitLocalTerminal` 改惰性 import sidecar + 清单补录。
   - **教训**：渲染层新增静态 import 边必须先在终态跑一次全量 `pnpm test`（Task 3–7 的局部测试大量 mock `@/store`，无法替代全量门禁）。
8. **Task 5：wait 判定细节**：`terminal.wait` 的 tui-idle 为渲染层近似（hook 状态 + 标题 + 1500ms 输出静默窗）；`isPtyLive` 的 IPC 瞬时失败会让 wait 抛错（在发送栈顶层被捕获为失败结果）。
9. **Task 4/6：非 hook agent 的状态映射**：无 hook 且有标题证据的非 hook agent 会被映射为 `idle`（即使标题显示 working）——guarded 发送仅在 hook/bracketed 证据成立时进行（同 §5.3 披露）。
10. **Task 6：`bytesWritten` 口径**：为 UTF-16 code units（`text.length`），对齐既有 mock 语义；非 UTF-8 字节数。
11. **Task 3：未发射的错误码（spec §3.2）**：本地适配器实际发射 `method_not_found` / `terminal_handle_stale` / `terminal_exited`，未发射 spec §3.2 列出的 `terminal_gone` / `no_active_terminal`；消费侧以等价语义覆盖（终端列表为空 → `no-active-terminal`；send 路径写入前重新校验 pty 活性）。

## 6. 已知边界与后续

- **远端 runtime `terminal.*`**：无 host 实现；本阶段只对 local 的 5 个 terminal 方法做本地实现，其余方法与 remote target 维持 mock/现状回退。
- **未做（spec §2.2）**：sentAt「已发送」保留/徽标与已解决状态模型；PR 评论/hosted review/gh 域；diff 面键盘加注释；文件级注释（lineNumber 0）创建入口；`window.api.shell.openUrl`；浏览器注释面；`mobileDiffReview` 投影；Rust 端 DiffComment schema 校验。
- **可升级点**：Phase 1C `inspectProcess` 落地后可将 wait tui-idle 从渲染层近似升级为宿主级进程/静默判定。
- **回滚策略**：移除 `callRuntimeRpc` 的 local terminal 接缝分支即回到 mock 回退（Rust 投影字段保留无副作用）；渲染层发送栈本身零改动，不受回滚影响。
