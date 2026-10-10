# Phase 2 子项目 E：automations（本地定时自动化）Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 交付本地定时自动化最小闭环：Rust 内核（存储/schedule 数学/60s tick/precheck/对账）+ 类型化命令与事件 + TS local RPC 拦截与 real 域接线；渲染层既有 dispatch/完成观察链路零改动。

**Architecture:** 新 crate `ade-automations` 承载全部内核；`ade-bridge` 加 10 个薄命令与两个 Tauri 事件；`runtime-rpc-client` local 分支拦截 `automation.*` 映射到命令（延续 `local-terminal-rpc` 先例）；`real/automations.ts` 接真 9 方法。参照实现 `/Users/itsuka/CodeSpace/orca`（只读），`orca:` 前缀均为其内路径。

**Tech Stack:** Rust（ade-automations、chrono、serde_json）、TypeScript（Vitest、Tauri IPC）。

**Spec:** `docs/superpowers/specs/2026-10-09-automations-design.md`（执行者须同时阅读）

## Global Constraints

- Rust 依赖：新 crate 引入 `chrono = { version = "0.4", default-features = false, features = ["clock", "std"] }`（唯一新外部依赖）与 `libc = "0.2.189"`（workspace 既有版本，供进程组杀）；不新增任何 npm 依赖。
- bindings 唯一生成方式：`cargo run -p ade-bridge --bin export-bindings`（workdir `src-tauri`）；`bindings_are_fresh` 校验；新命令名进 `specta_export.rs` 两处清单。
- 提交信息中文 conventional + `Co-Authored-By: Claude Code <noreply@anthropic.com>`。
- cargo 命令在 `src-tauri` 下执行；pnpm 在仓库根。
- 仅 macOS/POSIX；可选字段省略不物化 null。
- 参照逐字：run 状态集、missed/stranded 固定文案、precheck 截断/超时语义、schedule 数学语义；渲染层状态机不动。
- fenced 参数（selector/expectedOwner/destination）接收后忽略，不报错。

---

### Task 1: crate 骨架 + schedule 数学

**Files:**
- Create: `src-tauri/crates/ade-automations/Cargo.toml`
- Create: `src-tauri/crates/ade-automations/src/lib.rs`
- Create: `src-tauri/crates/ade-automations/src/schedule.rs`
- Test: 同 crate 内 `#[cfg(test)]` 模块（沿 ade-hooks 惯例）

**Interfaces:**
- Produces:
  - `pub fn is_valid_schedule(text: &str) -> bool`
  - `pub fn next_occurrence_after(text: &str, dtstart_ms: i64, after_ms: i64) -> Option<i64>`
  - `pub fn latest_occurrence_at_or_before(text: &str, dtstart_ms: i64, now_ms: i64) -> Option<i64>`
  - `pub enum ScheduleError { Invalid, NoOccurrence }`（或等价）

- [x] **Step 1: 创建 crate**

`Cargo.toml`（镜像 ade-hooks）：

```toml
[package]
name = "ade-automations"
version = "0.0.1"
edition = "2021"

[dependencies]
ade-core = { path = "../ade-core" }
chrono = { version = "0.4", default-features = false, features = ["clock", "std"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
thiserror = "2"
```

`lib.rs`：`pub mod schedule;`（后续任务追加模块）。workspace 自动收编（members = `crates/*`）。

- [x] **Step 2: 写失败测试**

`schedule.rs` 测试模块，对齐 `src/shared/automation-schedules.test.ts` 场景（实现时逐条对应）：

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn local_ms(y: i32, mo: u32, d: u32, h: u32, mi: u32) -> i64 {
        use chrono::{Local, TimeZone};
        Local.with_ymd_and_hms(y, mo, d, h, mi, 0).single().unwrap().timestamp_millis()
    }

    #[test]
    fn cron_weekday_schedule_never_returns_weekend() {
        // 9:30 on weekdays: "30 9 * * 1-5"
        let dtstart = local_ms(2026, 10, 5, 0, 0); // Monday
        let mut cursor = dtstart;
        for _ in 0..7 {
            cursor = next_occurrence_after("30 9 * * 1-5", dtstart, cursor).unwrap();
            let weekday = chrono::Local.timestamp_millis_opt(cursor).unwrap().weekday();
            assert!(weekday.num_days_from_monday() < 5, "weekend candidate: {cursor}");
        }
    }

    #[test]
    fn hourly_rrule_respects_minute_and_dtstart() {
        // FREQ=HOURLY;BYMINUTE=15
        let dtstart = local_ms(2026, 10, 9, 10, 20);
        let next = next_occurrence_after("FREQ=HOURLY;BYMINUTE=15", dtstart, dtstart).unwrap();
        assert_eq!(next, local_ms(2026, 10, 9, 11, 15));
    }

    #[test]
    fn weekly_sunday_round_trips() {
        // FREQ=WEEKLY;BYDAY=SU;BYHOUR=8;BYMINUTE=0
        let dtstart = local_ms(2026, 10, 9, 0, 0); // Friday
        let next = next_occurrence_after("FREQ=WEEKLY;BYDAY=SU;BYHOUR=8;BYMINUTE=0", dtstart, dtstart).unwrap();
        use chrono::{Datelike, Timelike, TimeZone};
        let dt = chrono::Local.timestamp_millis_opt(next).unwrap();
        assert_eq!(dt.weekday(), chrono::Weekday::Sun);
        assert_eq!((dt.hour(), dt.minute()), (8, 0));
    }

    #[test]
    fn invalid_schedule_rejected() {
        assert!(!is_valid_schedule("not a schedule"));
        assert!(!is_valid_schedule("61 9 * * *"));
        assert!(!is_valid_schedule("FREQ=SECONDLY;BYMINUTE=0"));
    }

    #[test]
    fn impossible_cron_rejected() {
        // Feb 31 never occurs.
        assert!(!is_valid_schedule("0 0 31 2 *"));
    }

    #[test]
    fn cron_length_cap() {
        let long = format!("0 0 * * *{}", " ".repeat(2100));
        assert!(!is_valid_schedule(&long));
    }

    #[test]
    fn latest_occurrence_within_day_and_grace_math() {
        let now = local_ms(2026, 10, 9, 12, 0);
        let dtstart = local_ms(2026, 10, 1, 0, 0);
        let latest = latest_occurrence_at_or_before("0 9 * * *", dtstart, now).unwrap();
        assert_eq!(latest, local_ms(2026, 10, 9, 9, 0));
        assert_eq!(now - latest, 3 * 60 * 60 * 1000);
    }
}
```

（勿固定成列出的示例文本即可；执行者按 TS 测试补足场景：闰日、DOM/DOW OR/AND、off-minute dtstart、malformed BYDAY。）

- [x] **Step 3: 跑测试确认失败**

Run（workdir src-tauri）: `cargo test -p ade-automations`
Expected: 编译失败（模块不存在）。

- [x] **Step 4: 实现**

逐字语义移植（来源：`src/shared/automation-schedule-parsing.ts`、`automation-schedule-occurrences.ts`、`automation-cron-occurrence.ts`；只看语义与常量，不照搬 JS 结构）：

- 判定：文本含 `=` → RRULE（仅 `FREQ=HOURLY|DAILY|WEEKLY` + `BYDAY`/`BYHOUR`/`BYMINUTE`；未知键拒绝）；否则 5 字段 cron（`*`、逗号列表、范围、`/` 步进、月/周英文名、`7→0` 归一；字段数≠5 拒绝）。
- cron 语义：DOM 与 DOW 双限 → OR；否则 AND；文本 ≤ 2048 字节；`is_valid_schedule` 需在 9×366 天内至少一个 occurrence（用 `next_occurrence_after` 探测）。
- 时间：**本地墙钟**（chrono `Local`）；扫描上限 9×366 天 / 370 天（对齐 TS 常量）；`from_local` 对不存在时刻取当日后继合法时刻、对歧义时刻取最早实例（在模块文档注释固定该选择）。
- `next_occurrence_after`：`base = max(dtstart, after)` 对齐分钟后 +1 分钟起扫；`latest_...`：自 `now` 对齐分钟后向前扫；无解 → None。

- [x] **Step 5: 跑测试确认通过并提交**

Run: `cargo test -p ade-automations`

```bash
git add src-tauri/crates/ade-automations src-tauri/Cargo.lock
git commit -m "feat(automations): ade-automations crate 与 schedule 数学（cron/RRULE）"
```

---

### Task 2: `AutomationStore`

**Files:**
- Create: `src-tauri/crates/ade-automations/src/store.rs`
- Modify: `src-tauri/crates/ade-automations/src/lib.rs`
- Test: 同文件 `#[cfg(test)]`

**Interfaces:**
- Consumes: `ade_core::ids::new_uuid`；`chrono`（时间戳）。
- Produces（签名可微调，语义固定）：

```rust
pub struct AutomationStore { /* path + Mutex<Inner> */ }
pub struct RunsPage { pub runs: Vec<serde_json::Value>, pub next_cursor: Option<String> }
impl AutomationStore {
    pub fn load(path: std::path::PathBuf) -> Self;
    pub fn list(&self) -> Vec<serde_json::Value>;
    pub fn get(&self, id: &str) -> Option<serde_json::Value>;
    pub fn create(&self, input: serde_json::Value, now_ms: i64) -> Result<serde_json::Value, StoreError>;
    pub fn update(&self, id: &str, updates: serde_json::Value, now_ms: i64) -> Result<Option<serde_json::Value>, StoreError>;
    pub fn delete(&self, id: &str) -> Result<bool, StoreError>;
    pub fn create_run(&self, run: serde_json::Value) -> Result<(serde_json::Value, bool), StoreError>;
    pub fn update_run(&self, run_id: &str, fields: serde_json::Value) -> Result<Option<serde_json::Value>, StoreError>;
    pub fn runs_page(&self, automation_id: Option<&str>, limit: usize, cursor: Option<&str>) -> RunsPage;
    pub fn prune_runs(&self, automation_id: &str);
    pub fn non_final_runs_before(&self, cutoff_ms: i64) -> Vec<serde_json::Value>;
    pub fn snapshot_workspace_name(&self, workspace_id: &str, display_name: &str) -> usize;
}
```

- [x] **Step 1: 写失败测试**

覆盖（fake 时钟传入 `now_ms`）：
1. create 自动生成 `id`/`createdAt`/`updatedAt`/`schedulerOwner:'local_host_service'`/`missedRunPolicy:'run_once_within_grace'`/`enabled` 默认 true/`nextRunAt` 由 service 提供（store 不计算；create 入参须带）；
2. `creationKey` 幂等：同 key 二次 create 返回原记录、`created=false`；
3. update 合并 + `updatedAt` 刷新 + `nextRunAt` 若 schedule 变化由调用方传入；
4. delete 级联删除该 automations 的 runs；
5. `create_run` 去重 `(automationId, scheduledFor)`；`runNumber` 单调回填（裁剪后不重复）；
6. `update_run` 终态守卫：`completed`/`dispatch_failed` 等终态不被覆盖；非终态合并；
7. `runs_page`：`createdAt desc, id asc`、limit 上限 100、`nextCursor` 稳定性（翻页窗口不漂移）；
8. `prune_runs`：仅终态可逐、上限 100、in-flight 永不逐；
9. 损坏文件：`load` 时备份 `.corrupt-<ts>` 并重建空 store；
10. `snapshot_workspace_name`：仅更新匹配 workspaceId 的运行（新→旧首条），返回更新条数。

- [x] **Step 2: 跑测试确认失败**

Run: `cargo test -p ade-automations store`

- [x] **Step 3: 实现**

- 文件形态 `{ "schemaVersion": 1, "automations": [...], "runs": [...] }`；原子写（tmp + rename，目录 fsync 可省）；每次变更整写（文件有界：定义数小 + 100×runs）。
- 记录为 `serde_json::Value`（opaque；Rust 只读取调度字段 `id/enabled/rrule/dtstart/nextRunAt/missedRunGraceMinutes` 与 run 的 `id/automationId/scheduledFor/status/createdAt`）。
- 分页游标：`<createdAt>:<id>` base64 或明文（实现自定，测试锚定稳定性）。
- 保留常量 `MAX_RUNS_PER_AUTOMATION: usize = 100`（对齐 `shared/automation-run-retention.ts:3`）。

- [x] **Step 4: 跑测试 + 提交**

Run: `cargo test -p ade-automations`

```bash
git add src-tauri/crates/ade-automations
git commit -m "feat(automations): AutomationStore（CRUD/去重/保留/分页/损坏隔离）"
```

---

### Task 3: precheck 执行器

**Files:**
- Create: `src-tauri/crates/ade-automations/src/precheck.rs`
- Modify: `src-tauri/crates/ade-automations/src/lib.rs`

**Interfaces:**
- Produces: `pub fn run_precheck(command: &str, cwd: Option<&str>, timeout_seconds: Option<u64>) -> serde_json::Value`（返回 `AutomationPrecheckResult` JSON：`command/exitCode/timedOut/durationMs/stdout/stderr/stdoutTruncated/stderrTruncated/error/startedAt/completedAt`）。

- [x] **Step 1: 写失败测试**

1. 成功：`echo hello` → exit 0、stdout 含 hello；
2. 非零：`exit 3` → exitCode 3；
3. 超时：`sleep 30 & sleep 30` + `timeout_seconds: 1` → `timedOut: true`，且孙进程被杀（脚本把孙 PID 写文件，轮询 `libc::kill(pid,0)` 消亡）——`libc` 加到该 crate 依赖（与 ade-pty 同版本）；
4. 截断：输出 > 4000 字符 → 尾 4000 + `stdoutTruncated: true`（stderr 同理）；
5. 默认/上限：`None` → 60s；`Some(9999)` → clamp 600（用极短超时测试只验证 clamp 函数，不真等）。

- [x] **Step 2: 跑测试确认失败 → 实现**

实现要点：`Command::new("sh").arg("-c").arg(command)`；`cwd` 存在则设置；`process_group(0)`；stdout/stderr piped，读取线程收集：内存保留上限 1MiB（超出即丢弃最旧缓冲、始终保留尾部），完成后各取最后 4000 字符并置 `*Truncated`；超时 `libc::kill(-pid, SIGTERM)` → 2s → `SIGKILL`；`exitCode=status.code()`；`error` 填 spawn/IO 错误文案；`startedAt/completedAt` 毫秒。

- [x] **Step 3: 提交**

```bash
git add src-tauri/crates/ade-automations
git commit -m "feat(automations): precheck 执行器（超时组杀/尾截断）"
```

---

### Task 4: `AutomationService`（tick/grace/advance/对账）

**Files:**
- Create: `src-tauri/crates/ade-automations/src/service.rs`
- Modify: `src-tauri/crates/ade-automations/src/lib.rs`

**Interfaces:**
- Produces：

```rust
pub trait EventSink: Send + Sync {
    fn automations_changed(&self, reason: &str);
    fn dispatch_requested(&self, payload: serde_json::Value);
}
pub struct AutomationService { /* store, sink, now: fn, started_at, armed_at, state */ }
impl AutomationService {
    pub fn new(store: Arc<AutomationStore>, sink: Arc<dyn EventSink>, now: fn() -> i64) -> Arc<Self>; // now 可注入
    pub fn start(self: &Arc<Self>);            // spawn interval 60s（测试直接调 tick）
    pub fn renderer_ready(&self);              // armed=true + 立即 tick
    pub fn tick(&self);                        // 幂等/重入保护；测试直调
    pub fn mark_dispatch_result(&self, run_id: &str, fields: serde_json::Value) -> Option<serde_json::Value>;
    pub fn run_precheck_for(&self, automation_id: &str, run_id: &str) -> Option<serde_json::Value>; // 读定义 precheck 执行并落盘
    pub fn snapshot_workspace_name(&self, workspace_id: &str, display_name: &str) -> usize;
}
```

- [x] **Step 1: 写失败测试（FakeSink + 临时 store + 注入时钟）**

1. 未 armed：tick 不派发、不写 `dispatching`；
2. armed 后 due：`nextRunAt<=now` → 创建 run（`status:'dispatching'`、`trigger:'scheduled'`、`title:"<name> run <n>"`）→ 发一次 `dispatch_requested`（payload 含 `automation/run/dispatchToken`）→ `nextRunAt` 前移（`next_occurrence_after(now)`，无回填）；
3. grace：`now - latest > grace×60_000` → 一条 `skipped_missed`（`error: "Orca was unavailable during the missed-run grace window."` 逐字）+ advance，不派发；
4. 去重：同 `(automationId, scheduledFor)` 的第二次 tick 不再产生新 run/事件；
5. `runNow` 语义经 store 单独测（不受 enabled 限制）；
6. 终态守卫：`mark_dispatch_result` 覆盖 `dispatched→completed`，但不覆盖已终态；
7. 对账：service 启动前存在非终态 run → `renderer_ready` 后首 tick 不立即杀（结算窗内），注入时钟推进 2 分钟后 tick → `dispatch_failed` + 固定 stranded 文案（逐字移植 `orca:src/main/automations/run-completion-watcher.ts:26-31` 的句子）；
8. 重入：tick 内再入直接返回。

- [x] **Step 2: 实现**

要点：`armed: AtomicBool`；`evaluating: Mutex<()>`（try_lock 失败即返回）；`started_at = now()`；`armed_at: Mutex<Option<i64>>`；tick 顺序 = 对账（仅 armed 后首次且 `now-armed_at>=120_000` 时执行一次，`reconciled` 旗标）→ 到期评估（遍历 `store.list()`，读取调度字段；`enabled==true` 且 `nextRunAt<=now`）。`dispatchToken`：`new_uuid()`；不持久化（内存 Set + 30 分钟惰性清理）。每次写入后 `sink.automations_changed("run"/"definition")`。测试的可控时钟：测试文件用 `static NOW_MS: AtomicI64`，`now` 传入读取该静态量的自由函数。

- [x] **Step 3: 提交**

```bash
git add src-tauri/crates/ade-automations
git commit -m "feat(automations): AutomationService（tick/grace/advance/对账）"
```

---

### Task 5: ade-bridge 命令 + 事件 + bindings

**Files:**
- Create: `src-tauri/crates/ade-bridge/src/commands/automations.rs`
- Modify: `src-tauri/crates/ade-bridge/src/commands/mod.rs`
- Modify: `src-tauri/crates/ade-bridge/src/state.rs`（AppState：`automations: Arc<AutomationStore>`、`automation_service: Arc<AutomationService>`、初始化装配 + `ServiceEventSink`）
- Modify: `src-tauri/crates/ade-bridge/src/events.rs`（常量 + `emit_automations_changed`/`emit_automation_dispatch_requested`）
- Modify: `src-tauri/crates/ade-bridge/Cargo.toml`（`ade-automations = { path = "../ade-automations" }`）
- Modify: `src-tauri/crates/ade-bridge/src/specta_export.rs`
- Test: `src-tauri/crates/ade-bridge/tests/automations.rs`
- Regenerate: `src/bridge/real/generated/tauri-bindings.ts`

**Interfaces:**
- Consumes: Task 1–4 crate API；`emit_json`（`events.rs:51`）；`new_uuid`。
- Produces：命令
  `automations_list() -> Vec<Value>`、
  `automations_create({input}) -> {automation}`（schedule 校验失败 → BridgeError）、
  `automations_update({id, updates}) -> {automation|null}`、
  `automations_delete({id}) -> {removed: bool}`、
  `automations_run_now({id}) -> {run}`、
  `automations_runs({automationId?, limit?, cursor?}) -> {runs, nextCursor}`、
  `automations_run_precheck({automationId, runId}) -> {result|null}`、
  `automations_mark_dispatch_result({runId, result}) -> {run|null}`、
  `automations_snapshot_workspace_name({workspaceId, displayName}) -> {updated: number}`、
  `automations_renderer_ready() -> null`。

- [x] **Step 1: 写失败测试（tests/automations.rs，state-free 层）**

用临时 store + 直接调用 `*_impl`（沿 `git_commands.rs` 风格）：
create（合法 rrule 通过 / 非法拒绝且不落盘）、list、update、delete 级联、run_now（disabled 也创建）、runs 分页、mark_dispatch_result 终态守卫、run_precheck 落盘（`echo ok`）。

- [x] **Step 2: 实现**

- `ServiceEventSink` 实现 `EventSink`：持有 `AppHandle`，`automations_changed` → `emit_json(app, "automations:changed", json!({"reason": reason}))`；`dispatch_requested` → `emit_json(app, "automations:dispatch-requested", payload)`。
- `state.rs::AppState::initialize`：`AutomationStore::load(app_data.join("automations.json"))` → `AutomationService::new(store, sink, now)`，并**在 initialize 内调用 `service.start()`**（60s tick 先起、`armed` 等待 `renderer_ready`——对齐参照「服务先起、武装等渲染层」）。
- `automations_create`：以 crate 提供的 `validate_definition(input)`（Task 1 的 `is_valid_schedule` + 字段校验，放 service 或 store 模块）先行校验；`nextRunAt` 由命令用 `next_occurrence_after(rrule, dtstart, now)` 计算后写入。
- specta 注册 + 名称清单（10 命令）；bindings 重生成。

- [x] **Step 3: 跑测试 + bindings**

Run: `cargo test -p ade-bridge --test automations`
Run: `cargo run -p ade-bridge --bin export-bindings && cargo test -p ade-bridge`

- [x] **Step 4: 提交**

```bash
git add src-tauri/crates/ade-bridge src/bridge/real/generated/tauri-bindings.ts
git commit -m "feat(bridge): automations 命令面与 changed/dispatch-requested 事件"
```

---

### Task 6: TS local-automation-rpc 拦截

**Files:**
- Create: `src/renderer/src/runtime/local-automation-rpc.ts`
- Test: `src/renderer/src/runtime/local-automation-rpc.test.ts`
- Modify: `src/renderer/src/runtime/runtime-rpc-client.ts`
- Test: `src/renderer/src/runtime/runtime-rpc-client.test.ts`

**Interfaces:**
- Consumes: Task 5 命令（`invokeCommand`）。
- Produces: `isLocalAutomationRpcMethod(method)`；`callLocalAutomationRpc<T>(method, params)`。

- [x] **Step 1: 写失败测试**

- method 集 = `automation.list/create/update/delete/runNow/runs`（其余 false）；
- `automation.list`：接收 `{}` 或 `{selector}`（忽略）→ invoke `automations_list` → 返回 `{automations, items}`（items 构造 `{automationId, selector:{kind:'self'}}`；如 Rust 直接返回同形则透传）；
- `automation.create`：入参 wire 重命名（读 `automation-host-client.ts:80-103` 与 `automation-host-client.test.ts:150-156` 确定准确形状）——`repo:"id:X"` → `projectId:"X"`、`workspace:"id:Y"` → `workspaceId:"Y"`、剥离 `destination` → invoke `automations_create {input}`；
- `automation.update`：剥离 `expectedOwner`/`destination` → `automations_update {id, updates}`；
- `automation.delete`/`runNow`：剥离 fencing → 对应命令；
- `automation.runs`：`{automationId, limit, cursor}`（剥离 `expectedOwner`）→ `automations_runs` → `{runs, nextCursor}`；
- runtime-rpc-client 新增路由测试：local + `automation.list` 不进 `window.api.runtime.call`；非 automation 方法行为不变。

- [x] **Step 2: 实现**

`runtime-rpc-client.ts` local 分支追加（在 terminal 拦截旁）：

```ts
if (target.kind === 'local' && isLocalAutomationRpcMethod(method)) {
  return await callLocalAutomationRpc<TResult>(method, nextParams)
}
```

映射细节以既有客户端测试锚定（见上）；fencing 字段**剥离而非透传**。

- [x] **Step 3: 跑测试 + typecheck + 提交**

Run: `pnpm vitest run src/renderer/src/runtime/local-automation-rpc.test.ts src/renderer/src/runtime/runtime-rpc-client.test.ts && pnpm typecheck`

```bash
git add src/renderer/src/runtime/local-automation-rpc.ts src/renderer/src/runtime/local-automation-rpc.test.ts \
  src/renderer/src/runtime/runtime-rpc-client.ts src/renderer/src/runtime/runtime-rpc-client.test.ts
git commit -m "feat(renderer): local automation RPC 拦截（CRUD 映射）"
```

---

### Task 7: `real/automations.ts` + create-api/parity + 全量

**Files:**
- Create: `src/bridge/real/automations.ts`
- Create: `src/bridge/real/automations.test.ts`
- Modify: `src/bridge/create-api.ts`
- Modify: `src/bridge/real/parity.test.ts`
- Modify: `src/bridge/create-api.test.ts`（如需）

**Interfaces:**
- Consumes: Task 5 命令；`subscribeToEvent`；`withMethodFallback`。
- Produces: `createAutomationsRealApi(): PreloadApi['automations']`。

- [x] **Step 1: 写失败测试**

- `runPrecheck` → `invokeCommand('automations_run_precheck', {args:{automationId, runId}})` 且返回 `{result}` 解包；
- `markDispatchResult` → `automations_mark_dispatch_result`，返回 `{run}` 解包；
- `snapshotWorkspaceName` → 命令；`rendererReady` → 命令；
- `onChanged`/`onDispatchRequested` → `subscribeToEvent('automations:changed'|'automations:dispatch-requested')` 转发；
- external 五方法仍 reject `UnimplementedBridgeError`。

- [x] **Step 2: 实现**

`real/automations.ts` 用 `withMethodFallback` 提供 9 个真方法；`create-api.ts` `RealDomains` 加 `'automations'` + 工厂；parity 增加 `automations` surfaceCase（explicit 9 / missing 5：external 五方法）。

- [x] **Step 3: 全量回归**

Run: `pnpm vitest run src/bridge && pnpm typecheck`
Run: `pnpm test`（全量；性能抖动隔离复跑；其它失败 STOP + BLOCKED）

- [x] **Step 4: 提交**

```bash
git add src/bridge/real/automations.ts src/bridge/real/automations.test.ts \
  src/bridge/create-api.ts src/bridge/real/parity.test.ts src/bridge/create-api.test.ts
git commit -m "feat(bridge): automations real 域接线与 parity"
```

---

### Task 8: 门禁复跑 + 收尾记录

**Files:**
- Create: `docs/phase2e-automations-record.md`
- Modify: `docs/superpowers/plans/2026-10-09-automations.md`（勾选复选框）

- [x] **Step 1: 全量门禁**

Run: `cargo test --workspace`（workdir src-tauri）
Run: `rm -f tsconfig.tsbuildinfo && pnpm typecheck && pnpm build:web`
Run: `pnpm test`

- [x] **Step 2: 写收尾记录**

结构照 `docs/phase2d2-create-pr-record.md`：§1 范围与验收 / §2 提交清单 / §3 门禁证据 / §4 手工验收（6 项，待用户复核）/ §5 偏差与边界备案（spec §7 九条 + 实现中发现的新偏差）/ §6 已知边界与后续（external managers、SSH 多宿主、owner fencing、usage 计费、CLI、headless serve、skip 折叠、live-run watchdog、`automation.show`）。

手工验收清单（写入记录）：创建 hourly 自动化到点执行；Run Now；precheck 失败 skipped_precheck；new_per_run 工作区；重启保留；超 grace skipped_missed。

- [x] **Step 3: 提交**

```bash
git add docs/phase2e-automations-record.md docs/superpowers/plans/2026-10-09-automations.md
git commit -m "docs: Phase 2E automations 实施记录与门禁证据"
```

---

## Self-Review

**Spec coverage:**
- §3.1 内核（store/schedule/service/precheck）→ Tasks 1–4；§3.2 命令/事件 → Task 5；§3.3 local RPC → Task 6；§2.1 real 域接线 → Task 7。
- §4 数据流 → Task 4 tick/对账 + Task 5 命令 + Task 7 事件测试；§5 错误处理 → Task 2（损坏/去重/保留）、Task 3（precheck）、Task 4（armed/grace/终态）。
- §6 测试与门禁 → 各任务 + Task 7 全量 + Task 8。
- §7 风险 → Task 8 记录。

**Placeholder scan:** Rust 关键签名/测试场景在计划内；schedule 数学与 store 细节附参照 file:line 与行为清单；TS 映射以既有客户端测试为准确锚。

**Type consistency:** 命令名 `automations_*`（10）在 Tasks 5/6/7 一致；`EventSink` 在 Task 4/5 一致；`run_read`/`mark_dispatch_result` 形状在 Tasks 2/4/5 一致；事件名 `automations:changed`/`automations:dispatch-requested` 在 Tasks 5/7 一致。
