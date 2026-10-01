use std::collections::HashMap;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::commands::run_blocking;
use crate::errors::BridgeError;
use crate::json::Json;
use crate::state::AppState;

/// PTY 数据面端点（规格 §4.7）：WS 环回服务的端口与一次性下发 token。
/// Task 9 起 `PtyHost::start` 接管（端口与 token 在进程生命周期内不变）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct DataEndpointPayload {
    pub port: u16,
    pub token: String,
}

/// `pty_spawn` 参数（对齐 `src/shared/preload-api/api/pty-api.ts` `spawn` opts
/// 的生效字段——规格 §2.4；契约-only 字段渲染层可能附带，serde 未知字段忽略）。
#[derive(Debug, Clone, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct PtySpawnArgs {
    pub cols: u16,
    pub rows: u16,
    #[serde(default)]
    pub cwd: Option<String>,
    /// 仅识别 `'worktree'`（规格 §2.4）：cwd 缺省时按 `worktreeId` 解析路径。
    #[serde(default)]
    pub cwd_fallback: Option<String>,
    #[serde(default)]
    pub env: HashMap<String, String>,
    #[serde(default)]
    pub env_to_delete: Vec<String>,
    /// spawn 后作为一行输入键入（`Session::spawn` 的命令交付语义）。
    #[serde(default)]
    pub command: Option<String>,
    #[serde(default)]
    pub shell_override: Option<String>,
    /// 命中在册活会话 → reattach：回 `{id, isReattach:true}`，不再 spawn。
    #[serde(default)]
    pub session_id: Option<String>,
    /// 成功 spawn 后记入 worktreeId 映射（`pty_list_sessions` 补列）。
    #[serde(default)]
    pub worktree_id: Option<String>,
}

/// `pty_spawn` 返回（规格 §2.1 最小合法响应；`isReattach` 仅 reattach 命中时
/// 为 `true`，新开 spawn 时整字段缺省）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct PtySpawnReply {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_reattach: Option<bool>,
}

/// spawn cwd 解析结果。
pub struct ResolvedSpawnCwd {
    /// 终值 cwd（规格 §2.4 解析链终点）。
    pub path: String,
    /// `cwdFallback === 'worktree'` 被请求但未命中（终值落在 home）——
    /// 调用侧据此记 warn（规格 §4.6）。
    pub worktree_fallback_missed: bool,
}

/// spawn cwd 解析（规格 §2.4）：`cwd` 优先（空串视为缺省）→
/// `cwdFallback === 'worktree'` 且 `worktreeId` 可解析（`id.split_once("::")`
/// 取 path 段 + `Path::is_dir` 校验）→ home。
pub fn resolve_spawn_cwd(
    cwd: Option<&str>,
    cwd_fallback: Option<&str>,
    worktree_id: Option<&str>,
    home: &str,
) -> ResolvedSpawnCwd {
    if let Some(cwd) = cwd.filter(|value| !value.is_empty()) {
        return ResolvedSpawnCwd {
            path: cwd.to_string(),
            worktree_fallback_missed: false,
        };
    }
    let worktree_fallback_missed = cwd_fallback == Some("worktree");
    if worktree_fallback_missed {
        if let Some((_, path)) = worktree_id.and_then(|id| id.split_once("::")) {
            if std::path::Path::new(path).is_dir() {
                return ResolvedSpawnCwd {
                    path: path.to_string(),
                    worktree_fallback_missed: false,
                };
            }
        }
    }
    ResolvedSpawnCwd {
        path: home.to_string(),
        worktree_fallback_missed,
    }
}

/// `pty_write` / `pty_write_accepted` 参数。
#[derive(Debug, Clone, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct PtyWriteArgs {
    pub id: String,
    pub data: String,
}

/// `pty_resize` 参数。
#[derive(Debug, Clone, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct PtyResizeArgs {
    pub id: String,
    pub cols: u16,
    pub rows: u16,
}

/// `pty_signal` 参数（信号名透传，`SIG` 前缀可选）。
#[derive(Debug, Clone, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct PtySignalArgs {
    pub id: String,
    pub signal: String,
}

/// 单 `id` 参数（clearBuffer/getCwd/getSize/hasPty/进程检查 stub 族共用）。
#[derive(Debug, Clone, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct PtyIdArgs {
    pub id: String,
}

/// `pty_kill` 参数：`keepHistory` 契约参数收下但忽略（规格 §2.1：kill 即
/// teardown，退出码走 `pty:exit` 事件）。
#[derive(Debug, Clone, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct PtyKillArgs {
    pub id: String,
    #[serde(default)]
    pub keep_history: Option<bool>,
}

/// `pty_get_size` 返回；未知会话为 `null`（TS `getSize` 契约）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct PtySizeReply {
    pub cols: u16,
    pub rows: u16,
}

/// `pty_list_sessions` 行（对齐 `src/shared/pty-listed-session.ts` 的
/// `PtyListedSession`）：host 三元组 + bridge 补 `worktreeId`/`title`。
#[derive(Debug, Clone, PartialEq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct PtyListedSessionRow {
    pub id: String,
    pub cwd: String,
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub worktree_id: Option<String>,
    pub agent_ownership: String,
}

/// `pty_list_sessions` 参数：`PtySessionListScope` 契约参数收下但忽略
/// （本地 provider 恒全量）。
#[derive(Debug, Clone, Default, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct PtyListSessionsArgs {
    #[serde(default)]
    pub scope: Option<Json>,
}

/// `pty_get_main_buffer_snapshot` 参数：scrollback 行数契约参数收下但忽略
/// （快照机械属 Phase 2+，规格 §2.5）。
#[derive(Debug, Clone, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct PtyGetMainBufferSnapshotArgs {
    pub id: String,
    #[serde(default)]
    pub scrollback_rows: Option<u32>,
}

/// `pty_inspect_process` 参数：id 之外的选项契约收下但忽略（§2.2 stub）。
#[derive(Debug, Clone, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct PtyInspectProcessArgs {
    pub id: String,
    #[serde(default)]
    pub expected_incarnation_id: Option<String>,
    #[serde(default)]
    pub scan_child_processes: Option<bool>,
    #[serde(default)]
    pub steady_state: Option<bool>,
}

/// `pty_get_authoritative_buffer_snapshot_capabilities` 参数。
#[derive(Debug, Clone, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct PtySnapshotCapabilitiesArgs {
    pub ids: Vec<String>,
}

/// 逐 id 能力行（§2.2：取 `authoritative:false` 对齐 web stub；规格允许
/// null/false 任一）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct PtySnapshotCapability {
    pub id: String,
    pub authoritative: bool,
}

/// `pty_report_renderer_delivery_state` 参数：渲染层报告整包收下但忽略
/// （本侧无投递机械，§2.2）。
#[derive(Debug, Clone, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct PtyReportRendererDeliveryStateArgs {
    pub report: Json,
}

/// `pty_report_renderer_delivery_state` 回复：零 in-flight 让 watchdog 保持
/// idle（§2.2，对齐 web stub）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct PtyRendererDeliveryHealthReply {
    pub in_flight_total_chars: u64,
    pub in_flight_pty_count: u32,
    /// null = 自主侧计数（重）建以来无 ACK。
    pub ms_since_last_ack: Option<u64>,
}

/// §2.2 的 diagnostics 零对象（`EMPTY_PTY_MAIN_DELIVERY_DIAGNOSTICS` 逐字）。
#[derive(Debug, Clone, PartialEq, Default, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct PtyMainDeliveryDiagnostics {
    pub app_version: String,
    pub main_uptime_ms: u64,
    pub window_focused: Option<bool>,
    pub window_visible: Option<bool>,
    pub window_minimized: Option<bool>,
    pub ms_since_last_power_suspend: Option<u64>,
    pub ms_since_last_power_resume: Option<u64>,
    pub per_pty: Vec<PtyPerPtyDeliveryDiagnostics>,
    pub breadcrumbs: Vec<PtyDeliveryBreadcrumb>,
}

/// `PtyPerPtyDeliveryDiagnostics`（`src/shared/pty-delivery-diagnostics.ts`）。
#[derive(Debug, Clone, PartialEq, Default, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct PtyPerPtyDeliveryDiagnostics {
    pub id: String,
    pub sent_chars: u64,
    pub acked_chars: u64,
    pub in_flight_chars: u64,
    pub pending_chars: u64,
    pub hidden: bool,
    pub visible: bool,
    pub active: bool,
    pub ms_since_last_send: Option<u64>,
    pub ms_since_last_ack: Option<u64>,
}

/// `PtyDeliveryBreadcrumb`（`src/shared/pty-delivery-diagnostics.ts`）。
#[derive(Debug, Clone, PartialEq, Default, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct PtyDeliveryBreadcrumb {
    pub at_ms: u64,
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<HashMap<String, Json>>,
    /// Same-kind events within the coalesce window fold into this counter.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repeats: Option<u32>,
}

/// `pty_get_renderer_delivery_debug_snapshot` 返回：web stub 零对象逐字
/// （`web-terminal-api.ts:51-78`）。
#[derive(Debug, Clone, PartialEq, Default, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct PtyRendererDeliveryDebugSnapshot {
    pub pending_pty_count: u32,
    pub pending_chars: u64,
    pub max_pending_chars_by_pty: u64,
    pub renderer_in_flight_pty_count: u32,
    pub renderer_in_flight_chars: u64,
    pub max_renderer_in_flight_chars_by_pty: u64,
    pub active_renderer_pty_count: u32,
    pub flush_scheduled: bool,
    pub peak_pending_chars: u64,
    pub peak_max_pending_chars_by_pty: u64,
    pub peak_renderer_in_flight_chars: u64,
    pub peak_max_renderer_in_flight_chars_by_pty: u64,
    pub ack_gated_flush_skip_count: u64,
    pub hidden_delivery_gated_pty_count: u32,
    pub hidden_delivery_gated_visible_pty_count: u32,
    pub hidden_delivery_gated_active_pty_count: u32,
    pub delivery_interest_pty_count: u32,
    pub hidden_delivery_dropped_chars: u64,
    pub hidden_delivery_dropped_chunks: u64,
    pub pending_dropped_chars: u64,
    pub diagnostics: PtyMainDeliveryDiagnostics,
    pub renderer_lifecycle_reset_count: u32,
    pub last_lifecycle_reset_cleared_chars: u64,
    pub renderer_pty_dispatcher_ready: bool,
    pub renderer_dispatcher_ready_forced_count: u32,
}

/// management 行的 `state`：在册即活 → 恒 `running`。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, specta::Type)]
pub enum PtyManagementSessionState {
    #[serde(rename = "running")]
    Running,
}

/// management 行的 `shellState`：shell-ready 协议未实现 → 恒 `unsupported`。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, specta::Type)]
pub enum PtyManagementShellState {
    #[serde(rename = "unsupported")]
    Unsupported,
}

/// management `listSessions` 行（对齐 `pty-management-api.ts` 的
/// `PtyManagementSession`，即 daemon `DaemonSessionInfo` 的 preload 镜像；
/// 本侧无 daemon——pid/createdAt/protocolVersion 无源，恒空/零值）。
#[derive(Debug, Clone, PartialEq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct PtyManagementSessionRow {
    pub session_id: String,
    pub state: PtyManagementSessionState,
    pub shell_state: PtyManagementShellState,
    pub is_alive: bool,
    pub pid: Option<u32>,
    pub cwd: Option<String>,
    pub cols: u16,
    pub rows: u16,
    pub created_at: u64,
    pub protocol_version: u32,
}

/// management `listSessions` 回复：`degraded` 恒 `false`（本侧总能本地
/// spawn，无「daemon 活但不可 spawn」语义）。
#[derive(Debug, Clone, PartialEq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct PtyManagementListReply {
    pub degraded: bool,
    pub sessions: Vec<PtyManagementSessionRow>,
}

/// management `killAll` 回复（规格 §2.1：逐会话 kill 并聚合）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct PtyManagementKillAllReply {
    pub killed_count: u32,
    pub remaining_count: u32,
    pub killed_session_ids: Vec<String>,
}

/// `pty_management_kill_one` 参数。
#[derive(Debug, Clone, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct PtyManagementKillOneArgs {
    pub session_id: String,
}

/// management killOne/restart 共用的 `{success}` 回复。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, specta::Type)]
pub struct PtyManagementOpReply {
    pub success: bool,
}

/// `macTccAttribution` 的 `health` 值：本侧无 daemon pid 记录可查 → 恒
/// `'unknown'`（横幅不显示，web stub 同语义）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, specta::Type)]
pub enum PtyManagementMacTccHealth {
    #[serde(rename = "unknown")]
    Unknown,
}

/// management `macTccAttribution` 回复（对齐 `pty-management-api.ts` 的
/// `{ health }` 包裹契约）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct PtyManagementMacTccAttributionReply {
    pub health: PtyManagementMacTccHealth,
}

#[tauri::command]
#[specta::specta]
pub fn pty_data_endpoint(
    state: tauri::State<'_, crate::state::AppState>,
) -> Result<DataEndpointPayload, BridgeError> {
    Ok(state.pty_data_endpoint())
}

// ===== 会话控制（真实走 PtyHost，规格 §2.1） =====

/// 下发 WS 数据面端点（`src/bridge/real/pty-socket.ts` 的
/// `fetchPtyDataEndpoint` 消费）。`PtyHost` 装配即起服务，恒 `Ok`
/// （渲染层缓存成功结果）。
#[tauri::command]
#[specta::specta]
pub async fn pty_spawn(
    state: tauri::State<'_, AppState>,
    args: PtySpawnArgs,
) -> Result<PtySpawnReply, BridgeError> {
    // reattach 判定（webview reload 后 tab 重挂路径）：sessionId 命中在册
    // 活会话即原样返回，不再 spawn（规格 §2.1）。纯注册表锁，异步直查。
    if let Some(session_id) = args.session_id.as_deref() {
        if state.pty_host.is_alive(session_id) {
            return Ok(PtySpawnReply {
                id: session_id.to_string(),
                is_reattach: Some(true),
            });
        }
    }
    let worktree_id = args.worktree_id.clone();
    let host = Arc::clone(&state.pty_host);
    let resolved_cwd = resolve_spawn_cwd(
        args.cwd.as_deref(),
        args.cwd_fallback.as_deref(),
        args.worktree_id.as_deref(),
        &state.home,
    );
    if resolved_cwd.worktree_fallback_missed {
        // 规格 §4.6：worktree cwd 解析失败记录 warn（bridge 层 eprintln 惯例）。
        eprintln!(
            "[ade-bridge] pty_spawn: cwdFallback 'worktree' unresolved (worktreeId: {:?}), falling back to home",
            args.worktree_id
        );
    }
    let request = ade_pty::SpawnRequest {
        cols: args.cols,
        rows: args.rows,
        cwd: Some(resolved_cwd.path),
        env: args.env,
        env_to_delete: args.env_to_delete,
        command: args.command,
        shell_override: args.shell_override,
    };
    let id = run_blocking(move || host.spawn(request).map_err(BridgeError::from)).await?;
    // worktreeId 映射仅当会话仍在册才记：即时退出会话的 Exit 可能先于本
    // 返回到达并已清场（Task 9 交接），不得残留孤儿条目。
    if let Some(worktree_id) = worktree_id {
        if state.pty_host.is_alive(&id) {
            state.pty_worktree_ids.record(&id, worktree_id);
        }
    }
    Ok(PtySpawnReply {
        id,
        is_reattach: None,
    })
}

/// 上行直写 master（阻塞写，等效键入；会话已 kill 时静默丢弃）。
#[tauri::command]
#[specta::specta]
pub async fn pty_write(
    state: tauri::State<'_, AppState>,
    args: PtyWriteArgs,
) -> Result<(), BridgeError> {
    let host = Arc::clone(&state.pty_host);
    run_blocking(move || {
        host.write(&args.id, args.data.into_bytes())
            .map_err(BridgeError::from)
    })
    .await
}

/// 非阻塞上行写：满即 `false`（背压显式化），通道关（写线程不在）→ Err。
#[tauri::command]
#[specta::specta]
pub async fn pty_write_accepted(
    state: tauri::State<'_, AppState>,
    args: PtyWriteArgs,
) -> Result<bool, BridgeError> {
    state
        .pty_host
        .write_accepted(&args.id, args.data.into_bytes())
        .map_err(BridgeError::from)
}

/// 写 PTY 尺寸（ConPTY reflow 依赖）。
#[tauri::command]
#[specta::specta]
pub async fn pty_resize(
    state: tauri::State<'_, AppState>,
    args: PtyResizeArgs,
) -> Result<(), BridgeError> {
    let host = Arc::clone(&state.pty_host);
    run_blocking(move || {
        host.resize(&args.id, args.cols, args.rows)
            .map_err(BridgeError::from)
    })
    .await
}

/// 信号透传（unix `libc::kill`，Windows 有限映射；不等待子进程）。
#[tauri::command]
#[specta::specta]
pub async fn pty_signal(
    state: tauri::State<'_, AppState>,
    args: PtySignalArgs,
) -> Result<(), BridgeError> {
    state
        .pty_host
        .signal(&args.id, &args.signal)
        .map_err(BridgeError::from)
}

/// 清空会话的 pre-attach 环形缓冲（首连重放语义保留）。
#[tauri::command]
#[specta::specta]
pub async fn pty_clear_buffer(
    state: tauri::State<'_, AppState>,
    args: PtyIdArgs,
) -> Result<(), BridgeError> {
    state
        .pty_host
        .clear_buffer(&args.id)
        .map_err(BridgeError::from)
}

/// kill 会话（`keepHistory` 忽略）。退出码由 `pty:exit` 事件携带，此处不回
/// 传；宿主线程不参与升级等待（PtyHost::kill 内部 2s+2s 时限，run_blocking
/// 承载），未知/已杀会话 → Err。
#[tauri::command]
#[specta::specta]
pub async fn pty_kill(
    state: tauri::State<'_, AppState>,
    args: PtyKillArgs,
) -> Result<(), BridgeError> {
    let host = Arc::clone(&state.pty_host);
    run_blocking(move || host.kill(&args.id).map(|_| ()).map_err(BridgeError::from)).await
}

/// spawn cwd（不做 OSC7 追踪，规格 §2.1）；未知会话 → Err。
#[tauri::command]
#[specta::specta]
pub async fn pty_get_cwd(
    state: tauri::State<'_, AppState>,
    args: PtyIdArgs,
) -> Result<String, BridgeError> {
    let host = Arc::clone(&state.pty_host);
    run_blocking(move || {
        host.get_cwd(&args.id)
            .ok_or_else(|| BridgeError::message(format!("unknown session: {}", args.id)))
    })
    .await
}

/// 最近 resize/spawn 的尺寸；未知会话 → `null`（TS `getSize` 契约）。
#[tauri::command]
#[specta::specta]
pub async fn pty_get_size(
    state: tauri::State<'_, AppState>,
    args: PtyIdArgs,
) -> Result<Option<PtySizeReply>, BridgeError> {
    let host = Arc::clone(&state.pty_host);
    run_blocking(move || {
        Ok(host
            .get_size(&args.id)
            .map(|(cols, rows)| PtySizeReply { cols, rows }))
    })
    .await
}

/// 会话在册即有 pty（kill/退出路径摘除后为 `false`）。
#[tauri::command]
#[specta::specta]
pub async fn pty_has_pty(
    state: tauri::State<'_, AppState>,
    args: PtyIdArgs,
) -> Result<bool, BridgeError> {
    Ok(state.pty_host.has_pty(&args.id))
}

/// 在册活会话列表：host 三元组 + bridge 补 `worktreeId`/`title:''`；
/// `agentOwnership` 保持 host 的 `'unknown'`（对齐 `PtyListedSession`）。
#[tauri::command]
#[specta::specta]
pub async fn pty_list_sessions(
    state: tauri::State<'_, AppState>,
    args: PtyListSessionsArgs,
) -> Result<Vec<PtyListedSessionRow>, BridgeError> {
    let _ = args.scope;
    let host = Arc::clone(&state.pty_host);
    let worktree_ids = state.pty_worktree_ids.clone();
    run_blocking(move || {
        Ok(host
            .list_sessions()
            .into_iter()
            .map(|session| PtyListedSessionRow {
                worktree_id: worktree_ids.get(&session.id),
                title: String::new(),
                id: session.id,
                cwd: session.cwd,
                agent_ownership: session.agent_ownership,
            })
            .collect())
    })
    .await
}

// ===== §2.2 web-stub 同形缺省（权威清单：web-terminal-api.ts） =====
//
// 终端 liveness / 快照 / 投递机械属 Phase 2+（规格 §2.5）；命令面先以 web
// stub 逐字同形的常量占位，渲染层 watchdog 与能力探测不会误判。

/// §2.2：`getForegroundProcess` → `null`。
fn stub_foreground_process() -> Option<String> {
    None
}

#[tauri::command]
#[specta::specta]
pub fn pty_get_foreground_process(args: PtyIdArgs) -> Result<Option<String>, BridgeError> {
    let _ = args;
    Ok(stub_foreground_process())
}

/// §2.2：`confirmForegroundProcess` → `null`。
#[tauri::command]
#[specta::specta]
pub fn pty_confirm_foreground_process(args: PtyIdArgs) -> Result<Option<String>, BridgeError> {
    let _ = args;
    Ok(stub_foreground_process())
}

/// §2.2：`hasChildProcesses` → `false`。
fn stub_has_child_processes() -> bool {
    false
}

#[tauri::command]
#[specta::specta]
pub fn pty_has_child_processes(args: PtyIdArgs) -> Result<bool, BridgeError> {
    let _ = args;
    Ok(stub_has_child_processes())
}

/// §2.2：`getMainBufferSnapshot` → `null`（快照机械 Phase 2+，规格 §2.5）。
fn stub_main_buffer_snapshot() -> Option<Json> {
    None
}

#[tauri::command]
#[specta::specta]
pub fn pty_get_main_buffer_snapshot(
    args: PtyGetMainBufferSnapshotArgs,
) -> Result<Option<Json>, BridgeError> {
    let _ = args;
    Ok(stub_main_buffer_snapshot())
}

/// §2.2：`getAuthoritativeBufferSnapshotCapabilities` → 逐 id
/// `{id, authoritative:false}`。
pub fn authoritative_snapshot_capabilities(ids: Vec<String>) -> Vec<PtySnapshotCapability> {
    ids.into_iter()
        .map(|id| PtySnapshotCapability {
            id,
            authoritative: false,
        })
        .collect()
}

#[tauri::command]
#[specta::specta]
pub fn pty_get_authoritative_buffer_snapshot_capabilities(
    args: PtySnapshotCapabilitiesArgs,
) -> Result<Vec<PtySnapshotCapability>, BridgeError> {
    Ok(authoritative_snapshot_capabilities(args.ids))
}

/// §2.2：`inspectProcess` → reject `Error('terminal_liveness_unavailable')`。
#[tauri::command]
#[specta::specta]
pub fn pty_inspect_process(args: PtyInspectProcessArgs) -> Result<Json, BridgeError> {
    let _ = args;
    Err(BridgeError::message("terminal_liveness_unavailable"))
}

/// §2.2：`reportRendererDeliveryState` 回零 in-flight（watchdog 保持 idle）。
fn stub_delivery_health_reply() -> PtyRendererDeliveryHealthReply {
    PtyRendererDeliveryHealthReply {
        in_flight_total_chars: 0,
        in_flight_pty_count: 0,
        ms_since_last_ack: None,
    }
}

#[tauri::command]
#[specta::specta]
pub fn pty_report_renderer_delivery_state(
    args: PtyReportRendererDeliveryStateArgs,
) -> Result<PtyRendererDeliveryHealthReply, BridgeError> {
    let _ = args;
    Ok(stub_delivery_health_reply())
}

/// §2.2：`getRendererDeliveryDebugSnapshot` → web stub 零对象逐字。
fn stub_delivery_debug_snapshot() -> PtyRendererDeliveryDebugSnapshot {
    PtyRendererDeliveryDebugSnapshot::default()
}

#[tauri::command]
#[specta::specta]
pub fn pty_get_renderer_delivery_debug_snapshot(
) -> Result<PtyRendererDeliveryDebugSnapshot, BridgeError> {
    Ok(stub_delivery_debug_snapshot())
}

// ===== management（规格 §2.1：前三者真实走注册表；restart/tcc 无 daemon 语义） =====

/// host 投影 + management 行装配（daemon 字段无源：pid/createdAt/
/// protocolVersion 恒空/零值，见行类型 doc）。
pub fn management_session_row(
    session: ade_pty::ListedSession,
    size: Option<(u16, u16)>,
) -> PtyManagementSessionRow {
    let (cols, rows) = size.unwrap_or((0, 0));
    PtyManagementSessionRow {
        session_id: session.id,
        state: PtyManagementSessionState::Running,
        shell_state: PtyManagementShellState::Unsupported,
        is_alive: true,
        pid: None,
        cwd: Some(session.cwd),
        cols,
        rows,
        created_at: 0,
        protocol_version: 0,
    }
}

#[tauri::command]
#[specta::specta]
pub async fn pty_management_list_sessions(
    state: tauri::State<'_, AppState>,
) -> Result<PtyManagementListReply, BridgeError> {
    let host = Arc::clone(&state.pty_host);
    run_blocking(move || {
        let sessions = host
            .list_sessions()
            .into_iter()
            .map(|session| {
                let size = host.get_size(&session.id);
                management_session_row(session, size)
            })
            .collect();
        Ok(PtyManagementListReply {
            degraded: false,
            sessions,
        })
    })
    .await
}

#[tauri::command]
#[specta::specta]
pub async fn pty_management_kill_all(
    state: tauri::State<'_, AppState>,
) -> Result<PtyManagementKillAllReply, BridgeError> {
    let host = Arc::clone(&state.pty_host);
    run_blocking(move || {
        let ids: Vec<String> = host
            .list_sessions()
            .into_iter()
            .map(|session| session.id)
            .collect();
        let mut killed_session_ids = Vec::new();
        for id in &ids {
            // 竞态容忍：list 与 kill 之间自然退出的会话按未杀计，不中断
            // 其余会话的收尾。
            if host.kill(id).is_ok() {
                killed_session_ids.push(id.clone());
            }
        }
        let remaining_count = host.list_sessions().len();
        Ok(PtyManagementKillAllReply {
            killed_count: killed_session_ids.len() as u32,
            remaining_count: remaining_count as u32,
            killed_session_ids,
        })
    })
    .await
}

/// 单会话 kill；未知/已杀会话回 `{success:false}`（与 web stub 形状一致），
/// 不作错误抛出。
#[tauri::command]
#[specta::specta]
pub async fn pty_management_kill_one(
    state: tauri::State<'_, AppState>,
    args: PtyManagementKillOneArgs,
) -> Result<PtyManagementOpReply, BridgeError> {
    let host = Arc::clone(&state.pty_host);
    run_blocking(move || {
        Ok(PtyManagementOpReply {
            success: host.kill(&args.session_id).is_ok(),
        })
    })
    .await
}

/// 无 daemon 可重启（规格 §2.1）；恒 `{success:true}`。
#[tauri::command]
#[specta::specta]
pub fn pty_management_restart() -> Result<PtyManagementOpReply, BridgeError> {
    Ok(PtyManagementOpReply { success: true })
}

/// 本侧无 daemon pid 记录可查 → 恒 `{health:'unknown'}`（横幅不显示）。
#[tauri::command]
#[specta::specta]
pub fn pty_management_mac_tcc_attribution(
) -> Result<PtyManagementMacTccAttributionReply, BridgeError> {
    Ok(PtyManagementMacTccAttributionReply {
        health: PtyManagementMacTccHealth::Unknown,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};

    #[test]
    fn payload_serializes_camel_case() {
        let payload = DataEndpointPayload {
            port: 51234,
            token: "abcd".to_string(),
        };
        let value: Value = serde_json::to_value(&payload).unwrap();
        assert_eq!(value, serde_json::json!({ "port": 51234, "token": "abcd" }));
    }

    #[test]
    fn spawn_args_deserialize_effective_fields_from_invoke_payload() {
        // 规格 §2.4 生效字段全量（Task 10 brief Step 1-2 的字面载荷）。
        let payload = json!({
            "args": {
                "cols": 80,
                "rows": 24,
                "cwd": "/tmp",
                "cwdFallback": "worktree",
                "worktreeId": "r1::/tmp/x",
                "env": { "K": "V" },
                "envToDelete": ["E"],
                "command": "echo hi",
                "shellOverride": "/bin/zsh",
                "sessionId": "p-existing"
            }
        });
        let args: PtySpawnArgs =
            serde_json::from_value(payload["args"].clone()).expect("deserialize spawn args");
        assert_eq!(args.cols, 80);
        assert_eq!(args.rows, 24);
        assert_eq!(args.cwd.as_deref(), Some("/tmp"));
        assert_eq!(args.cwd_fallback.as_deref(), Some("worktree"));
        assert_eq!(args.worktree_id.as_deref(), Some("r1::/tmp/x"));
        assert_eq!(args.env.get("K").map(String::as_str), Some("V"));
        assert_eq!(args.env_to_delete, vec!["E".to_string()]);
        assert_eq!(args.command.as_deref(), Some("echo hi"));
        assert_eq!(args.shell_override.as_deref(), Some("/bin/zsh"));
        assert_eq!(args.session_id.as_deref(), Some("p-existing"));
    }

    #[test]
    fn spawn_args_ignore_contract_only_fields_and_default_missing() {
        let args: PtySpawnArgs = serde_json::from_value(json!({
            "cols": 120,
            "rows": 40,
            "commandDelivery": "provider",
            "launchToken": "tok",
            "launchAgent": "codex",
            "startupCommandDelivery": "fast",
            "telemetry": { "agent_kind": "codex" },
            "connectionId": null,
            "tabId": "t1",
            "leafId": "l1",
            "initiallyHidden": true,
            "terminalColorQueryReplies": { "foreground": "\\e]11;?" }
        }))
        .expect("contract-only fields are ignored");
        assert_eq!(args.cols, 120);
        assert_eq!(args.rows, 40);
        assert!(args.cwd.is_none());
        assert!(args.cwd_fallback.is_none());
        assert!(args.env.is_empty());
        assert!(args.env_to_delete.is_empty());
        assert!(args.command.is_none());
        assert!(args.shell_override.is_none());
        assert!(args.session_id.is_none());
        assert!(args.worktree_id.is_none());
    }

    #[test]
    fn spawn_reply_shape_fresh_and_reattach() {
        let fresh = PtySpawnReply {
            id: "p1".to_string(),
            is_reattach: None,
        };
        assert_eq!(serde_json::to_value(&fresh).unwrap(), json!({ "id": "p1" }));
        let reattach = PtySpawnReply {
            id: "p1".to_string(),
            is_reattach: Some(true),
        };
        assert_eq!(
            serde_json::to_value(&reattach).unwrap(),
            json!({ "id": "p1", "isReattach": true })
        );
    }

    #[test]
    fn resolve_cwd_prefers_explicit_cwd() {
        let explicit =
            resolve_spawn_cwd(Some("/explicit"), Some("worktree"), Some("r::/wt"), "/home");
        assert_eq!(explicit.path, "/explicit");
        assert!(!explicit.worktree_fallback_missed);
        // 空串 cwd 视为缺省，走回退链（空 program/cwd 必败，不得透传）；
        // 回退被请求但未命中 → miss 旗标立（规格 §4.6 记 warn 用）。
        let missed = resolve_spawn_cwd(Some(""), Some("worktree"), Some("r::/wt"), "/home");
        assert_eq!(missed.path, "/home");
        assert!(missed.worktree_fallback_missed);
    }

    #[test]
    fn resolve_cwd_worktree_fallback_needs_fallback_marker_and_real_dir() {
        let dir = std::env::temp_dir().join(format!("ade-bridge-cwd-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let dir_str = dir.to_str().expect("utf-8 temp dir").to_string();
        // `r1::<dir>` 命中 path 段且是目录 → 采用。
        let hit = resolve_spawn_cwd(
            None,
            Some("worktree"),
            Some(&format!("r1::{dir_str}")),
            "/home",
        );
        assert_eq!(hit.path, dir_str);
        assert!(!hit.worktree_fallback_missed);
        // worktreeId 无 "::" path 段 → home + miss。
        let no_sep = resolve_spawn_cwd(None, Some("worktree"), Some("r1"), "/home");
        assert_eq!(no_sep.path, "/home");
        assert!(no_sep.worktree_fallback_missed);
        // path 段不是目录 → home + miss。
        let not_dir = resolve_spawn_cwd(
            None,
            Some("worktree"),
            Some("r1::/definitely/not/a/dir"),
            "/home",
        );
        assert_eq!(not_dir.path, "/home");
        assert!(not_dir.worktree_fallback_missed);
        // cwdFallback 非 'worktree' → home 但不算 miss（回退未被请求）。
        let other = resolve_spawn_cwd(None, Some("repo"), Some(&format!("r1::{dir_str}")), "/home");
        assert_eq!(other.path, "/home");
        assert!(!other.worktree_fallback_missed);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn stub_five_piece_matches_web_stub_verbatim() {
        // §2.2 逐字：getForegroundProcess/confirmForegroundProcess → null。
        assert_eq!(
            serde_json::to_value(stub_foreground_process()).unwrap(),
            Value::Null
        );
        // hasChildProcesses → false。
        assert_eq!(
            serde_json::to_value(stub_has_child_processes()).unwrap(),
            json!(false)
        );
        // getMainBufferSnapshot → null。
        assert_eq!(
            serde_json::to_value(stub_main_buffer_snapshot()).unwrap(),
            Value::Null
        );
        // inspectProcess → reject Error('terminal_liveness_unavailable')。
        assert_eq!(
            serde_json::to_value(BridgeError::message("terminal_liveness_unavailable")).unwrap(),
            json!({ "message": "terminal_liveness_unavailable" })
        );
        // reportRendererDeliveryState → 零 in-flight（watchdog 保持 idle）。
        assert_eq!(
            serde_json::to_value(stub_delivery_health_reply()).unwrap(),
            json!({
                "inFlightTotalChars": 0,
                "inFlightPtyCount": 0,
                "msSinceLastAck": null
            })
        );
    }

    #[test]
    fn authoritative_capabilities_are_false_per_id() {
        let rows = authoritative_snapshot_capabilities(vec!["a".to_string(), "b".to_string()]);
        assert_eq!(
            serde_json::to_value(&rows).unwrap(),
            json!([
                { "id": "a", "authoritative": false },
                { "id": "b", "authoritative": false }
            ])
        );
        assert!(authoritative_snapshot_capabilities(vec![]).is_empty());
    }

    #[test]
    fn renderer_delivery_debug_snapshot_is_web_stub_zero_object() {
        // web-terminal-api.ts:51-78 逐字段照抄（含 EMPTY diagnostics）。
        assert_eq!(
            serde_json::to_value(stub_delivery_debug_snapshot()).unwrap(),
            json!({
                "pendingPtyCount": 0,
                "pendingChars": 0,
                "maxPendingCharsByPty": 0,
                "rendererInFlightPtyCount": 0,
                "rendererInFlightChars": 0,
                "maxRendererInFlightCharsByPty": 0,
                "activeRendererPtyCount": 0,
                "flushScheduled": false,
                "peakPendingChars": 0,
                "peakMaxPendingCharsByPty": 0,
                "peakRendererInFlightChars": 0,
                "peakMaxRendererInFlightCharsByPty": 0,
                "ackGatedFlushSkipCount": 0,
                "hiddenDeliveryGatedPtyCount": 0,
                "hiddenDeliveryGatedVisiblePtyCount": 0,
                "hiddenDeliveryGatedActivePtyCount": 0,
                "deliveryInterestPtyCount": 0,
                "hiddenDeliveryDroppedChars": 0,
                "hiddenDeliveryDroppedChunks": 0,
                "pendingDroppedChars": 0,
                "diagnostics": {
                    "appVersion": "",
                    "mainUptimeMs": 0,
                    "windowFocused": null,
                    "windowVisible": null,
                    "windowMinimized": null,
                    "msSinceLastPowerSuspend": null,
                    "msSinceLastPowerResume": null,
                    "perPty": [],
                    "breadcrumbs": []
                },
                "rendererLifecycleResetCount": 0,
                "lastLifecycleResetClearedChars": 0,
                "rendererPtyDispatcherReady": false,
                "rendererDispatcherReadyForcedCount": 0
            })
        );
    }

    #[test]
    fn listed_session_row_joins_worktree_map_and_empty_title() {
        let row = PtyListedSessionRow {
            id: "p1".to_string(),
            cwd: "/wt".to_string(),
            title: String::new(),
            worktree_id: Some("r1::/wt".to_string()),
            agent_ownership: "unknown".to_string(),
        };
        assert_eq!(
            serde_json::to_value(&row).unwrap(),
            json!({
                "id": "p1",
                "cwd": "/wt",
                "title": "",
                "worktreeId": "r1::/wt",
                "agentOwnership": "unknown"
            })
        );
        let bare = PtyListedSessionRow {
            id: "p2".to_string(),
            cwd: "/tmp".to_string(),
            title: String::new(),
            worktree_id: None,
            agent_ownership: "unknown".to_string(),
        };
        let value = serde_json::to_value(&bare).unwrap();
        assert!(
            value.get("worktreeId").is_none(),
            "worktreeId absent when the session carries no mapping"
        );
    }

    #[test]
    fn management_row_projects_running_registry_sessions() {
        let row = management_session_row(
            ade_pty::ListedSession {
                id: "p1".to_string(),
                cwd: "/wt".to_string(),
                agent_ownership: "unknown".to_string(),
            },
            Some((120, 40)),
        );
        assert_eq!(
            serde_json::to_value(&row).unwrap(),
            json!({
                "sessionId": "p1",
                "state": "running",
                "shellState": "unsupported",
                "isAlive": true,
                "pid": null,
                "cwd": "/wt",
                "cols": 120,
                "rows": 40,
                "createdAt": 0,
                "protocolVersion": 0
            })
        );
    }

    #[test]
    fn management_constant_replies_match_contract() {
        assert_eq!(
            serde_json::to_value(PtyManagementOpReply { success: true }).unwrap(),
            json!({ "success": true })
        );
        // macTccAttribution → `{health:'unknown'}` 包裹对象（对齐
        // pty-management-api.ts 的 `{ health }` 契约与 web stub）。
        assert_eq!(
            serde_json::to_value(PtyManagementMacTccAttributionReply {
                health: PtyManagementMacTccHealth::Unknown,
            })
            .unwrap(),
            json!({ "health": "unknown" })
        );
        assert_eq!(
            serde_json::to_value(PtyManagementKillAllReply {
                killed_count: 2,
                remaining_count: 0,
                killed_session_ids: vec!["a".to_string(), "b".to_string()],
            })
            .unwrap(),
            json!({
                "killedCount": 2,
                "remainingCount": 0,
                "killedSessionIds": ["a", "b"]
            })
        );
        assert_eq!(
            serde_json::to_value(PtyManagementListReply {
                degraded: false,
                sessions: vec![],
            })
            .unwrap(),
            json!({ "degraded": false, "sessions": [] })
        );
    }

    #[test]
    fn management_kill_one_args_deserialize_camel_case() {
        let args: PtyManagementKillOneArgs =
            serde_json::from_value(json!({ "sessionId": "p9" })).unwrap();
        assert_eq!(args.session_id, "p9");
    }
}
