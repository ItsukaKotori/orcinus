# Phase 2 子项目 E：automations（本地定时自动化）设计规格

- 日期：2026-10-09
- 状态：brainstorming 输出（调度器 = Rust 全量、首切片 = 最小闭环、CRUD = local runtime RPC 拦截 + 类型化命令、新增依赖 chrono——均已批准；本规格为实现依据）
- 基线：`main@f9043916`
- 行为 oracle：`/Users/itsuka/CodeSpace/orca`（Electron 版，只读参照；`orca:` 前缀均为其内路径）
- 上游规格：`docs/superpowers/specs/2026-09-14-ade-design.md`（§8 Phase 2「automations（按依赖排序）」）
- 前置事实（2026-10-09 本会话双探索代理）：
  - **渲染层产品面几乎全量已移植**（约 130 个文件）：Automations 页面/编辑器/Runs 仪表盘、dispatch 循环（`hooks/useAutomationDispatchEvents.ts` → `hooks/automation-dispatch-handler.ts`）、完成观察（`hooks/automation-dispatch-completion.ts`，基于 `agentStatusByPaneKey`）、会话复用、后台 agent 启动（`lib/launch-agent-background-session.ts`，本地 `pty.spawn`）、`new_per_run` 建工作区（带 `automationProvenanceRequest`）、shared 的 cron/RRULE 解析与 occurrence 数学、保留策略（`MAX_AUTOMATION_RUNS_PER_AUTOMATION=100`）、keyset 游标、host 缓存/刷新调度。
  - **宿主侧全缺**：`window.api.automations` 仅 3 方法 mock（`bridge/mock/automations-api.ts`），其余 9 方法 fallback reject；无 `real/automations.ts`、无 parity 覆盖；`callRuntimeRpc(local, 'automation.*')` 落到 mock `runtime.call` reject；无存储、无 tick、无 precheck 执行器、无完成对账；`specta_export.rs` 无任何 automations 命令。
  - **参照架构**：`orca:src/main/automations/service.ts` 60s tick（`DEFAULT_TICK_MS=60_000`、重入旗标、串行评估）→ 逐条 `requestDispatch` → 桌面经 `automations:dispatchRequested` 交渲染层执行 → `markDispatchResult` 写回；`run_once_within_grace`（默认 720 分钟）只派发最近一次、超 grace 写一条 `skipped_missed`；`(automationId, scheduledFor)` 去重；保留 100 条仅终态可逐；启动对账（`retained-run-reconciliation.ts`）对无宿主遗留运行在结算窗后写 `dispatch_failed`；precheck 为 shell 命令（本地 `spawn` detached、超时组杀、各流 4000 字符尾截断）。
  - **参照契约**：CRUD 走 runtime RPC（`automation.list/show/create/update/delete/runNow/runs`），`window.api.automations` 只承载 precheck/markDispatchResult/snapshotWorkspaceName/rendererReady 与 external-manager 域；事件 `automations:changed`（每次写入后）与 `automations:dispatchRequested`。
  - **参照边界**：external managers（hermes/openclaw）、SSH 多宿主/owner fencing、usage 计费、CLI、headless serve 均为独立子系统（本规格不做）。

## 1. 背景与目标

GitHub 外部集成按用户决定暂停/跳过。本子项目（automations）交付**本地定时自动化**的最小闭环：定义与运行记录持久化、定时/手动触发、把提示词自动送进后台 agent、观察完成并留历史。

- **Rust 内核**（新 crate `ade-automations`）：存储、cron/RRULE 数学、60s tick、missed-grace、保留、precheck 执行、dispatch token、启动对账
- **传输**：新增类型化命令 + local `automation.*` RPC 拦截 + `automations:changed`/`automations:dispatch-requested` 事件；`real/automations.ts` 接真
- **渲染层执行零改动**：既有 dispatch/完成观察/会话复用/工作区创建链路原样工作

**验收（自动 + 手工）**：创建定时自动化 → 到点派发 → 后台 agent 执行 → 完成并记录历史；`runNow` 手动触发；precheck 失败 → `skipped_precheck`；`new_per_run` 生成工作区；重启后定义/历史/下次触发保留；错过超 grace → `skipped_missed`。自动化门禁：`cargo test --workspace` 全绿 + `pnpm test` 全绿 + `pnpm typecheck && pnpm build:web` exit 0。

## 2. 范围

### 2.1 实现面

| 面 | 载体 | 说明 |
|---|---|---|
| 内核 crate | `ade-automations`（新） | 存储 + schedule 数学 + tick 服务 + precheck + 对账（§3.1） |
| Tauri 命令 | `ade-bridge` 新增 10 命令 | CRUD/precheck/markDispatchResult/snapshot/rendererReady（§3.2） |
| 事件 | `automations:changed` / `automations:dispatch-requested` | Rust 发布，`real/automations.ts` 订阅转发（§3.2） |
| 桥接 | `real/automations.ts` + `create-api.ts` RealDomains | 9 个非 external 方法接真；external 维持 fallback |
| local RPC | `runtime-rpc-client` local 分支 + `local-automation-rpc.ts` | `automation.list/create/update/delete/runNow/runs` 映射（§3.3） |
| 设置接线 | `showAutomationsButton` 等 | 已有默认与持久化，无需改动 |

### 2.2 明确不做（防蔓延）

External managers（hermes/openclaw：probe/cache/relay/命令执行）；SSH 多宿主调度（`ssh_bridge`/`remote_host_service`、`executionTargetGeneration`、workspace pin/repin）；owner fencing（`selector`/`expectedOwner`/`destination`/orphan 投影——参数接收后忽略）；usage 计费（token/成本归因、`usageSummary`）；CLI（`orca automations …`）；headless serve 调度（tick 依赖渲染层 `rendererReady` 武装）；skip 折叠（`occurrenceCount`）；mobile/web 客户端；`automation.show`（渲染层未消费）；上下文迁移（legacy `projectId→runContext` 回填）。

## 3. 架构

### 3.1 Rust 内核（`ade-automations`）

- **`AutomationStore`**：`automations.json`（`{schemaVersion, automations: [...], runs: [...]}`；原子写 + 损坏隔离重建，沿用 `WorktreeMetaStore` 先例）。
  - CRUD：list（含 fenced 参数忽略）、create（`creationKey` 幂等）、update、delete（级联删除其 runs）、runNow（`scheduledFor=now`、不受 `enabled` 限制）、runs（keyset 游标 `createdAt desc, id asc`、页上限 100、`nextCursor`）。
  - 去重：`(automationId, scheduledFor)` 命中返回既有行。
  - 保留：每自动化 100 条，仅**终态**可逐（`isFinalAutomationRunStatus`）；`runNumber` 在裁剪前回填、递增自最高值。
  - 定义校验：name/prompt 非空、`agentId` 为已知 TUI agent、rrule 可解析、precheck 超时 1–600、`missedRunGraceMinutes` 默认 720、`dtstart` 下界。
- **`schedule` 模块**：逐字移植 `shared/automation-schedule-parsing.ts` + `automation-schedule-occurrences.ts` + `automation-cron-occurrence.ts` 语义：
  - 含 `=` → RRULE（`FREQ=HOURLY|DAILY|WEEKLY`、`BYDAY`、`BYHOUR`、`BYMINUTE`），否则 5 字段 cron（`*`/范围/列表/步进/名称、DOW 7→0、DOM/DOW 双限 OR）；cron 文本 ≤ 2KiB；9×366 天内无解 → 拒绝。
  - `next_occurrence_after(rrule, dtstart, after)` 与 `latest_occurrence_at_or_before(rrule, dtstart, now)`；**本地墙钟**语义（对齐参照；`timezone` 仅存储）。
  - 时间数学用 `chrono`（`clock` feature；新增依赖，见 §7.5）。
- **`AutomationService`**：`start(store, app_handle)` → `setInterval` 60s tick；`armed` 由 `rendererReady` 置位；重入旗标；每 tick：
  1. `reconcile_stranded_runs`（仅首次 armed 后 + 结算窗 2 分钟；本进程启动前的非终态 → `dispatch_failed` + 固定文案）。
  2. 遍历 enabled 且 `nextRunAt<=now`：算 `scheduledFor=latest ≤ now`；`now-scheduledFor > grace*60_000` → 写 `skipped_missed`（固定文案）+ advance；否则创建/去重 run、置 `dispatching`、发 `dispatch-requested` + 30 分钟内存 token、advance。
  3. `advance`：`nextRunAt = next_occurrence_after(now)`（无回填）。
- **`precheck`**：`std::process::Command` + `sh -c`（POSIX）+ `process_group(0)`；默认 60s/上限 600s；超时组杀（SIGTERM → 2s → SIGKILL）；stdout/stderr 各截 4000 字符尾；返回 `AutomationPrecheckResult`（`exitCode===0 && !timedOut && !error` 为通过，判定在渲染层）。
- **写入即广播**：store 每次变更后发 `automations:changed{selector?:{kind:'self'}}`（首切片无 selector 亦可；payload 兼容既有消费方）。

### 3.2 Tauri 命令与事件（ade-bridge 薄层）

命令（camelCase args，`run_blocking` 包阻塞 IO）：
`automations_list`, `automations_create`, `automations_update`, `automations_delete`, `automations_run_now`, `automations_runs`, `automations_run_precheck`, `automations_mark_dispatch_result`, `automations_snapshot_workspace_name`, `automations_renderer_ready`。
- `automations_runs` 入参兼容三种形态：`{automationId?, expectedOwner?, limit?, cursor?}`（fenced/legacy 均忽略额外字段）。
- `automations_mark_dispatch_result` 写回 run 字段（status/workspace/terminal ids/outputSnapshot/precheckResult/error/startedAt/dispatchedAt）并保留 `dispatched→completed` 幂等（终态不再被覆盖）。
- `automations_snapshot_workspace_name` 把 `{workspaceId, displayName}` 写进该工作区最近运行（对齐 `remove-worktree` 的调用时机）。
- `automations_renderer_ready` 武装服务 + 触发追赶 tick。

事件（Tauri event，名与既有 `pty:exit`/`agent-hook:raw` 风格一致）：
- `automations:changed` → `real/automations.ts` `onChanged` 转发（payload `{selector?, reason?}`）。
- `automations:dispatch-requested` → `onDispatchRequested` 转发（payload `AutomationDispatchRequest`，含 `dispatchToken`）。

`real/automations.ts`：接真 9 方法（precheck/markDispatchResult/snapshotWorkspaceName/rendererReady + 两订阅 + `retainExternalScopes` 与 external 五方法维持 fallback）；`create-api.ts` `RealDomains` 加入 `automations`；parity `surfaceCases`/`realApiFor` 增加条目（explicit 9 / missing 5）。

### 3.3 local runtime RPC 拦截（TS）

- `runtime-rpc-client.ts` local 分支：`automation.` 前缀方法（`automation.list|create|update|delete|runNow|runs`）→ `local-automation-rpc.ts` → 对应类型化命令。
  - `automation.list`：返回 `AutomationListResult` 形态 `{automations, items, orphanCount?}`（items 每项 `{automationId, selector:{kind:'self'}}`；渲染层页面消费形态见既有测试）。
  - `automation.create/update`：`projectId→repo:"id:<id>"`、`workspaceId→workspace:"id:<id>"` 的 wire 重命名在 TS 侧反向映射（既有客户端发送的是 wire 形态，Rust 命令直接存 `projectId/workspaceId` 字段即可——实现时以既有客户端测试锚定的参数形状为准）。
  - `automation.runs`：`{automationId, limit?, cursor?}`；`expectedOwner` 忽略。
- 未覆盖的 `automation.*`（`show` 等）→ 走现有 fallback（渲染层未消费）。

## 4. 数据流

1. **启动**：`AppState` 初始化 → `AutomationStore::load` → `AutomationService::start`（tick 空转）。
2. **武装**：渲染层 boot（`useAutomationDispatchEvents` 既有）→ `rendererReady` → armed + 立即追赶 tick。
3. **tick → 派发**：见 §3.1；`dispatch-requested` → 渲染层 `handleAutomationDispatchRequest`（工作区解析 → 可选 precheck → 复用/启动后台 agent → `markDispatchResult(dispatched)`）。
4. **完成**：渲染层完成观察（agentStatus `done`/PTY exit 佐证）→ `markDispatchResult(completed + outputSnapshot)`；不可验证丢失保持 `dispatched`。
5. **对账**：`rendererReady` + 2 分钟结算窗后，遗留非终态 → `dispatch_failed`（固定文案）。
6. **CRUD/历史**：页面 → store 客户端 → local 拦截 → 命令 → store → `automations:changed` → 页面刷新；runs 分页经 `automation.runs`。
7. **保留**：run 创建与加载时逐；`runNumber` 连续。

## 5. 错误处理与边界

| 场景 | 行为 |
|---|---|
| 非法 rrule/cron/precheck 超时 | create/update 返回 validation 错误（不落盘） |
| 错过超 grace | 一条 `skipped_missed` + "Orca was unavailable during the missed-run grace window."（逐字） |
| 重复 occurrence | `(automationId, scheduledFor)` 去重返回既有 run |
| armed 前 tick | 不派发、不写 dispatching（追赶在 `rendererReady` 后一次） |
| precheck 失败/超时 | 结果落盘；渲染层置 `skipped_precheck` |
| 派发中应用退出 | 重启后对账 → `dispatch_failed`（固定 stranded 文案） |
| 非终态 run 保留 | 不逐（仅终态可逐） |
| store 损坏 | 隔离备份 + 重建（`WorktreeMetaStore` 先例） |
| fenced 参数（selector/expectedOwner/destination） | 接收并忽略（首切片无多宿主）；不报错 |
| external 方法 | 维持 fallback reject（UI 外部区显示不可用，披露） |

## 6. 测试与门禁

### 6.1 Rust

- `schedule`：对齐 `shared/automation-schedules.test.ts` 场景（weekday 不出周末、闰日 cron、hourly latest/grace、off-minute dtstart、Sunday/weekly 往返、malformed BYDAY、DOM/DOW OR/AND、无解拒绝、2KiB 上限）。
- `store`：CRUD、`creationKey` 幂等、`(automationId,scheduledFor)` 去重、保留 100 仅终态、`runNumber` 回填/连续、keyset 分页稳定性、级联删除、损坏隔离重建。
- `service`：due→dispatch 一次、grace 内派发最近一次、超 grace skipped_missed、advance from now、armed 门、重入保护、对账（遗留非终态 → stranded 固定文案）。
- `precheck`：成功/非零/超时组杀/4000 字符尾截断/600s 上限。
- bindings 新鲜度 + 命令清单（10 新命令）。

### 6.2 TS

- `local-automation-rpc`：6 方法映射 argv/参数、fencing 参数忽略、legacy 形态（`null` params / 无 selector）。
- `real/automations`：9 方法路由（命令名 + 参数）、两事件订阅转发、external 维持 fallback。
- `create-api`/parity 更新。
- 既有约 130 个自动化测试保持全绿（页面/客户端/调度刷新/完成观察/会话复用等锚定行为不变）。

### 6.3 手工验收

1. 新建 hourly 自动化（现有工作区模式）→ 到点后后台 agent 启动并执行提示词 → 运行历史出现且完成状态正确；
2. `Run Now` 手动触发（含 disabled 记录）；
3. precheck 配 `exit 1` → `skipped_precheck` 且历史可见 stdout/stderr 尾；
4. `new_per_run` 模式 → 生成 `auto-<slug>-<stamp>` 工作区且带 provenance；
5. 应用重启 → 定义/历史/`nextRunAt` 保留，tick 继续；
6. 将 grace 设小 + 暂停应用超过 grace → 重启后 `skipped_missed` 一条。

### 6.4 门禁

`cargo test --workspace`、`pnpm typecheck && pnpm build:web`、`pnpm test`。

## 7. 风险与偏差备案

1. **时区=host-local**：对齐参照（timezone 字段仅存储）；跨时区语义偏差在记录中披露。
2. **无 live-run watchdog**：运行中强杀应用 → 重启后对账 stranded；窗口期内无恢复（参照桌面同款）。
3. **不做项**（§2.2）：external/SSH 多宿主/owner fencing/usage/CLI/headless serve/skip 折叠/`automation.show`/上下文迁移。
4. **skip 折叠**：同一拒绝连续出现各写一条（参照会折叠为 `occurrenceCount`）；历史噪声略高（披露，后续补）。
5. **新增依赖 `chrono`**（`clock` feature）：本地时区/DST 数学的标准工具；无其它新依赖。
6. **dispatch token**：30 分钟 TTL、内存态；应用重启即失效（渲染层未消费 token 校验时无影响；provenance 消费见 `automationProvenanceRequest` 既有链路）。
7. **tick 依赖渲染层武装**：无 headless 调度；窗口关闭即无 tick（桌面单窗口语义）。
8. **fenced 参数忽略**：多宿主数据（selector/expectedOwner）被静默忽略；单机语义正确，未来接多宿主时需回填。
9. **worktree provenance token 不在 Rust 校验**：`new_per_run` 的 `automationProvenanceRequest` 经既有工作区创建链路落 metadata；参照版在 main 端用 dispatch token 校验归属，本切片不校验（渲染层自证 + 单机信任边界），披露。
