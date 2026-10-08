use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use ade_core::defaults::{onboarding_defaults, settings_defaults, ui_state_defaults};
use ade_fs::{FsService, FsWatcher};
use ade_git::runner::CancelToken;
use ade_hooks::{AgentHookServer, StartOptions};
use ade_pty::PtyHost;
use ade_store::onboarding_store::OnboardingStore;
use ade_store::sqlite::Store;
use ade_store::projects_store::ProjectsStore;
use ade_store::settings_store::SettingsStore;
use ade_store::ui_state_store::UiStateStore;
use ade_store::worktree_meta_store::WorktreeMetaStore;
use ade_store::SCHEMA_VERSION;
use serde::Serialize;
use serde_json::Value;
use tauri::{AppHandle, Manager};

use crate::commands::platform::{platform_info, PlatformInfo};
use crate::commands::preflight::PathHydrationCache;
use crate::errors::BridgeError;
use crate::events;
use crate::json::Json;

/// Debounce window for settings/ui-state disk writes (spec §4.1).
pub const WRITE_DEBOUNCE: Duration = Duration::from_millis(1000);
/// Hard cap from the first dirty schedule in a burst; a busy writer flushes at
/// this point even while updates keep arriving (spec §4.1).
pub const WRITE_MAX_WAIT: Duration = Duration::from_millis(5000);

/// Renderer flush window on window-close quit (spec §3.4).
const FLUSH_ACK_TIMEOUT: Duration = Duration::from_secs(2);

/// Renderer flush handshake state: `false` until `session_flush_ack` lands.
pub type SessionFlushSignal = Arc<(Mutex<bool>, Condvar)>;

/// One-shot `CloseRequested` re-entry latch (spec §3.4).
///
/// `WebviewWindow::close()` re-fires `CloseRequested` "like a user-initiated
/// close request", so the flush waiter's own programmatic close comes back
/// through the same host handler. Only the first (user-initiated) request may
/// start the handshake and hold the window open; the re-entrant one must pass
/// through unprevented or the quit would never converge.
#[derive(Default)]
pub(crate) struct CloseFlushLatch(AtomicBool);

impl CloseFlushLatch {
    /// `true` = first close request (run the flush handshake, `prevent_close`);
    /// `false` = re-entry (let the close proceed).
    pub(crate) fn try_start(&self) -> bool {
        !self.0.swap(true, Ordering::SeqCst)
    }
}

pub(crate) fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

type PersistFn = Box<dyn Fn() -> Result<(), ade_store::StoreError> + Send + Sync + 'static>;

struct PendingState {
    dirty: bool,
    first_dirty_at: Option<Instant>,
    last_dirty_at: Option<Instant>,
    force_flush: bool,
    writing: bool,
    shutdown: bool,
    writes: u64,
    last_error: Option<String>,
}

struct SchedulerShared {
    state: Mutex<PendingState>,
    wake: Condvar,
    persist: PersistFn,
    debounce: Duration,
    max_wait: Duration,
}

/// Debounced, asynchronous persistence for one JSON store.
///
/// `schedule` marks the store dirty and returns immediately; a worker thread
/// persists at most once per [`WRITE_DEBOUNCE`] of quiet, forced every
/// [`WRITE_MAX_WAIT`] while updates keep arriving. The in-memory store is the
/// source of truth; the persist closure writes its current snapshot.
pub struct WriteScheduler {
    shared: Arc<SchedulerShared>,
    worker: Mutex<Option<JoinHandle<()>>>,
}

impl WriteScheduler {
    pub fn new(
        debounce: Duration,
        max_wait: Duration,
        persist: impl Fn() -> Result<(), ade_store::StoreError> + Send + Sync + 'static,
    ) -> Self {
        let shared = Arc::new(SchedulerShared {
            state: Mutex::new(PendingState {
                dirty: false,
                first_dirty_at: None,
                last_dirty_at: None,
                force_flush: false,
                writing: false,
                shutdown: false,
                writes: 0,
                last_error: None,
            }),
            wake: Condvar::new(),
            persist: Box::new(persist),
            debounce,
            max_wait,
        });
        let worker_shared = Arc::clone(&shared);
        let handle = thread::Builder::new()
            .name("ade-bridge-writer".to_string())
            .spawn(move || run_worker(&worker_shared))
            .expect("failed to spawn ade-bridge write scheduler");
        Self {
            shared,
            worker: Mutex::new(Some(handle)),
        }
    }

    /// Mark the backing store dirty. Never blocks on IO.
    pub fn schedule(&self) {
        let mut state = lock(&self.shared.state);
        if state.shutdown {
            return;
        }
        let now = Instant::now();
        state.dirty = true;
        if state.first_dirty_at.is_none() {
            state.first_dirty_at = Some(now);
        }
        state.last_dirty_at = Some(now);
        self.shared.wake.notify_all();
    }

    /// Persist any pending update immediately and wait for the write to finish.
    pub fn flush(&self) -> Result<(), String> {
        let mut state = lock(&self.shared.state);
        if !state.dirty && !state.writing {
            return state.last_error.clone().map_or(Ok(()), Err);
        }
        state.force_flush = true;
        self.shared.wake.notify_all();
        while state.dirty || state.writing {
            state = self
                .shared
                .wake
                .wait(state)
                .unwrap_or_else(|poisoned| poisoned.into_inner());
        }
        state.last_error.clone().map_or(Ok(()), Err)
    }

    /// Number of completed disk writes; exposed for tests and diagnostics.
    pub fn write_count(&self) -> u64 {
        lock(&self.shared.state).writes
    }
}

impl Drop for WriteScheduler {
    fn drop(&mut self) {
        {
            let mut state = lock(&self.shared.state);
            state.shutdown = true;
            self.shared.wake.notify_all();
        }
        let handle = lock(&self.worker).take();
        if let Some(handle) = handle {
            let _ = handle.join();
        }
    }
}

fn run_worker(shared: &SchedulerShared) {
    loop {
        let mut state = lock(&shared.state);
        while !state.dirty && !state.shutdown {
            state = shared
                .wake
                .wait(state)
                .unwrap_or_else(|poisoned| poisoned.into_inner());
        }
        if state.shutdown && !state.dirty {
            return;
        }

        while !state.force_flush && !state.shutdown {
            let now = Instant::now();
            let first = state.first_dirty_at.unwrap_or(now);
            let last = state.last_dirty_at.unwrap_or(now);
            let deadline = (last + shared.debounce).min(first + shared.max_wait);
            if now >= deadline {
                break;
            }
            let (next, _) = shared
                .wake
                .wait_timeout(state, deadline - now)
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            state = next;
        }

        state.dirty = false;
        state.force_flush = false;
        state.first_dirty_at = None;
        state.last_dirty_at = None;
        state.writing = true;
        drop(state);

        // Why: a panicking persist must not leave `writing` latched, or every
        // later `flush()` (including the exit flush) would hang forever.
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| (shared.persist)()));

        let mut state = lock(&shared.state);
        state.writing = false;
        match result {
            Ok(Ok(())) => {
                state.writes += 1;
                state.last_error = None;
            }
            Ok(Err(error)) => {
                state.last_error = Some(error.to_string());
                eprintln!("[ade-bridge] failed to persist store: {error}");
            }
            Err(_) => {
                state.last_error = Some("persist task panicked".to_string());
                eprintln!("[ade-bridge] persist task panicked");
            }
        }
        shared.wake.notify_all();
        if state.shutdown {
            return;
        }
    }
}

/// Bootstrap snapshot injected into the main window before the document parses.
#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct BootstrapPayload {
    pub settings: Json,
    pub platform: PlatformInfo,
    pub schema_version: u64,
}

/// Pure assembly of the init-script snapshot; separated from [`AppState`] so
/// its serialized shape can be tested without a Tauri runtime.
pub fn bootstrap_payload(settings: serde_json::Value, platform: PlatformInfo) -> BootstrapPayload {
    BootstrapPayload {
        settings: Json::new(settings),
        platform,
        schema_version: SCHEMA_VERSION,
    }
}

/// Stores loaded from the app data directory, with fs roots already authorized.
pub struct PersistedState {
    pub settings: SettingsStore,
    pub ui: UiStateStore,
    pub onboarding: OnboardingStore,
    pub projects: ProjectsStore,
    pub worktree_meta: WorktreeMetaStore,
    pub fs: FsService,
}

/// Load `settings.json`/`ui-state.json`/`onboarding.json`/`projects.json` and
/// re-grant fs access to every persisted repo and folder-workspace root, so a
/// restart does not leave existing repos failing access-denied.
pub fn load_persisted_state(data_dir: &Path, home: &str) -> PersistedState {
    let settings = SettingsStore::load(data_dir.join("settings.json"), settings_defaults(home));
    let ui = UiStateStore::load(data_dir.join("ui-state.json"), ui_state_defaults());
    let onboarding = OnboardingStore::load(data_dir.join("onboarding.json"), onboarding_defaults());
    let projects = ProjectsStore::load(data_dir.join("projects.json"));
    let worktree_meta = WorktreeMetaStore::load(data_dir.join("worktrees.json"));
    let fs = FsService::new();
    authorize_persisted_roots(&fs, &projects);
    PersistedState {
        settings,
        ui,
        onboarding,
        projects,
        worktree_meta,
        fs,
    }
}

/// Authorize every persisted root; a failing path is logged and skipped so one
/// bad entry cannot abort startup.
pub fn authorize_persisted_roots(fs: &FsService, projects: &ProjectsStore) {
    for path in persisted_root_paths(projects) {
        if let Err(error) = fs.authorize_root(&path) {
            eprintln!("[ade-bridge] failed to authorize persisted root '{path}': {error}");
        }
    }
}

fn persisted_root_paths(projects: &ProjectsStore) -> Vec<String> {
    let mut paths = Vec::new();
    for repo in projects.repos() {
        if let Some(path) = repo.get("path").and_then(Value::as_str) {
            paths.push(path.to_string());
        }
    }
    for workspace in projects.folder_workspaces() {
        if let Some(path) = workspace.get("folderPath").and_then(Value::as_str) {
            paths.push(path.to_string());
        }
    }
    paths
}

pub(crate) fn arm_session_flush_signal(
    slot: &Mutex<Option<SessionFlushSignal>>,
) -> SessionFlushSignal {
    let signal: SessionFlushSignal = Arc::new((Mutex::new(false), Condvar::new()));
    *lock(slot) = Some(Arc::clone(&signal));
    signal
}

pub(crate) fn signal_session_flush_ack(slot: &Mutex<Option<SessionFlushSignal>>) {
    let current = lock(slot).clone();
    if let Some(signal) = current {
        let (flag, cvar) = &*signal;
        let mut acked = flag.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        *acked = true;
        cvar.notify_all();
    }
}

/// spawn 成功后的 ptyId → worktreeId 映射：`pty_list_sessions` 补列用。
/// 即时退出会话的 Exit 可能先于 spawn 返回到达（Task 9 交接）——清理与查询
/// 对未知 id 一律静默，不得 unwrap/panic。
#[derive(Clone, Default)]
pub struct PtyWorktreeIds(Arc<Mutex<HashMap<String, String>>>);

impl PtyWorktreeIds {
    pub fn record(&self, pty_id: &str, worktree_id: String) {
        lock(&self.0).insert(pty_id.to_string(), worktree_id);
    }

    pub fn get(&self, pty_id: &str) -> Option<String> {
        lock(&self.0).get(pty_id).cloned()
    }

    /// Exit 清理：未知 id（miss）静默返回 `None`。
    pub fn remove(&self, pty_id: &str) -> Option<String> {
        lock(&self.0).remove(pty_id)
    }
}

/// In-flight `git_status` cancellations, keyed by the renderer's request
/// token. Re-registering the same token cancels the superseded run first, so a
/// stale poll can never publish a result after its caller gave up.
pub struct GitCancelRegistry {
    tokens: Mutex<HashMap<String, CancelToken>>,
}

impl GitCancelRegistry {
    pub fn new() -> Self {
        Self {
            tokens: Mutex::new(HashMap::new()),
        }
    }

    /// Register one run and return its live cancellation flag.
    pub fn register(&self, token: &str) -> CancelToken {
        let mut tokens = lock(&self.tokens);
        if let Some(previous) = tokens.get(token) {
            previous.cancel();
        }
        let fresh = CancelToken::new();
        tokens.insert(token.to_string(), fresh.clone());
        fresh
    }

    /// Drop one registration once its run returned (success or failure).
    pub fn finish(&self, token: &str) {
        lock(&self.tokens).remove(token);
    }

    /// Set and drop one registration; `false` when the run already finished.
    pub fn cancel(&self, token: &str) -> bool {
        match lock(&self.tokens).remove(token) {
            Some(cancel) => {
                cancel.cancel();
                true
            }
            None => false,
        }
    }
}

impl Default for GitCancelRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// gh 子进程并发闸（全局 4，FIFO 近似——Condvar 唤醒不保证顺序，可接受）。
pub struct GhConcurrencyGate {
    permits: Mutex<usize>,
    available: Condvar,
}

impl GhConcurrencyGate {
    pub fn new(max: usize) -> Self {
        Self { permits: Mutex::new(max), available: Condvar::new() }
    }

    pub fn acquire(&self) -> GhGateGuard<'_> {
        let mut permits = lock(&self.permits);
        while *permits == 0 {
            permits = self.available.wait(permits).unwrap_or_else(|poisoned| poisoned.into_inner());
        }
        *permits -= 1;
        GhGateGuard { gate: self }
    }
}

pub struct GhGateGuard<'a> { gate: &'a GhConcurrencyGate }

impl Drop for GhGateGuard<'_> {
    fn drop(&mut self) {
        let mut permits = lock(&self.gate.permits);
        *permits += 1;
        self.gate.available.notify_one();
    }
}

/// gh 可执行文件解析结果的进程内缓存（成功才写；失败每次重试）。
#[derive(Clone, Default)]
pub struct GhPathCache(Arc<Mutex<Option<std::path::PathBuf>>>);

impl GhPathCache {
    pub fn get(&self) -> Option<std::path::PathBuf> { lock(&self.0).clone() }
    pub fn set(&self, value: std::path::PathBuf) { *lock(&self.0) = Some(value); }
}

/// Shared backend state for every command. Initialized once in `setup` with the
/// app data directory before the main window is built.
pub struct AppState {
    pub settings: Arc<Mutex<SettingsStore>>,
    pub ui: Arc<Mutex<UiStateStore>>,
    pub onboarding: Arc<Mutex<OnboardingStore>>,
    pub projects: Mutex<ProjectsStore>,
    /// Per-worktree metadata (`worktrees.json`), shared with the worktree
    /// commands as an `Arc` so blocking closures can own it.
    pub worktree_meta: Arc<WorktreeMetaStore>,
    pub fs: Arc<FsService>,
    pub watchers: FsWatcher,
    pub app: AppHandle,
    /// PtyHost 门面（Task 9）：WS 数据面 + 会话注册表 + 事件回调源。
    pub pty_host: Arc<PtyHost>,
    /// ptyId → worktreeId（`pty_spawn` 记、`pty_list_sessions` 查、Exit 清）。
    pub pty_worktree_ids: PtyWorktreeIds,
    /// SQLite 会话态存储（规格 §3.1；Task 2）。损坏由 `Store::open` 隔离重建。
    pub session: Arc<Store>,
    /// hook server（规格 §3.1；Task 6）：HTTP 接收 + endpoint 发布 + 状态缓存。
    pub hooks: Arc<AgentHookServer>,
    /// `session:flush-requested` 握手槽（spec §3.4；None = 无等待中的退出编排）。
    pub(crate) session_flush_slot: Mutex<Option<SessionFlushSignal>>,
    /// CloseRequested 重入门闩（spec §3.4）：仅首次（用户发起）close 编排
    /// flush；flush 后的 programmatic `window.close()` 会再次触发
    /// CloseRequested，该次必须放行，否则 quit 永不收敛。
    close_flush_latch: CloseFlushLatch,
    /// 启动时解析的用户 home（spawn cwd 兜底，规格 §2.4）。
    pub home: String,
    /// 登录 shell PATH 水合的进程内缓存（Task 11）：进程生命周期内最多水合一次。
    pub path_hydration_cache: PathHydrationCache,
    /// gh 执行器并发闸（规格 §3.1；全局 4）。
    pub gh_gate: Arc<GhConcurrencyGate>,
    /// gh 可执行文件解析缓存（成功才写）。
    pub gh_path_cache: GhPathCache,
    pub git_cancels: GitCancelRegistry,
    settings_writer: WriteScheduler,
    ui_writer: WriteScheduler,
}

impl AppState {
    pub fn initialize(app: &AppHandle, pty_host: Arc<PtyHost>) -> Result<Self, BridgeError> {
        let data_dir = app.path().app_data_dir().map_err(|error| {
            BridgeError::message(format!("failed to resolve app data dir: {error}"))
        })?;
        std::fs::create_dir_all(&data_dir)?;
        let session = Arc::new(Store::open(&data_dir.join("ade.sqlite"))?);
        let home = app
            .path()
            .home_dir()
            .map_err(|error| BridgeError::message(format!("failed to resolve home dir: {error}")))?
            .to_string_lossy()
            .into_owned();

        let persisted = load_persisted_state(&data_dir, &home);
        let settings = Arc::new(Mutex::new(persisted.settings));
        let hooks_enabled = lock(&settings)
            .get()
            .get("agentStatusHooksEnabled")
            .and_then(Value::as_bool)
            .unwrap_or(true);
        let hooks = {
            let app_handle = app.clone();
            AgentHookServer::start(
                StartOptions {
                    app_data_dir: data_dir.join("agent-hooks"),
                    home: home.clone(),
                    env: if cfg!(debug_assertions) {
                        "development".to_string()
                    } else {
                        "production".to_string()
                    },
                    // 启动策略（规格 §3.3）：关闭 = skip 不删；开启 = 安装/更新。
                    install_enabled: hooks_enabled,
                },
                Box::new(move |event| {
                    events::emit_json(
                        &app_handle,
                        events::AGENT_HOOK_RAW,
                        events::AgentHookRawPayload::from(event),
                    );
                }),
            )
        };
        let ui = Arc::new(Mutex::new(persisted.ui));
        let onboarding = Arc::new(Mutex::new(persisted.onboarding));

        let settings_writer = WriteScheduler::new(WRITE_DEBOUNCE, WRITE_MAX_WAIT, {
            let store = Arc::clone(&settings);
            move || lock(&store).persist()
        });
        let ui_writer = WriteScheduler::new(WRITE_DEBOUNCE, WRITE_MAX_WAIT, {
            let store = Arc::clone(&ui);
            move || lock(&store).persist()
        });

        let watchers = FsWatcher::new();
        {
            let app_handle = app.clone();
            watchers.subscribe(move |payload| {
                events::emit_json(&app_handle, events::FS_CHANGED, payload);
            });
        }

        let pty_worktree_ids = PtyWorktreeIds::default();
        {
            let app_handle = app.clone();
            let worktree_ids = pty_worktree_ids.clone();
            // PtyHost 事件 → Tauri 事件广播（规格 §2.1：pty:spawned/pty:exit）。
            // 事件统一经此回调发（`PtyHost::spawn` 已发 Spawned，命令层不重复
            // emit）；Exit 顺带清 worktreeId 映射——即时退出会话的 Exit 可能
            // 先于 spawn 返回到达，remove 对未知 id 静默（Task 9 交接）。
            pty_host.set_event_callback(Box::new(move |event| {
                if let ade_pty::PtyEvent::Exit(info) = &event {
                    worktree_ids.remove(&info.id);
                }
                events::forward_pty_event(&app_handle, event);
            }));
        }

        Ok(Self {
            settings,
            ui,
            onboarding,
            projects: Mutex::new(persisted.projects),
            worktree_meta: Arc::new(persisted.worktree_meta),
            fs: Arc::new(persisted.fs),
            watchers,
            app: app.clone(),
            pty_host,
            pty_worktree_ids,
            session,
            hooks,
            session_flush_slot: Mutex::new(None),
            close_flush_latch: CloseFlushLatch::default(),
            home,
            path_hydration_cache: PathHydrationCache::default(),
            gh_gate: Arc::new(GhConcurrencyGate::new(4)),
            gh_path_cache: GhPathCache::default(),
            git_cancels: GitCancelRegistry::new(),
            settings_writer,
            ui_writer,
        })
    }

    pub(crate) fn settings_store(&self) -> MutexGuard<'_, SettingsStore> {
        lock(&self.settings)
    }

    /// Shared handle to the worktree metadata store (`worktrees.json`).
    pub fn worktree_meta_store(&self) -> Arc<WorktreeMetaStore> {
        Arc::clone(&self.worktree_meta)
    }

    pub(crate) fn ui_store(&self) -> MutexGuard<'_, UiStateStore> {
        lock(&self.ui)
    }

    /// Shared handle to the SQLite session store (spec §3.1).
    pub fn session_store(&self) -> &Store {
        &self.session
    }

    pub(crate) fn schedule_settings_write(&self) {
        self.settings_writer.schedule();
    }

    pub(crate) fn schedule_ui_write(&self) {
        self.ui_writer.schedule();
    }

    /// Force any pending ui-state write and surface its result, so
    /// `ui_set_with_ack` can reject when persistence fails (spec §5.4).
    pub(crate) fn flush_ui_state_write(&self) -> Result<(), String> {
        self.ui_writer.flush()
    }

    /// `ui_set_with_ack`: merge the partial in memory, then persist
    /// synchronously before returning.
    pub(crate) fn merge_ui_state_with_ack(&self, partial: Value) -> Result<Value, BridgeError> {
        crate::commands::ui::merge_ui_state_and_flush(&self.ui, &self.ui_writer, partial)
    }

    /// Persist debounced settings/ui updates; called on app exit so a quit
    /// shortly after a change cannot drop it.
    pub fn flush_pending_writes(&self) {
        for (label, result) in [
            ("settings", self.settings_writer.flush()),
            ("ui-state", self.flush_ui_state_write()),
        ] {
            if let Err(error) = result {
                eprintln!("[ade-bridge] failed to flush {label} on exit: {error}");
            }
        }
    }

    /// Window-close quit (spec §3.4): request one renderer flush window, wait
    /// on a background thread (never the event-loop thread — macOS WKWebView
    /// IPC is main-thread), then close the window regardless of the outcome.
    ///
    /// Why `CloseRequested` and not `RunEvent::ExitRequested`: tauri-runtime-wry
    /// emits `ExitRequested { code: None }` from inside its `Destroyed` handler,
    /// i.e. after the last window and its WKWebView/JS context are already gone —
    /// the `session:flush-requested` emit reached a dead webview, so no quit
    /// could ever be acked and every close paid the full 2s windowless stall.
    /// `CloseRequested` is the only window event where the webview is still
    /// alive, so the renderer can actually receive the event and ack.
    ///
    /// Returns `true` when the handshake was armed for this request — the caller
    /// must `prevent_close()` to hold the window open for it. Returns `false`
    /// for the re-entrant request fired by the programmatic `window.close()`
    /// below (see `CloseFlushLatch`); that request must pass through unprevented,
    /// and when the last window closes Tauri exits by default, running the
    /// `RunEvent::Exit` cleanup (flush_pending_writes + checkpoint_truncate +
    /// shutdown_all).
    pub fn begin_close_with_session_flush(&self, window: tauri::WebviewWindow) -> bool {
        if !self.close_flush_latch.try_start() {
            return false;
        }
        let signal = arm_session_flush_signal(&self.session_flush_slot);
        events::emit_json(&self.app, events::SESSION_FLUSH_REQUESTED, serde_json::json!({}));
        let window_for_thread = window.clone();
        let wait = std::thread::Builder::new()
            .name("session-flush-exit".to_string())
            .spawn(move || {
                let (flag, cvar) = &*signal;
                let guard = flag.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
                // `wait_timeout_while` = brief's `wait_timeout_for`: wake early on
                // ack, else give up at the deadline (`timed_out()` ⇒ not acked).
                let (guard, acked) = {
                    let (guard, result) = cvar
                        .wait_timeout_while(guard, FLUSH_ACK_TIMEOUT, |acked| !*acked)
                        .unwrap_or_else(|poisoned| poisoned.into_inner());
                    (guard, !result.timed_out())
                };
                drop(guard);
                if !acked {
                    eprintln!("[ade-bridge] session flush ack not received in time; closing");
                }
                // Proceed with the previously-prevented close; the app exits when
                // the last window closes and `RunEvent::Exit` does the cleanup.
                let _ = window_for_thread.close();
            });
        if wait.is_err() {
            // No waiter thread → no flush window; still must close. This
            // re-enters `CloseRequested`, but the latch already admits it.
            let _ = window.close();
        }
        true
    }

    pub fn bootstrap_payload(&self) -> BootstrapPayload {
        bootstrap_payload(self.settings_store().get(), platform_info())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn worktree_id_map_records_lookups_and_tolerates_unknown_exit() {
        let ids = PtyWorktreeIds::default();
        ids.record("p1", "r1::/wt".to_string());
        assert_eq!(ids.get("p1").as_deref(), Some("r1::/wt"));
        assert_eq!(ids.get("missing"), None);
        // Task 9 交接：即时退出会话的 Exit 可能先于 spawn 返回到达，未知 id
        // 的清理必须静默（不得 unwrap/panic）。
        assert_eq!(ids.remove("missing"), None);
        assert_eq!(ids.remove("p1").as_deref(), Some("r1::/wt"));
        assert_eq!(ids.get("p1"), None);
    }

    struct TestDir {
        path: std::path::PathBuf,
    }

    impl TestDir {
        fn new(name: &str) -> Self {
            let path = std::env::temp_dir().join(format!(
                "ade-bridge-scheduler-{name}-{}",
                std::process::id()
            ));
            let _ = std::fs::remove_dir_all(&path);
            std::fs::create_dir_all(&path).expect("create test dir");
            Self { path }
        }

        fn file(&self, name: &str) -> std::path::PathBuf {
            self.path.join(name)
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }

    fn store_in(dir: &TestDir) -> Arc<Mutex<SettingsStore>> {
        Arc::new(Mutex::new(SettingsStore::load(
            dir.file("settings.json"),
            json!({ "theme": "system" }),
        )))
    }

    fn scheduler_for(
        store: &Arc<Mutex<SettingsStore>>,
        debounce_ms: u64,
        max_wait_ms: u64,
    ) -> WriteScheduler {
        let store = Arc::clone(store);
        WriteScheduler::new(
            Duration::from_millis(debounce_ms),
            Duration::from_millis(max_wait_ms),
            move || lock(&store).persist(),
        )
    }

    fn wait_until(deadline: Duration, mut condition: impl FnMut() -> bool) -> bool {
        let start = Instant::now();
        while start.elapsed() < deadline {
            if condition() {
                return true;
            }
            thread::sleep(Duration::from_millis(5));
        }
        condition()
    }

    fn read_settings(path: &std::path::Path) -> serde_json::Value {
        serde_json::from_str(&std::fs::read_to_string(path).expect("read settings")).expect("parse")
    }

    #[test]
    fn bootstrap_payload_serializes_settings_platform_and_schema_version() {
        let payload = bootstrap_payload(
            json!({ "theme": "dark" }),
            crate::commands::platform::platform_info(),
        );
        let value = serde_json::to_value(&payload).unwrap();
        assert_eq!(value["settings"]["theme"], "dark");
        assert_eq!(value["schemaVersion"], SCHEMA_VERSION);
        assert!(value["platform"]["platform"].is_string());
        assert_eq!(
            value["platform"]["platform"],
            crate::commands::platform::map_platform(std::env::consts::OS)
        );
    }

    #[test]
    fn persisted_repo_and_folder_roots_are_authorized_at_startup() {
        let dir = TestDir::new("persisted-roots");
        let repo_path = dir.path.join("repo");
        let folder_path = dir.path.join("folder-workspace");
        std::fs::create_dir_all(&repo_path).unwrap();
        std::fs::create_dir_all(&folder_path).unwrap();
        std::fs::write(
            dir.file("projects.json"),
            json!({
                "schemaVersion": 1,
                "repos": [{ "id": "r1", "path": repo_path.to_str().unwrap(), "kind": "git" }],
                "projectGroups": [],
                "folderWorkspaces": [{
                    "id": "f1",
                    "folderPath": folder_path.to_str().unwrap()
                }]
            })
            .to_string(),
        )
        .unwrap();

        let state = load_persisted_state(&dir.path, "/home/tester");

        assert!(state.fs.resolve(repo_path.to_str().unwrap()).is_ok());
        assert!(state.fs.resolve(folder_path.to_str().unwrap()).is_ok());
        let outside = dir.path.join("elsewhere");
        assert!(matches!(
            state.fs.resolve(outside.to_str().unwrap()),
            Err(ade_fs::FsError::PathAccessDenied)
        ));
    }

    #[test]
    fn persist_panic_does_not_hang_flush_and_recovers() {
        let dir = TestDir::new("persist-panic");
        let store = store_in(&dir);
        let panicking = Arc::new(std::sync::atomic::AtomicBool::new(true));
        let scheduler =
            WriteScheduler::new(Duration::from_millis(10), Duration::from_millis(10), {
                let store = Arc::clone(&store);
                let panicking = Arc::clone(&panicking);
                move || {
                    if panicking.load(std::sync::atomic::Ordering::SeqCst) {
                        panic!("persist exploded");
                    }
                    lock(&store).persist()
                }
            });

        lock(&store)
            .merge_partial(json!({ "theme": "dark" }))
            .unwrap();
        scheduler.schedule();
        assert!(
            scheduler.flush().is_err(),
            "panic surfaces as a write error"
        );

        panicking.store(false, std::sync::atomic::Ordering::SeqCst);
        scheduler.schedule();
        scheduler.flush().expect("scheduler recovers after a panic");
        assert_eq!(read_settings(&dir.file("settings.json"))["theme"], "dark");
    }

    #[test]
    fn debounces_until_quiet_then_writes() {
        let dir = TestDir::new("debounce");
        let store = store_in(&dir);
        let scheduler = scheduler_for(&store, 200, 5_000);
        lock(&store)
            .merge_partial(json!({ "theme": "dark" }))
            .unwrap();
        scheduler.schedule();

        assert!(!dir.file("settings.json").exists(), "wrote before debounce");

        assert!(wait_until(Duration::from_secs(5), || dir
            .file("settings.json")
            .exists()));
        assert_eq!(read_settings(&dir.file("settings.json"))["theme"], "dark");
        assert_eq!(scheduler.write_count(), 1);
    }

    #[test]
    fn repeated_schedules_still_write_within_max_wait() {
        let dir = TestDir::new("max-wait");
        let store = store_in(&dir);
        let scheduler = scheduler_for(&store, 2_000, 80);
        let started = Instant::now();
        while started.elapsed() < Duration::from_millis(60) {
            lock(&store)
                .merge_partial(json!({ "theme": "dark" }))
                .unwrap();
            scheduler.schedule();
            thread::sleep(Duration::from_millis(10));
        }

        assert!(
            wait_until(Duration::from_millis(400), || dir
                .file("settings.json")
                .exists()),
            "max-wait did not force a write while schedules kept arriving"
        );
        assert!(started.elapsed() < Duration::from_millis(400));
    }

    #[test]
    fn flush_forces_an_immediate_write() {
        let dir = TestDir::new("flush");
        let store = store_in(&dir);
        let scheduler = scheduler_for(&store, 60_000, 120_000);
        lock(&store)
            .merge_partial(json!({ "theme": "dark" }))
            .unwrap();
        scheduler.schedule();
        scheduler.flush().expect("flush");
        assert_eq!(read_settings(&dir.file("settings.json"))["theme"], "dark");
        assert_eq!(scheduler.write_count(), 1);
    }

    #[test]
    fn drop_flushes_pending_writes() {
        let dir = TestDir::new("drop-flush");
        let store = store_in(&dir);
        {
            let scheduler = scheduler_for(&store, 60_000, 120_000);
            lock(&store)
                .merge_partial(json!({ "theme": "dark" }))
                .unwrap();
            scheduler.schedule();
        }
        assert_eq!(read_settings(&dir.file("settings.json"))["theme"], "dark");
    }

    #[test]
    fn close_flush_latch_admits_only_the_first_close_request() {
        let latch = CloseFlushLatch::default();
        assert!(
            latch.try_start(),
            "first (user-initiated) close must run the flush handshake"
        );
        // The flush waiter's programmatic `window.close()` re-enters
        // CloseRequested; that request must pass through unprevented or the
        // quit never converges.
        assert!(!latch.try_start(), "re-entrant close must be let through");
        assert!(!latch.try_start(), "latch stays latched");
    }

    #[test]
    fn session_flush_signal_arms_resets_and_acks() {
        let slot: Mutex<Option<SessionFlushSignal>> = Mutex::new(None);
        let signal = arm_session_flush_signal(&slot);
        {
            let (lock, _) = &*signal;
            assert!(!*lock.lock().unwrap());
        }
        signal_session_flush_ack(&slot);
        {
            let (lock, _) = &*signal;
            assert!(*lock.lock().unwrap());
        }
        // Re-arm resets the flag (a second window-close quit must wait afresh).
        let rearmed = arm_session_flush_signal(&slot);
        {
            let (lock, _) = &*rearmed;
            assert!(!*lock.lock().unwrap());
        }
    }

    #[test]
    fn session_flush_ack_wakes_a_waiter_before_timeout() {
        let slot: std::sync::Arc<Mutex<Option<SessionFlushSignal>>> =
            std::sync::Arc::new(Mutex::new(None));
        // The waiter re-reads the armed signal from the slot; the parent's copy
        // must stay alive only for the arm itself (`_signal` keeps it bound).
        let _signal = arm_session_flush_signal(&slot);
        let waiter_slot = std::sync::Arc::clone(&slot);
        let handle = thread::spawn(move || {
            let slot_ref = &*waiter_slot;
            let signal = lock(slot_ref).clone().expect("armed");
            let (lock, cvar) = &*signal;
            let guard = lock.lock().unwrap();
            let (_guard, result) = cvar
                .wait_timeout_while(guard, Duration::from_millis(2_000), |acked| !*acked)
                .unwrap();
            !result.timed_out()
        });
        thread::sleep(Duration::from_millis(50));
        signal_session_flush_ack(&slot);
        assert!(handle.join().unwrap(), "ack must arrive well before the 2s timeout");
    }

    #[test]
    fn flush_propagates_persist_failures() {
        let dir = TestDir::new("flush-error");
        // A file where the store's parent directory should be makes every save
        // fail (`create_dir_all` hits ENOTDIR).
        std::fs::write(dir.file("blocked"), "").unwrap();
        let store = Arc::new(Mutex::new(SettingsStore::load(
            dir.file("blocked/settings.json"),
            json!({ "theme": "system" }),
        )));
        let scheduler =
            WriteScheduler::new(Duration::from_millis(10), Duration::from_millis(10), {
                let store = Arc::clone(&store);
                move || lock(&store).persist()
            });
        lock(&store)
            .merge_partial(json!({ "theme": "dark" }))
            .unwrap();
        scheduler.schedule();
        assert!(scheduler.flush().is_err());
    }
}
