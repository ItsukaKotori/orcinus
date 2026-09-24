use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use ade_core::defaults::{settings_defaults, ui_state_defaults};
use ade_fs::{FsService, FsWatcher};
use ade_store::projects_store::ProjectsStore;
use ade_store::settings_store::SettingsStore;
use ade_store::ui_state_store::UiStateStore;
use ade_store::SCHEMA_VERSION;
use serde::Serialize;
use tauri::{AppHandle, Manager};

use crate::commands::platform::{platform_info, PlatformInfo};
use crate::errors::BridgeError;
use crate::events;
use crate::json::Json;

/// Debounce window for settings/ui-state disk writes (spec §4.1).
pub const WRITE_DEBOUNCE: Duration = Duration::from_millis(1000);
/// Hard cap from the first dirty schedule in a burst; a busy writer flushes at
/// this point even while updates keep arriving (spec §4.1).
pub const WRITE_MAX_WAIT: Duration = Duration::from_millis(5000);

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

        let result = (shared.persist)();

        let mut state = lock(&shared.state);
        state.writing = false;
        match result {
            Ok(()) => {
                state.writes += 1;
                state.last_error = None;
            }
            Err(error) => {
                state.last_error = Some(error.to_string());
                eprintln!("[ade-bridge] failed to persist store: {error}");
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

/// Shared backend state for every command. Initialized once in `setup` with the
/// app data directory before the main window is built.
pub struct AppState {
    pub settings: Arc<Mutex<SettingsStore>>,
    pub ui: Arc<Mutex<UiStateStore>>,
    pub projects: Mutex<ProjectsStore>,
    pub fs: Arc<FsService>,
    pub watchers: FsWatcher,
    pub app: AppHandle,
    settings_writer: WriteScheduler,
    ui_writer: WriteScheduler,
}

impl AppState {
    pub fn initialize(app: &AppHandle) -> Result<Self, BridgeError> {
        let data_dir = app.path().app_data_dir().map_err(|error| {
            BridgeError::message(format!("failed to resolve app data dir: {error}"))
        })?;
        std::fs::create_dir_all(&data_dir)?;
        let home = app
            .path()
            .home_dir()
            .map_err(|error| BridgeError::message(format!("failed to resolve home dir: {error}")))?
            .to_string_lossy()
            .into_owned();

        let settings = Arc::new(Mutex::new(SettingsStore::load(
            data_dir.join("settings.json"),
            settings_defaults(&home),
        )));
        let ui = Arc::new(Mutex::new(UiStateStore::load(
            data_dir.join("ui-state.json"),
            ui_state_defaults(),
        )));
        let projects = ProjectsStore::load(data_dir.join("projects.json"));

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

        Ok(Self {
            settings,
            ui,
            projects: Mutex::new(projects),
            fs: Arc::new(FsService::new()),
            watchers,
            app: app.clone(),
            settings_writer,
            ui_writer,
        })
    }

    pub(crate) fn settings_store(&self) -> MutexGuard<'_, SettingsStore> {
        lock(&self.settings)
    }

    pub(crate) fn ui_store(&self) -> MutexGuard<'_, UiStateStore> {
        lock(&self.ui)
    }

    pub(crate) fn schedule_settings_write(&self) {
        self.settings_writer.schedule();
    }

    pub(crate) fn schedule_ui_write(&self) {
        self.ui_writer.schedule();
    }

    /// Persist debounced settings/ui updates; called on app exit so a quit
    /// shortly after a change cannot drop it.
    pub fn flush_pending_writes(&self) {
        for (label, result) in [
            ("settings", self.settings_writer.flush()),
            ("ui-state", self.ui_writer.flush()),
        ] {
            if let Err(error) = result {
                eprintln!("[ade-bridge] failed to flush {label} on exit: {error}");
            }
        }
    }

    pub fn bootstrap_payload(&self) -> BootstrapPayload {
        bootstrap_payload(self.settings_store().get(), platform_info())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

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
