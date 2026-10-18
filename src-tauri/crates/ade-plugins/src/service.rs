//! Service orchestration core: discovery/status projection, mutation-locked
//! install/remove/consent/enablement, lockfile state and batch restore, audit
//! tailing, the event-sink seam (spec §3.3/§3.9), and worker activation
//! orchestration (T11b2: `WorkerManager` wiring, host-call routing, command
//! invocation, plugin-event delivery, dev-watcher hot reload, idle reaping),
//! plus the T11b3 surface: panel entries + panel action flow (spec §3.6),
//! the global plugin view, and `workspace.openTab` dedupe/limits over the
//! same session registry. Scope-qualified instance keys (`global:<key>` /
//! `project:<repoId>:<key>`) are the unit of worker, session, and tab-tracker
//! identity, so a same-key dual-scope pair never disturbs its twin.
//!
//! Everything here is synchronous; the only processes this module spawns are
//! Git children for `git` install sources and Node workers on explicit
//! activation.
//!
//! ## Mutation serialization
//!
//! Every install-root mutation (install/publish/rollback/uninstall and the
//! project `.gitignore` seed) is serialized through a [`MutationLocks`] entry
//! keyed by the scope's install directory, and every per-plugin data mutation
//! (storage/settings/secrets set/delete) through an entry keyed by
//! `(data_dir, plugin_key)`. The acquisition order is always
//! **install-root → plugin-data**; a caller must never acquire an install-root
//! lock while holding a plugin-data lock. The two maps are independent, so
//! unrelated plugins and unrelated scopes never block each other, while a
//! concurrent uninstall and `secrets.set` for the same plugin cannot tear the
//! data directory.

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, Weak};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use crate::artifact::{
    resolve_declared_artifact, PLUGIN_PANEL_ENTRY_MAX_BYTES, PLUGIN_TAB_ENTRY_MAX_BYTES,
};
use crate::audit::{AuditLog, AuditOutcome, AuditRecord, LogLine};
use crate::consent::{fingerprint, scoped_key, ActivationState, ConsentKeyScope, ConsentState};
use crate::content_hash::hash_plugin_tree;
use crate::discovery::{discover, DiscoveredPlugin, DiscoveryProjection, PluginSourceKind};
use crate::enablement;
use crate::events::{is_plugin_event_name, project_payload};
use crate::git::{GitRunner, DEFAULT_PLUGIN_GIT_TIMEOUT};
use crate::host::gate;
use crate::host::methods::{self, HostCallContext, Outcome, PluginAuditSink};
use crate::host::services::{OpenTabResult, PluginHostServices};
use crate::install::lockfile::{read_lockfile, LockEntry, LockSource};
use crate::install::publication::publish_install;
use crate::install::staging::read_install_manifest;
use crate::install::{ensure_project_gitignore, install_from_local, uninstall};
use crate::install::{InstallContext, InstallResult};
use crate::integrity::{verify_installed, FsContentHasher, PluginContentVerifier};
use crate::manifest::engine_satisfies;
use crate::marketplace::installer::{MarketplaceInstaller, MarketplacePreview};
use crate::marketplace::service::{is_official_plugin_identity, MarketplaceService};
use crate::marketplace::TempWorkspace;
use crate::panel_shell::build_panel_shell_html;
use crate::paths::{global_roots, project_roots, ScopeRoots};
use crate::plugin_settings::PluginSettingsStore;
use crate::secrets::SecretStore;
use crate::sessions::{SessionBinding, SessionKind, SessionOwner, SessionRegistry};
use crate::storage::StorageStore;
use crate::types::{
    Capability, PluginError, PluginScope, INVALID_PARAMS, IO_ERROR, PANEL_NOT_FOUND, TAB_LIMIT,
    TAB_NOT_FOUND, UNAVAILABLE,
};
use crate::watcher::{DevWatcher, DEV_WATCH_BATCH_TRAILING_MS};
use crate::worker::manager::{
    FrameEventSink, HostCallHandler, WorkerManager, WorkerManagerConfig, UNKNOWN_COMMAND,
};
use crate::worker::spawn::{resolve_node, WorkerSpec};
use crate::worker::supervisor::{SupervisorConfig, WorkerState};

/// How many audit lines `get_logs` returns at most (merged: worker log ring
/// plus per-scope audit tails).
pub const PLUGIN_LOG_TAIL_LIMIT: usize = 200;

/// Bound of the in-memory worker log ring (spec §3.2 log frames).
pub const WORKER_LOG_RING_LIMIT: usize = 1000;

/// Main-side panel-action admission window (per session token).
pub const PANEL_ACTION_WINDOW: Duration = Duration::from_secs(10);
/// Maximum panel actions per token inside [`PANEL_ACTION_WINDOW`].
pub const PANEL_ACTION_MAX_PER_WINDOW: usize = 30;
/// Maximum serialized panel-action params (spec §3.5: ≤64KiB).
pub const PANEL_ACTION_PARAMS_MAX_BYTES: usize = 64 * 1024;
/// Maximum live plugin-tab instances per plugin instance per worktree.
pub const OPEN_TAB_MAX_INSTANCES: usize = 8;
/// Maximum new plugin tabs per plugin instance per worktree per window.
pub const OPEN_TAB_RATE_MAX: usize = 4;
/// Window for [`OPEN_TAB_RATE_MAX`] (spec §3.5: ≤4 per 10s).
pub const OPEN_TAB_RATE_WINDOW: Duration = Duration::from_secs(10);

/// Wire shape of `readPanelEntry` / the global plugin view: the shell HTML
/// plus the renderer session token that authorizes subsequent
/// `panel_action` calls.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PanelEntry {
    pub html: String,
    pub session_token: String,
}

/// One live main-area plugin tab instance, keyed for `(tabId, params)` dedupe.
#[derive(Debug, Clone)]
struct TrackedTab {
    tab_id: String,
    params: Value,
    instance_id: String,
}

/// Per `(plugin instance, worktree)` tab bookkeeping: live instances and the
/// rolling window of new-tab opens used for rate limiting.
#[derive(Debug, Default)]
struct TabTracker {
    tabs: Vec<TrackedTab>,
    opens: VecDeque<Instant>,
}

/// Per-token panel-action admission (size + rate), held main-side on top of
/// the renderer budgets. A token's bucket is pruned lazily so the map cannot
/// grow past the token's window of use.
#[derive(Debug, Default)]
struct PanelAdmission {
    hits: Mutex<HashMap<String, VecDeque<Instant>>>,
}

impl PanelAdmission {
    fn admit(&self, token: &str, params_bytes: usize) -> Result<(), PluginError> {
        if params_bytes > PANEL_ACTION_PARAMS_MAX_BYTES {
            return Err(PluginError::new(
                INVALID_PARAMS,
                format!("params must not exceed {PANEL_ACTION_PARAMS_MAX_BYTES} bytes"),
            ));
        }
        let now = Instant::now();
        let mut hits = self
            .hits
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        hits.retain(|_, times| {
            while times
                .front()
                .is_some_and(|time| now.duration_since(*time) >= PANEL_ACTION_WINDOW)
            {
                times.pop_front();
            }
            !times.is_empty()
        });
        let times = hits.entry(token.to_string()).or_default();
        if times.len() >= PANEL_ACTION_MAX_PER_WINDOW {
            return Err(PluginError::new(
                gate::RATE_LIMITED,
                format!("panel action rate limit exceeded ({PANEL_ACTION_MAX_PER_WINDOW} per 10s)"),
            ));
        }
        times.push_back(now);
        Ok(())
    }
}

/// Push budget window per session token (spec §3.4).
pub const PANEL_PUSH_WINDOW: Duration = Duration::from_secs(10);
/// Maximum pushes per token inside [`PANEL_PUSH_WINDOW`].
pub const PANEL_PUSH_MAX_PER_WINDOW: usize = 30;
/// Maximum serialized push payload (spec §3.4: ≤64KiB).
pub const PANEL_PUSH_PAYLOAD_MAX_BYTES: usize = 64 * 1024;

#[derive(Debug, Default)]
struct PushAdmission {
    hits: Mutex<HashMap<String, VecDeque<Instant>>>,
}

impl PushAdmission {
    fn admit(&self, token: &str, payload_bytes: usize) -> Result<(), PluginError> {
        if payload_bytes > PANEL_PUSH_PAYLOAD_MAX_BYTES {
            return Err(PluginError::new(
                INVALID_PARAMS,
                format!("payload must not exceed {PANEL_PUSH_PAYLOAD_MAX_BYTES} bytes"),
            ));
        }
        let now = Instant::now();
        let mut hits = self
            .hits
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        hits.retain(|_, times| {
            while times
                .front()
                .is_some_and(|time| now.duration_since(*time) >= PANEL_PUSH_WINDOW)
            {
                times.pop_front();
            }
            !times.is_empty()
        });
        let times = hits.entry(token.to_string()).or_default();
        if times.len() >= PANEL_PUSH_MAX_PER_WINDOW {
            return Err(PluginError::new(
                gate::RATE_LIMITED,
                format!("push rate limit exceeded ({PANEL_PUSH_MAX_PER_WINDOW} per 10s)"),
            ));
        }
        times.push_back(now);
        Ok(())
    }
}

/// Locates one plugin instance across the dual-scope model.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginRef {
    pub plugin_key: String,
    pub scope: PluginScope,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repo_id: Option<String>,
}

/// Install/deployment target: scope plus the owning project for project scope.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScopeTarget {
    pub scope: PluginScope,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repo_id: Option<String>,
}

/// Where an install comes from. `Bundled` seeds bypass this path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InstallSource {
    LocalPath {
        path: PathBuf,
    },
    Git {
        url: String,
        ref_: String,
    },
    Marketplace {
        source_id: String,
        plugin_key: String,
    },
}

/// The consent-dialog answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ConsentDecision {
    Approve,
    Decline,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConsentRequest {
    pub plugin_ref: PluginRef,
    pub reviewed_fingerprint: String,
    pub decision: ConsentDecision,
}

/// Projected plugin lifecycle state. `running`/`restarting`/`errored` are
/// only produced once workers are wired (T11b2); this core emits
/// `disabled|pending|invalid|idle`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PluginStatus {
    Running,
    Restarting,
    Idle,
    Pending,
    Disabled,
    Errored,
    Invalid,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginCapabilityProjection {
    pub kind: String,
    pub description: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginPanelProjection {
    pub id: String,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    pub tab_key: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginTabProjection {
    pub id: String,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginKeybindingProjection {
    pub key: String,
    pub when: crate::manifest::CommandContext,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum PluginCommandHandler {
    BuiltIn { action: String },
    Worker,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginCommandProjection {
    pub id: String,
    pub title: String,
    pub context: crate::manifest::CommandContext,
    pub handler: PluginCommandHandler,
    pub keybindings: Vec<PluginKeybindingProjection>,
    pub visibility: crate::manifest::CommandVisibility,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginSourceProjection {
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reference: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolved_commit: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content_hash: Option<String>,
}

/// Wire shape of `plugins:list` (spec §3.9 / T13).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginListEntry {
    pub plugin_key: String,
    pub scope: PluginScope,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repo_id: Option<String>,
    pub name: String,
    pub version: String,
    pub publisher: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub status: PluginStatus,
    pub needs_reconsent: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    pub is_dev: bool,
    pub official: bool,
    pub bundled: bool,
    pub capabilities: Vec<PluginCapabilityProjection>,
    pub panels: Vec<PluginPanelProjection>,
    pub tabs: Vec<PluginTabProjection>,
    pub commands: Vec<PluginCommandProjection>,
    pub has_worker: bool,
    pub restarts: u32,
    pub source: PluginSourceProjection,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub install_hash: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub consent_fingerprint: Option<String>,
}

/// Classified state of one project lockfile entry (spec §3.9/T13).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum LockEntryStatus {
    Installed,
    Missing,
    HashMismatch,
    SourceUnavailable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LockStateEntry {
    pub plugin_key: String,
    pub version: String,
    pub source: LockSource,
    pub content_hash: String,
    pub status: LockEntryStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LockState {
    pub repo_id: String,
    pub lockfile_path: String,
    pub entries: Vec<LockStateEntry>,
}

/// One language-pack catalog an approved plugin contributes (wire shape of
/// `plugins:list-language-packs`, TS `PluginLanguagePackRegistration`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LanguagePackRegistration {
    pub id: String,
    /// i18next resource id: `plugin` + the UTF-16 hex of `id` (mirrors the
    /// renderer's `pluginLanguageResourceId` so both sides agree byte-for-byte).
    pub resource_language: String,
    pub plugin_key: String,
    pub locale: String,
    pub catalog: Value,
}

/// Renderer-compatible resource language id for a `plugin:<key>:<locale>` id.
pub fn plugin_language_resource_id(id: &str) -> String {
    let mut encoded = String::from("plugin");
    for unit in id.encode_utf16() {
        encoded.push_str(&format!("{unit:04x}"));
    }
    encoded
}

/// Payload of the `plugins:open-tab` bridge event (emitted by T11b2+).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenTabRequest {
    pub plugin_key: String,
    pub tab_id: String,
    pub instance_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default)]
    pub params: Value,
    pub worktree_id: String,
    pub scope: PluginScope,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repo_id: Option<String>,
}

/// Payload of the `plugins:panel-push` bridge event (spec §3.3).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PanelPushRequest {
    pub session_token: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub topic: Option<String>,
    pub payload: Value,
}

/// Outbound seam to the host application (mirrors the automations service
/// sink). Implemented by ade-bridge over Tauri events.
pub trait ServiceEventSink: Send + Sync {
    fn plugins_changed(&self, content_packs_changed: bool, plugins_changed: bool);
    fn open_tab_requested(&self, request: OpenTabRequest);
    fn panel_push_requested(&self, request: PanelPushRequest);
}

/// Worker-pool tunables: pool size, supervision timeouts, dev-watch batching.
/// Defaults mirror spec §3.2/§3.8; tests inject short timeouts.
#[derive(Debug, Clone)]
pub struct WorkerRuntimeConfig {
    /// Maximum number of workers with a live process (spec §3.2: 5).
    pub max_active: usize,
    pub supervisor: SupervisorConfig,
    /// Trailing-edge debounce of the dev watcher (spec §3.8: 150/500ms).
    pub watch_batch: Duration,
}

impl Default for WorkerRuntimeConfig {
    fn default() -> Self {
        Self {
            max_active: 5,
            supervisor: SupervisorConfig::default(),
            watch_batch: Duration::from_millis(DEV_WATCH_BATCH_TRAILING_MS),
        }
    }
}

/// Injected dependencies of [`PluginService`]. Storage/settings/audit stores
/// are derived from `app_data_dir` by the service itself, so only the
/// main-owned pieces are injected.
pub struct ServiceConfig {
    pub app_data_dir: PathBuf,
    pub dev_paths: Vec<PathBuf>,
    pub projects: Vec<(String, PathBuf)>,
    pub host_entry_path: PathBuf,
    pub node_override: Option<String>,
    pub consent_state: Arc<dyn ConsentState>,
    pub secrets: Arc<dyn SecretStore>,
    pub host_services: Arc<dyn PluginHostServices>,
    pub event_sink: Arc<dyn ServiceEventSink>,
    pub host_version: String,
    pub workers: WorkerRuntimeConfig,
}

/// `(data_dir, plugin_key)` identity of a per-plugin data lock.
type PluginDataKey = (PathBuf, String);
/// Shared mutex handed out by [`MutationLocks`].
type MutationLock = Arc<Mutex<()>>;

/// Mutex map keyed by install root and by `(data_dir, plugin_key)`.
///
/// The maps are intentionally never pruned: removing an entry while another
/// thread still queued on it would allow a third acquisition to create a fresh
/// mutex and break mutual exclusion.
#[derive(Debug, Default)]
pub struct MutationLocks {
    install_roots: Mutex<HashMap<PathBuf, MutationLock>>,
    plugin_data: Mutex<HashMap<PluginDataKey, MutationLock>>,
}

impl MutationLocks {
    /// Runs `f` while holding the install-root lock. Acquisition order:
    /// install-root first, plugin-data second (see module docs).
    pub fn with_install_root<T>(&self, install_root: &Path, f: impl FnOnce() -> T) -> T {
        let lock = {
            let mut map = self
                .install_roots
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            map.entry(install_root.to_path_buf()).or_default().clone()
        };
        let guard = lock.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let result = f();
        drop(guard);
        result
    }

    /// Runs `f` while holding the per-plugin data lock for `(data_dir, key)`.
    pub fn with_plugin_data<T>(
        &self,
        data_dir: &Path,
        plugin_key: &str,
        f: impl FnOnce() -> T,
    ) -> T {
        let lock = {
            let mut map = self
                .plugin_data
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            map.entry((data_dir.to_path_buf(), plugin_key.to_string()))
                .or_default()
                .clone()
        };
        let guard = lock.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let result = f();
        drop(guard);
        result
    }
}

/// Activation facts of one worker, captured at `ensure_active` time and read
/// back by the host-call handler, log ring, and status projection.
#[derive(Debug, Clone)]
struct WorkerRecord {
    /// Scope-qualified instance identity: the `WorkerManager` pool key and
    /// the `WorkerBridge` lookup key.
    instance_key: String,
    plugin_key: String,
    scope: PluginScope,
    repo_id: Option<String>,
    data_dir: PathBuf,
    granted: Vec<Capability>,
    revision: String,
    /// Last fatal-frame error (kept until the worker is stopped/replaced).
    error: Option<String>,
}

/// Shared seam between [`PluginService`] and the [`WorkerManager`]: routes
/// child `hostCall` frames through `host::methods::execute` with the activation
/// context bound to the calling worker's scope-qualified instance key, and
/// buffers log/fatal frames. It never touches the manager itself, so it is safe
/// to call while the manager lock is held.
struct WorkerBridge {
    records: Mutex<HashMap<String, WorkerRecord>>,
    logs: Mutex<VecDeque<LogLine>>,
    /// Per-instance dynamic `events.subscribe` sets, recorded when a worker's
    /// host call succeeds. Delivered only while that worker is running.
    dynamic_subs: Mutex<HashMap<String, Vec<String>>>,
    services: Arc<dyn PluginHostServices>,
    sink: Arc<dyn ServiceEventSink>,
}

impl WorkerBridge {
    fn new(services: Arc<dyn PluginHostServices>, sink: Arc<dyn ServiceEventSink>) -> Self {
        Self {
            records: Mutex::new(HashMap::new()),
            logs: Mutex::new(VecDeque::new()),
            dynamic_subs: Mutex::new(HashMap::new()),
            services,
            sink,
        }
    }

    fn lock_dynamic_subs(&self) -> std::sync::MutexGuard<'_, HashMap<String, Vec<String>>> {
        self.dynamic_subs
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn record_dynamic_subs(&self, instance_key: &str, events: Vec<String>) {
        self.lock_dynamic_subs()
            .insert(instance_key.to_string(), events);
    }

    fn dynamic_subscribed(&self, instance_key: &str, event: &str) -> bool {
        self.lock_dynamic_subs()
            .get(instance_key)
            .is_some_and(|events| events.iter().any(|entry| entry == event))
    }

    fn clear_dynamic_subs(&self, instance_key: &str) {
        self.lock_dynamic_subs().remove(instance_key);
    }

    fn lock_records(&self) -> std::sync::MutexGuard<'_, HashMap<String, WorkerRecord>> {
        self.records
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn push_log(&self, record: AuditRecord) {
        let mut logs = self
            .logs
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        logs.push_back(record);
        while logs.len() > WORKER_LOG_RING_LIMIT {
            logs.pop_front();
        }
    }

    fn log_lines(&self, plugin_key: &str) -> Vec<LogLine> {
        let actor = format!("plugin:{plugin_key}");
        self.logs
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .iter()
            .filter(|line| line.actor == actor)
            .cloned()
            .collect()
    }
}

/// Per-scope audit sink bound for the duration of one host call.
struct ScopeAudit {
    log: AuditLog,
}

impl PluginAuditSink for ScopeAudit {
    fn record(&self, record: AuditRecord) -> Result<(), PluginError> {
        self.log.record(record)
    }
}

impl HostCallHandler for WorkerBridge {
    fn handle(
        &self,
        instance_key: &str,
        _call_id: u64,
        method: &str,
        params: Value,
    ) -> Result<Value, (String, String)> {
        let Some(record) = self.lock_records().get(instance_key).cloned() else {
            return Err((
                UNAVAILABLE.to_owned(),
                format!("plugin worker {instance_key} is not active"),
            ));
        };
        let audit = ScopeAudit {
            log: AuditLog::new(&record.data_dir),
        };
        let ctx = HostCallContext {
            plugin_key: &record.plugin_key,
            granted: Some(&record.granted),
            via_panel: false,
            session_token: None,
            scope: record.scope,
            repo_id: record.repo_id.as_deref(),
            worktree_hint: None,
            services: &*self.services,
            audit: Some(&audit),
        };
        let outcome = methods::execute(method, params, &ctx);
        if method == "events.subscribe" {
            if let Outcome::Ok(value) = &outcome {
                let events: Vec<String> = value
                    .get("subscribed")
                    .and_then(Value::as_array)
                    .map(|events| {
                        events
                            .iter()
                            .filter_map(|event| event.as_str().map(str::to_string))
                            .collect()
                    })
                    .unwrap_or_default();
                self.record_dynamic_subs(instance_key, events);
            }
        }
        match outcome {
            Outcome::Ok(value) => Ok(value),
            Outcome::Err { code, error } => Err((code, error)),
        }
    }
}

impl FrameEventSink for WorkerBridge {
    fn on_log(&self, instance_key: &str, level: &str, message: &str) {
        let plugin_key = self
            .lock_records()
            .get(instance_key)
            .map(|record| record.plugin_key.clone())
            .unwrap_or_else(|| instance_key.to_string());
        self.push_log(AuditRecord {
            ts: now_millis() as i64,
            actor: format!("plugin:{plugin_key}"),
            method: format!("worker.{level}"),
            summary: message.to_string(),
            outcome: AuditOutcome::Ok,
        });
    }

    fn on_fatal(&self, instance_key: &str, error: &str) {
        let plugin_key = if let Some(record) = self.lock_records().get_mut(instance_key) {
            record.error = Some(error.to_string());
            record.plugin_key.clone()
        } else {
            instance_key.to_string()
        };
        self.push_log(AuditRecord {
            ts: now_millis() as i64,
            actor: format!("plugin:{plugin_key}"),
            method: "worker.fatal".to_string(),
            summary: error.to_string(),
            outcome: AuditOutcome::Error,
        });
        self.sink.plugins_changed(false, true);
    }
}

/// Orchestrates discovery, projection, installs, consent, lockfile restore,
/// audit reads, and worker activation for both scopes.
pub struct PluginService {
    config: ServiceConfig,
    global: ScopeRoots,
    projects: Mutex<Vec<(String, PathBuf)>>,
    locks: MutationLocks,
    audit: AuditLog,
    storage: StorageStore,
    settings: PluginSettingsStore,
    marketplace: MarketplaceInstaller,
    git: GitRunner,
    bridge: Arc<WorkerBridge>,
    manager: Mutex<WorkerManager>,
    /// Lazy content-integrity verifier for installed worker payloads: each
    /// immutable `(plugin_key, version_dir, content_hash)` identity is hashed
    /// exactly once per process, before the first worker start (spec
    /// §3.1/§3.5; oracle `PluginContentVerifier` parity).
    verifier: Mutex<PluginContentVerifier<FsContentHasher>>,
    sessions: SessionRegistry,
    /// `(instance_key, worktree_id) -> tabs` dedupe/limits tracked main-side
    /// for `workspace.openTab`; pruned on worktree removal and deactivation.
    tab_trackers: Mutex<HashMap<(String, String), TabTracker>>,
    panel_admission: PanelAdmission,
    push_admission: PushAdmission,
    watcher: Mutex<Option<DevWatcher>>,
    next_call_id: AtomicU64,
}

impl PluginService {
    pub fn new(config: ServiceConfig) -> Arc<Self> {
        let global = global_roots(&config.app_data_dir);
        let data_dir = global.data_dir.clone();
        let audit = AuditLog::new(&data_dir);
        let storage = StorageStore::new(&data_dir);
        let settings = PluginSettingsStore::new(&data_dir);
        let marketplace_service = MarketplaceService::with_runner(
            &config.app_data_dir.join("plugins-data"),
            GitRunner::new(DEFAULT_PLUGIN_GIT_TIMEOUT),
        );
        let marketplace =
            MarketplaceInstaller::new(marketplace_service, config.host_version.clone());
        let projects = config.projects.clone();
        let bridge = Arc::new(WorkerBridge::new(
            Arc::clone(&config.host_services),
            Arc::clone(&config.event_sink),
        ));
        // `WorkerManager` resolves the node path once; when resolution fails
        // here the manager stores a terminal `node_missing` error that every
        // `ensure_active` reports.
        let node = resolve_node(config.node_override.as_deref(), None)
            .unwrap_or_else(|_| PathBuf::from("__ade_missing_node__"));
        let manager = WorkerManager::new(
            WorkerManagerConfig {
                max_active: config.workers.max_active,
                host_entry_path: config.host_entry_path.clone(),
                node,
                supervisor: config.workers.supervisor.clone(),
            },
            Arc::clone(&bridge) as Arc<dyn HostCallHandler>,
            Arc::clone(&bridge) as Arc<dyn FrameEventSink>,
        );
        Arc::new(Self {
            config,
            global,
            projects: Mutex::new(projects),
            locks: MutationLocks::default(),
            audit,
            storage,
            settings,
            marketplace,
            git: GitRunner::new(DEFAULT_PLUGIN_GIT_TIMEOUT),
            bridge,
            manager: Mutex::new(manager),
            verifier: Mutex::new(PluginContentVerifier::new(FsContentHasher)),
            sessions: SessionRegistry::new(),
            tab_trackers: Mutex::new(HashMap::new()),
            panel_admission: PanelAdmission::default(),
            push_admission: PushAdmission::default(),
            watcher: Mutex::new(None),
            next_call_id: AtomicU64::new(1),
        })
    }

    /// Replaces the registered project set and reconciles project-scope
    /// activation: workers of removed projects are stopped and their sessions
    /// invalidated, and approved+enabled project plugins of the current set
    /// are (re)activated (spec §3.3 lifecycle).
    pub fn set_projects(&self, projects: Vec<(String, PathBuf)>) {
        let (removed, previous): (Vec<String>, Vec<(String, PathBuf)>) = {
            let mut current = self
                .projects
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let next: HashSet<String> = projects.iter().map(|(id, _)| id.clone()).collect();
            let removed = current
                .iter()
                .filter(|(id, _)| !next.contains(id))
                .map(|(id, _)| id.clone())
                .collect();
            let previous = current.clone();
            *current = projects;
            (removed, previous)
        };
        for repo_id in removed {
            // Sessions (panel/tab/global-view) can exist without a live
            // worker, so invalidate by every key the removed repo ever
            // installed — from the pre-removal discovery and from live worker
            // records — scoped to exactly this repo so a same-key global twin
            // keeps its sessions.
            let mut plugin_keys: Vec<String> =
                discover(&self.global, &previous, &self.config.dev_paths)
                    .into_iter()
                    .filter(|entry| {
                        entry.scope == PluginScope::Project
                            && entry.repo_id.as_deref() == Some(repo_id.as_str())
                    })
                    .filter_map(|entry| entry.key)
                    .collect();
            let instances: Vec<String> = self
                .bridge
                .lock_records()
                .iter()
                .filter(|(_, record)| {
                    record.scope == PluginScope::Project
                        && record.repo_id.as_deref() == Some(repo_id.as_str())
                })
                .map(|(instance_key, record)| {
                    if !plugin_keys.contains(&record.plugin_key) {
                        plugin_keys.push(record.plugin_key.clone());
                    }
                    instance_key.clone()
                })
                .collect();
            if instances.is_empty() && plugin_keys.is_empty() {
                continue;
            }
            {
                let mut manager = self.manager();
                for instance_key in &instances {
                    manager.stop(instance_key);
                }
            }
            for instance_key in &instances {
                self.bridge.lock_records().remove(instance_key);
                self.bridge.clear_dynamic_subs(instance_key);
                self.prune_tab_instance(instance_key);
            }
            for plugin_key in &plugin_keys {
                self.sessions.invalidate_plugin_scope(
                    plugin_key,
                    PluginScope::Project,
                    Some(repo_id.as_str()),
                );
                if let Ok(instance_key) = instance_key_of(&PluginRef {
                    plugin_key: plugin_key.clone(),
                    scope: PluginScope::Project,
                    repo_id: Some(repo_id.clone()),
                }) {
                    self.prune_tab_instance(&instance_key);
                }
            }
            self.config.event_sink.plugins_changed(false, true);
        }
        for entry in self.list() {
            if entry.scope == PluginScope::Project {
                let _ = self.ensure_active(&PluginRef {
                    plugin_key: entry.plugin_key.clone(),
                    scope: entry.scope,
                    repo_id: entry.repo_id.clone(),
                });
            }
        }
    }

    /// Starts the service: activates the approved+enabled global plugins and
    /// installs the dev-path watcher (hot reload on manifest revision change).
    pub fn start(self: &Arc<Self>) -> Result<(), PluginError> {
        for entry in self.list() {
            if entry.scope == PluginScope::Global {
                let _ = self.ensure_active(&PluginRef {
                    plugin_key: entry.plugin_key.clone(),
                    scope: entry.scope,
                    repo_id: entry.repo_id.clone(),
                });
            }
        }
        let dev_paths = self.config.dev_paths.clone();
        if dev_paths.is_empty() {
            return Ok(());
        }
        let weak: Weak<PluginService> = Arc::downgrade(self);
        let mut watcher = DevWatcher::new(&dev_paths, self.config.workers.watch_batch);
        watcher.start(move |paths| {
            if let Some(service) = weak.upgrade() {
                service.on_dev_change(paths);
            }
        })?;
        *self
            .watcher
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(watcher);
        Ok(())
    }

    /// Stops the dev watcher, every worker, and every panel/tab session.
    pub fn shutdown(&self) {
        if let Some(mut watcher) = self
            .watcher
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .take()
        {
            watcher.stop();
        }
        self.manager().stop_all();
        self.bridge.lock_records().clear();
        self.bridge.lock_dynamic_subs().clear();
        self.tab_trackers
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clear();
        self.sessions.sweep(0);
    }

    /// One orchestration tick: drains worker frames (host calls, logs, fatal
    /// frames), reaps workers idle past the supervisor's `idle_timeout`, and
    /// bounds the session registry.
    pub fn pump(&self) {
        let reaped = {
            let mut manager = self.manager();
            let _ = manager.pump(None);
            manager.reap_idle()
        };
        if !reaped.is_empty() {
            for instance_key in &reaped {
                let binding = self.bridge.lock_records().get(instance_key).map(|record| {
                    (
                        record.plugin_key.clone(),
                        record.scope,
                        record.repo_id.clone(),
                    )
                });
                self.bridge.clear_dynamic_subs(instance_key);
                self.prune_tab_instance(instance_key);
                if let Some((plugin_key, scope, repo_id)) = binding {
                    self.sessions
                        .invalidate_plugin_scope(&plugin_key, scope, repo_id.as_deref());
                }
            }
            self.config.event_sink.plugins_changed(false, true);
        }
        self.sessions.sweep(crate::sessions::DEFAULT_SESSION_CAP);
    }

    /// Lazily activates the worker of `plugin_ref`. Only an approved plugin
    /// activates; every other state stops any stale worker and fails. The
    /// worker spec carries the manifest's declared worker commands, the
    /// manifest's granted capabilities, and the manifest hash as revision
    /// (a revision change restarts a running worker via the manager).
    pub fn ensure_active(&self, plugin_ref: &PluginRef) -> Result<(), PluginError> {
        let (spec, record) = match self.worker_spec_for(plugin_ref) {
            Ok(pair) => pair,
            Err(error) => {
                self.deactivate_ref(plugin_ref);
                return Err(error);
            }
        };
        self.bridge
            .lock_records()
            .insert(record.instance_key.clone(), record);
        self.manager().ensure_active(spec)
    }

    /// Invokes a manifest-declared worker command (built-in actions are
    /// renderer-side and rejected here), activating the worker lazily.
    pub fn invoke_command(
        &self,
        plugin_ref: &PluginRef,
        command_id: &str,
        args: Option<Value>,
    ) -> Result<Value, PluginError> {
        self.invoke_command_with_session(plugin_ref, command_id, args, None)
    }

    /// [`Self::invoke_command`] with an optional UI session token attached to
    /// the worker frame, letting the command push progress back through
    /// `session.post`.
    pub fn invoke_command_with_session(
        &self,
        plugin_ref: &PluginRef,
        command_id: &str,
        args: Option<Value>,
        session_token: Option<&str>,
    ) -> Result<Value, PluginError> {
        let discovered = self.discover_all();
        let entry = find_discovered(&discovered, plugin_ref).ok_or_else(|| {
            PluginError::new(
                UNAVAILABLE,
                format!(
                    "plugin {} is not installed or is invalid",
                    plugin_ref.plugin_key
                ),
            )
        })?;
        let manifest = entry
            .manifest
            .as_ref()
            .expect("valid entries carry a manifest");
        let command = manifest
            .contributes
            .commands
            .iter()
            .find(|command| command.id == command_id)
            .ok_or_else(|| {
                PluginError::new(
                    UNKNOWN_COMMAND,
                    format!(
                        "plugin {} does not declare command {command_id}",
                        plugin_ref.plugin_key
                    ),
                )
            })?;
        if command.action.is_some() {
            return Err(PluginError::new(
                INVALID_PARAMS,
                format!("command {command_id} is a built-in action, not a worker command"),
            ));
        }
        self.ensure_active(plugin_ref)?;
        let instance_key = instance_key_of(plugin_ref)?;
        let call_id = self.next_call_id.fetch_add(1, Ordering::SeqCst);
        self.manager().invoke_command(
            &instance_key,
            call_id,
            command_id,
            args,
            session_token.map(|token| crate::worker::protocol::SessionRef {
                token: token.to_string(),
            }),
        )
    }

    /// Projects `raw_payload` for the plugin event `name` and delivers it to
    /// every approved+enabled subscriber (static `events` contributions get
    /// their worker activated lazily; dynamic subscriptions only reach a
    /// running worker). Returns the number of delivered events.
    pub fn deliver_plugin_event(&self, name: &str, raw_payload: Value) -> usize {
        if !is_plugin_event_name(name) {
            return 0;
        }
        let Some(payload) = project_payload(name, &raw_payload) else {
            return 0;
        };
        let discovered = self.discover_all();
        let entries = self.project_entries(&discovered);
        let mut delivered = 0;
        for entry in &entries {
            if entry.status != PluginStatus::Idle {
                continue;
            }
            let plugin_ref = PluginRef {
                plugin_key: entry.plugin_key.clone(),
                scope: entry.scope,
                repo_id: entry.repo_id.clone(),
            };
            let Ok(instance_key) = instance_key_of(&plugin_ref) else {
                continue;
            };
            let static_subscription = find_discovered(&discovered, &plugin_ref)
                .and_then(|entry| entry.manifest.as_ref())
                .is_some_and(|manifest| {
                    manifest
                        .contributes
                        .events
                        .iter()
                        .any(|contribution| contribution.on == name)
                });
            let ready = if static_subscription {
                self.ensure_active(&plugin_ref).is_ok()
            } else {
                // Dynamic subscriptions are recorded per instance when the
                // worker's `events.subscribe` host call succeeds; a stopped
                // worker keeps no live subscription.
                self.bridge.dynamic_subscribed(&instance_key, name)
                    && matches!(
                        self.manager().state(&instance_key),
                        WorkerState::Running { .. }
                    )
            };
            if !ready {
                continue;
            }
            let event_id = self.next_call_id.fetch_add(1, Ordering::SeqCst);
            if self
                .manager()
                .deliver_event(&instance_key, event_id, name, payload.clone())
                .is_ok()
            {
                delivered += 1;
            }
        }
        delivered
    }

    /// Convenience entry point for `worktree.created`.
    pub fn on_worktree_created(&self, payload: crate::events::WorktreeCreatedPayload) -> usize {
        self.deliver_plugin_event(
            "worktree.created",
            serde_json::to_value(payload).unwrap_or(Value::Null),
        )
    }

    /// Convenience entry point for `worktree.removed`: drops the tab
    /// dedupe/limit bookkeeping of that worktree before delivering.
    pub fn on_worktree_removed(&self, payload: crate::events::WorktreeRemovedPayload) -> usize {
        self.prune_tab_worktree(&payload.worktree_id);
        self.deliver_plugin_event(
            "worktree.removed",
            serde_json::to_value(payload).unwrap_or(Value::Null),
        )
    }

    /// Convenience entry point for `agent.status.changed`.
    pub fn on_agent_status_changed(
        &self,
        payload: crate::events::AgentStatusChangedPayload,
    ) -> usize {
        self.deliver_plugin_event(
            "agent.status.changed",
            serde_json::to_value(payload).unwrap_or(Value::Null),
        )
    }

    /// Discovery plus status projection for every registered context
    /// (spec §3.3 masking: a project entry hides the same-key global entry in
    /// its own repo context; with several repos a global stays listed while at
    /// least one context can still see it).
    pub fn list(&self) -> Vec<PluginListEntry> {
        let discovered = self.discover_all();
        let mut entries = self.project_entries(&discovered);
        self.project_runtime_status(&mut entries);
        entries
    }

    /// Installs `source` into `target`, serialized per install root.
    pub fn install(
        &self,
        source: InstallSource,
        target: ScopeTarget,
    ) -> Result<InstallResult, PluginError> {
        let roots = self.roots_for_target(&target)?;
        let install_root = roots.install_dir.clone();
        self.locks
            .with_install_root(&install_root, || self.install_locked(source, target, roots))
    }

    /// Uninstalls the referenced plugin (install directory, data directory,
    /// lock entry), holding the install-root and plugin-data locks.
    pub fn remove(&self, plugin_ref: PluginRef) -> Result<Vec<PluginListEntry>, PluginError> {
        let roots = self.roots_for_ref(&plugin_ref)?;
        self.deactivate_ref(&plugin_ref);
        let plugin_key = plugin_ref.plugin_key.clone();
        self.locks.with_install_root(&roots.install_dir, || {
            self.locks
                .with_plugin_data(&roots.data_dir, &plugin_key, || {
                    uninstall(&plugin_key, &roots)
                })
        })?;
        // Full uninstall: unlike scope-limited deactivation, no session of
        // this key can survive in any scope.
        self.sessions.invalidate_plugin(&plugin_key);
        self.audit(
            &roots.data_dir,
            &plugin_key,
            "plugins.remove",
            "uninstalled",
        );
        self.config.event_sink.plugins_changed(false, true);
        Ok(self.list())
    }

    /// Records the consent-dialog answer after re-verifying the reviewed
    /// fingerprint against the current bytes.
    pub fn consent(&self, request: ConsentRequest) -> Result<Vec<PluginListEntry>, PluginError> {
        let plugin_ref = request.plugin_ref;
        let current = self.current_fingerprint(&plugin_ref)?;
        let consent_scope = scope_key_scope(&plugin_ref)?;
        let scoped = scoped_key(&plugin_ref.plugin_key, consent_scope)
            .ok_or_else(|| PluginError::new(INVALID_PARAMS, "invalid plugin scope for consent"))?;
        match request.decision {
            ConsentDecision::Approve => enablement::approve(
                &*self.config.consent_state,
                &scoped,
                &request.reviewed_fingerprint,
                &current,
            )?,
            ConsentDecision::Decline => {
                enablement::decline(&*self.config.consent_state, &scoped);
                self.deactivate_ref(&plugin_ref);
            }
        }
        let data_dir = self.data_dir_for_ref(&plugin_ref)?;
        self.audit(
            &data_dir,
            &plugin_ref.plugin_key,
            "plugins.consent",
            match request.decision {
                ConsentDecision::Approve => "approved",
                ConsentDecision::Decline => "declined",
            },
        );
        self.config.event_sink.plugins_changed(false, true);
        Ok(self.list())
    }

    /// Flips the disable bit; consent still gates activation.
    pub fn set_enabled(
        &self,
        plugin_ref: PluginRef,
        enabled: bool,
    ) -> Result<Vec<PluginListEntry>, PluginError> {
        let consent_scope = scope_key_scope(&plugin_ref)?;
        let scoped = scoped_key(&plugin_ref.plugin_key, consent_scope).ok_or_else(|| {
            PluginError::new(INVALID_PARAMS, "invalid plugin scope for enablement")
        })?;
        enablement::set_enabled(&*self.config.consent_state, &scoped, enabled);
        if !enabled {
            self.deactivate_ref(&plugin_ref);
        }
        let data_dir = self.data_dir_for_ref(&plugin_ref)?;
        self.audit(
            &data_dir,
            &plugin_ref.plugin_key,
            "plugins.set_enabled",
            if enabled { "enabled" } else { "disabled" },
        );
        self.config.event_sink.plugins_changed(false, true);
        Ok(self.list())
    }

    /// Re-discovers and re-projects (settings edits land in T11b2/T12).
    pub fn refresh(&self) -> Vec<PluginListEntry> {
        self.list()
    }

    /// Reads a declared panel entry, wraps it in the host CSP shell, and
    /// issues a renderer session token bound to that panel + manifest
    /// revision + owner window (spec §3.6). Requires approved consent and,
    /// for installed trees, a passing content-integrity check.
    pub fn panel_entry(
        &self,
        plugin_ref: &PluginRef,
        panel_id: &str,
        owner_window: &str,
    ) -> Result<PanelEntry, PluginError> {
        self.panel_entry_for(plugin_ref, panel_id, SessionKind::Panel, owner_window)
    }

    /// The global plugin view (left rail): the first declared panel of the
    /// global instance of `plugin_key` wrapped in the same shell/session
    /// contract as `panel_entry`.
    pub fn global_view_entry(
        &self,
        plugin_key: &str,
        owner_window: &str,
    ) -> Result<PanelEntry, PluginError> {
        let plugin_ref = PluginRef {
            plugin_key: plugin_key.to_string(),
            scope: PluginScope::Global,
            repo_id: None,
        };
        let discovered = self.discover_all();
        let entry = find_discovered(&discovered, &plugin_ref).ok_or_else(|| {
            PluginError::new(
                UNAVAILABLE,
                format!("plugin {plugin_key} is not installed or is invalid"),
            )
        })?;
        let manifest = entry
            .manifest
            .as_ref()
            .expect("valid entries carry a manifest");
        let first = manifest
            .contributes
            .panels
            .first()
            .ok_or_else(|| {
                PluginError::new(
                    PANEL_NOT_FOUND,
                    format!("plugin {plugin_key} does not declare any panel"),
                )
            })?
            .id
            .clone();
        self.panel_entry_for(&plugin_ref, &first, SessionKind::GlobalView, owner_window)
    }

    /// Reads a declared main-area tab entry, wraps it in the host CSP shell,
    /// and issues a renderer session token bound to that tab + instance +
    /// worktree + manifest revision (spec §3.6). Same activation gate as
    /// [`Self::panel_entry`]; an undeclared tab maps to `tab_not_found`.
    #[allow(clippy::too_many_arguments)]
    pub fn tab_entry(
        &self,
        plugin_key: &str,
        scope: PluginScope,
        repo_id: Option<&str>,
        tab_id: &str,
        instance_id: &str,
        worktree_id: Option<&str>,
        owner_window: &str,
    ) -> Result<PanelEntry, PluginError> {
        let plugin_ref = PluginRef {
            plugin_key: plugin_key.to_string(),
            scope,
            repo_id: repo_id.map(str::to_string),
        };
        let (entry, manifest) = self.approved_entry(&plugin_ref)?;
        let tab = manifest
            .contributes
            .tabs
            .iter()
            .find(|tab| tab.id == tab_id)
            .ok_or_else(|| {
                PluginError::new(
                    TAB_NOT_FOUND,
                    format!("plugin {plugin_key} does not declare tab {tab_id}"),
                )
            })?;
        // Installed trees are immutable and hash-addressed; dev trees are
        // deliberately mutable and skipped (integrity module docs).
        if !entry.is_dev() {
            verify_installed(&self.roots_for_ref(&plugin_ref)?.install_dir, &manifest)?;
        }
        let path = resolve_declared_artifact(&entry.root, &tab.entry, PLUGIN_TAB_ENTRY_MAX_BYTES)?;
        let bytes = std::fs::read(&path).map_err(|error| {
            PluginError::new(IO_ERROR, format!("tab entry could not be read: {error}"))
        })?;
        let html = String::from_utf8_lossy(&bytes);
        let token = self.sessions.issue(SessionBinding {
            plugin_key: plugin_key.to_string(),
            kind: SessionKind::MainTab,
            panel_id: None,
            tab_id: Some(tab_id.to_string()),
            instance_id: Some(instance_id.to_string()),
            scope,
            repo_id: repo_id.map(str::to_string),
            worktree_id: worktree_id.map(str::to_string),
            manifest_revision: manifest_revision(&manifest),
            owner: SessionOwner::Renderer {
                window_label: owner_window.to_string(),
            },
        });
        Ok(PanelEntry {
            html: build_panel_shell_html(&html),
            session_token: token,
        })
    }

    fn panel_entry_for(
        &self,
        plugin_ref: &PluginRef,
        panel_id: &str,
        kind: SessionKind,
        owner_window: &str,
    ) -> Result<PanelEntry, PluginError> {
        let (entry, manifest) = self.approved_entry(plugin_ref)?;
        let panel = manifest
            .contributes
            .panels
            .iter()
            .find(|panel| panel.id == panel_id)
            .ok_or_else(|| {
                PluginError::new(
                    PANEL_NOT_FOUND,
                    format!(
                        "plugin {} does not declare panel {panel_id}",
                        plugin_ref.plugin_key
                    ),
                )
            })?;
        // Installed trees are immutable and hash-addressed; dev trees are
        // deliberately mutable and skipped (integrity module docs).
        if !entry.is_dev() {
            verify_installed(&self.roots_for_ref(plugin_ref)?.install_dir, &manifest)?;
        }
        let path =
            resolve_declared_artifact(&entry.root, &panel.entry, PLUGIN_PANEL_ENTRY_MAX_BYTES)?;
        let bytes = std::fs::read(&path).map_err(|error| {
            PluginError::new(IO_ERROR, format!("panel entry could not be read: {error}"))
        })?;
        let html = String::from_utf8_lossy(&bytes);
        let token = self.sessions.issue(SessionBinding {
            plugin_key: plugin_ref.plugin_key.clone(),
            kind,
            panel_id: Some(panel_id.to_string()),
            tab_id: None,
            instance_id: None,
            scope: plugin_ref.scope,
            repo_id: plugin_ref.repo_id.clone(),
            worktree_id: None,
            manifest_revision: manifest_revision(&manifest),
            owner: SessionOwner::Renderer {
                window_label: owner_window.to_string(),
            },
        });
        Ok(PanelEntry {
            html: build_panel_shell_html(&html),
            session_token: token,
        })
    }

    /// Shared renderer-entry gate: discovery + engine check + approved
    /// activation, returning the discovered tree and its manifest.
    fn approved_entry(
        &self,
        plugin_ref: &PluginRef,
    ) -> Result<(DiscoveredPlugin, crate::manifest::Manifest), PluginError> {
        let discovered = self.discover_all();
        let entry = find_discovered(&discovered, plugin_ref).ok_or_else(|| {
            PluginError::new(
                UNAVAILABLE,
                format!(
                    "plugin {} is not installed or is invalid",
                    plugin_ref.plugin_key
                ),
            )
        })?;
        let manifest = entry
            .manifest
            .clone()
            .expect("valid entries carry a manifest");
        if !engine_satisfies(&manifest.engines.orca, &self.config.host_version) {
            return Err(PluginError::new(
                crate::types::ENGINE_INCOMPATIBLE,
                format!(
                    "plugin {} requires Orca {} (this is {})",
                    plugin_ref.plugin_key, manifest.engines.orca, self.config.host_version
                ),
            ));
        }
        let content_key = entry
            .install_hash
            .clone()
            .or_else(|| hash_plugin_tree(&entry.root).ok())
            .unwrap_or_else(|| "unresolved".to_string());
        let activation = self.config.consent_state.activation(
            &plugin_ref.plugin_key,
            consent_scope_of(plugin_ref.scope, plugin_ref.repo_id.as_deref()),
            &fingerprint(&manifest, &content_key),
        );
        if activation != Some(ActivationState::Approved) {
            return Err(PluginError::new(
                gate::CONSENT_REQUIRED,
                format!(
                    "plugin {} is not approved for activation",
                    plugin_ref.plugin_key
                ),
            ));
        }
        Ok((entry.clone(), manifest))
    }

    /// Runs one panel-bridge action (spec §3.6): resolves the session token,
    /// re-checks approval + manifest revision + declared panel, admits the
    /// call per token (size/rate), gates it through `host::gate` with
    /// `via_panel = true` (only the four panel-callable methods pass), and
    /// executes through `host::methods::execute` with the bound context.
    pub fn panel_action(&self, token: &str, action: &str, params: Value) -> Outcome {
        let Some(binding) = self.sessions.resolve(token) else {
            return Outcome::err(UNAVAILABLE, "panel session is not available");
        };
        let plugin_ref = PluginRef {
            plugin_key: binding.plugin_key.clone(),
            scope: binding.scope,
            repo_id: binding.repo_id.clone(),
        };
        let discovered = self.discover_all();
        let Some(entry) = find_discovered(&discovered, &plugin_ref) else {
            return Outcome::err(
                UNAVAILABLE,
                format!(
                    "plugin {} is not installed or is invalid",
                    binding.plugin_key
                ),
            );
        };
        let manifest = entry
            .manifest
            .as_ref()
            .expect("valid entries carry a manifest");
        if binding.manifest_revision != manifest_revision(manifest) {
            return Outcome::err(
                UNAVAILABLE,
                "panel session predates the current plugin revision",
            );
        }
        if let Some(panel_id) = binding.panel_id.as_deref() {
            if !manifest
                .contributes
                .panels
                .iter()
                .any(|panel| panel.id == panel_id)
            {
                return Outcome::err(
                    PANEL_NOT_FOUND,
                    format!("panel {panel_id} is no longer declared"),
                );
            }
        }
        if let Some(tab_id) = binding.tab_id.as_deref() {
            if !manifest.contributes.tabs.iter().any(|tab| tab.id == tab_id) {
                return Outcome::err(UNAVAILABLE, format!("tab {tab_id} is no longer declared"));
            }
        }
        let content_key = entry
            .install_hash
            .clone()
            .or_else(|| hash_plugin_tree(&entry.root).ok())
            .unwrap_or_else(|| "unresolved".to_string());
        let approved = engine_satisfies(&manifest.engines.orca, &self.config.host_version)
            && self.config.consent_state.activation(
                &binding.plugin_key,
                consent_scope_of(binding.scope, binding.repo_id.as_deref()),
                &fingerprint(manifest, &content_key),
            ) == Some(ActivationState::Approved);
        // An unapproved plugin presents no grant set, but the gate still runs
        // first on `via_panel` so non-panel methods report `panel_forbidden`
        // exactly like the oracle decision order.
        let granted: Option<&[Capability]> = approved.then_some(manifest.capabilities.as_slice());
        if let gate::GateDecision::Denied { code, error } = gate::decide(granted, true, action) {
            return Outcome::err(code, error);
        }
        let params_bytes = serde_json::to_vec(&params)
            .map(|encoded| encoded.len())
            .unwrap_or(usize::MAX);
        if let Err(error) = self.panel_admission.admit(token, params_bytes) {
            return Outcome::err(error.code(), error.message().to_string());
        }
        let Ok(roots) = self.roots_for_ref(&plugin_ref) else {
            return Outcome::err(UNAVAILABLE, "plugin scope is no longer registered");
        };
        let audit = ScopeAudit {
            log: AuditLog::new(&roots.data_dir),
        };
        let ctx = HostCallContext {
            plugin_key: &binding.plugin_key,
            granted,
            via_panel: true,
            session_token: Some(token),
            scope: binding.scope,
            repo_id: binding.repo_id.as_deref(),
            worktree_hint: binding.worktree_id.as_deref(),
            services: &*self.config.host_services,
            audit: Some(&audit),
        };
        methods::execute(action, params, &ctx)
    }

    /// `session.post` core (spec §3.5): resolves the panel session token,
    /// verifies it belongs to the calling plugin, admits the push against the
    /// token's per-window budget, and emits
    /// [`ServiceEventSink::panel_push_requested`] to the renderer.
    pub fn session_post(
        &self,
        plugin_key: &str,
        session_token: &str,
        topic: Option<&str>,
        payload: Value,
    ) -> Result<(), PluginError> {
        let Some(binding) = self.sessions.resolve(session_token) else {
            return Err(PluginError::new(UNAVAILABLE, "session is not available"));
        };
        if binding.plugin_key != plugin_key {
            return Err(PluginError::new(
                UNAVAILABLE,
                "session does not belong to the calling plugin",
            ));
        }
        let payload_bytes = serde_json::to_vec(&payload)
            .map(|encoded| encoded.len())
            .unwrap_or(usize::MAX);
        self.push_admission.admit(session_token, payload_bytes)?;
        self.config
            .event_sink
            .panel_push_requested(PanelPushRequest {
                session_token: session_token.to_string(),
                topic: topic.map(str::to_string),
                payload,
            });
        Ok(())
    }

    /// `workspace.openTab` core: validates `tab_id` against the plugin's
    /// declared tabs, dedupes by `(instance, worktree, tabId, params)`, and
    /// bounds each plugin instance per worktree (≤8 live instances, ≤4 new
    /// opens / 10s). Emits [`ServiceEventSink::open_tab_requested`] for new
    /// instances only; the renderer creates/focuses accordingly.
    pub fn open_tab(
        &self,
        plugin_ref: &PluginRef,
        tab_id: &str,
        title: Option<&str>,
        params: Option<Value>,
        worktree_id: &str,
    ) -> Result<OpenTabResult, PluginError> {
        let declared = self.config.host_services.declared_tabs(
            &plugin_ref.plugin_key,
            plugin_ref.scope,
            plugin_ref.repo_id.as_deref(),
        );
        if !declared.iter().any(|declared| declared == tab_id) {
            return Err(PluginError::new(
                INVALID_PARAMS,
                format!("tab is not declared by the plugin: {tab_id}"),
            ));
        }
        let instance_key = instance_key_of(plugin_ref)?;
        let params_value = params.unwrap_or(Value::Null);
        let now = Instant::now();
        let mut trackers = self
            .tab_trackers
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let tracker = trackers
            .entry((instance_key.clone(), worktree_id.to_string()))
            .or_default();
        if let Some(existing) = tracker
            .tabs
            .iter()
            .find(|tracked| tracked.tab_id == tab_id && tracked.params == params_value)
        {
            return Ok(OpenTabResult {
                instance_id: existing.instance_id.clone(),
                worktree_id: worktree_id.to_string(),
            });
        }
        while tracker
            .opens
            .front()
            .is_some_and(|opened| now.duration_since(*opened) >= OPEN_TAB_RATE_WINDOW)
        {
            tracker.opens.pop_front();
        }
        if tracker.opens.len() >= OPEN_TAB_RATE_MAX {
            return Err(PluginError::new(
                gate::RATE_LIMITED,
                format!("plugin tab open rate limit exceeded ({OPEN_TAB_RATE_MAX} per 10s)"),
            ));
        }
        if tracker.tabs.len() >= OPEN_TAB_MAX_INSTANCES {
            return Err(PluginError::new(
                TAB_LIMIT,
                format!("plugin already has {OPEN_TAB_MAX_INSTANCES} tabs open in this worktree"),
            ));
        }
        let instance_id = Uuid::new_v4().to_string();
        tracker.tabs.push(TrackedTab {
            tab_id: tab_id.to_string(),
            params: params_value.clone(),
            instance_id: instance_id.clone(),
        });
        tracker.opens.push_back(now);
        drop(trackers);
        self.config.event_sink.open_tab_requested(OpenTabRequest {
            plugin_key: plugin_ref.plugin_key.clone(),
            tab_id: tab_id.to_string(),
            instance_id: instance_id.clone(),
            title: title.map(str::to_string),
            params: params_value,
            worktree_id: worktree_id.to_string(),
            scope: plugin_ref.scope,
            repo_id: plugin_ref.repo_id.clone(),
        });
        Ok(OpenTabResult {
            instance_id,
            worktree_id: worktree_id.to_string(),
        })
    }

    /// Merged log tail for one plugin: buffered worker log frames plus the
    /// audit file of the global data dir and of every registered project data
    /// dir (a project-scope plugin writes its audit lines under
    /// `<repo>/.ade/plugin-data`). Chronological, capped at
    /// [`PLUGIN_LOG_TAIL_LIMIT`].
    pub fn get_logs(&self, plugin_key: &str) -> Vec<LogLine> {
        let mut lines = self.bridge.log_lines(plugin_key);
        lines.extend(self.audit.read_tail(plugin_key, PLUGIN_LOG_TAIL_LIMIT));
        let projects = self
            .projects
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone();
        for (_, path) in projects {
            let data_dir = project_roots(&path).data_dir;
            if data_dir == self.global.data_dir {
                continue;
            }
            lines.extend(AuditLog::new(&data_dir).read_tail(plugin_key, PLUGIN_LOG_TAIL_LIMIT));
        }
        lines.sort_by_key(|line| line.ts);
        if lines.len() > PLUGIN_LOG_TAIL_LIMIT {
            lines.drain(0..lines.len() - PLUGIN_LOG_TAIL_LIMIT);
        }
        lines
    }

    /// Classifies the project lockfile against the current install state.
    /// An unknown `repo_id` yields an empty state with an empty path.
    pub fn lockfile_state(&self, repo_id: &str) -> LockState {
        let Some(path) = self.project_path(repo_id) else {
            return LockState {
                repo_id: repo_id.to_string(),
                lockfile_path: String::new(),
                entries: Vec::new(),
            };
        };
        let roots = project_roots(&path);
        let lock = read_lockfile(&roots.lockfile_path);
        let discovered = self.discover_all();
        let mut installed: HashMap<&str, Option<&str>> = HashMap::new();
        for entry in &discovered {
            if entry.scope == PluginScope::Project && entry.repo_id.as_deref() == Some(repo_id) {
                if let Some(key) = entry.key.as_deref() {
                    installed.insert(key, entry.install_hash.as_deref());
                }
            }
        }
        let entries = lock
            .plugins
            .values()
            .map(|entry| {
                let (status, detail) = match installed.get(entry.plugin_key.as_str()) {
                    Some(Some(hash)) if *hash == entry.content_hash => {
                        (LockEntryStatus::Installed, None)
                    }
                    Some(Some(hash)) => (
                        LockEntryStatus::HashMismatch,
                        Some(format!(
                            "installed content {} does not match lockfile {}",
                            hash, entry.content_hash
                        )),
                    ),
                    Some(None) => (
                        LockEntryStatus::HashMismatch,
                        Some("installed plugin has no content identity".to_string()),
                    ),
                    None => match &entry.source {
                        LockSource::LocalPath { path } if !Path::new(path).is_dir() => (
                            LockEntryStatus::SourceUnavailable,
                            Some(format!("local source is unavailable: {path}")),
                        ),
                        _ => (LockEntryStatus::Missing, None),
                    },
                };
                LockStateEntry {
                    plugin_key: entry.plugin_key.clone(),
                    version: entry.version.clone(),
                    source: entry.source.clone(),
                    content_hash: entry.content_hash.clone(),
                    status,
                    detail,
                }
            })
            .collect();
        LockState {
            repo_id: repo_id.to_string(),
            lockfile_path: roots.lockfile_path.to_string_lossy().into_owned(),
            entries,
        }
    }

    /// Reinstalls every `missing`/`hash-mismatch` lockfile entry from its
    /// recorded source; `source-unavailable` entries yield an error result and
    /// `installed` entries are skipped.
    pub fn install_missing_from_lockfile(
        &self,
        repo_id: &str,
    ) -> Vec<Result<InstallResult, PluginError>> {
        let state = self.lockfile_state(repo_id);
        let mut results = Vec::new();
        for entry in state.entries {
            match entry.status {
                LockEntryStatus::Installed => continue,
                LockEntryStatus::SourceUnavailable => {
                    results.push(Err(PluginError::new(
                        UNAVAILABLE,
                        entry
                            .detail
                            .unwrap_or_else(|| "lockfile source is unavailable".to_string()),
                    )));
                    continue;
                }
                LockEntryStatus::Missing | LockEntryStatus::HashMismatch => {}
            }
            let target = ScopeTarget {
                scope: PluginScope::Project,
                repo_id: Some(repo_id.to_string()),
            };
            let result = match &entry.source {
                LockSource::LocalPath { path } => self.install(
                    InstallSource::LocalPath {
                        path: PathBuf::from(path),
                    },
                    target,
                ),
                LockSource::Git { url, ref_ } => self.install(
                    InstallSource::Git {
                        url: url.clone(),
                        ref_: ref_.clone(),
                    },
                    target,
                ),
                LockSource::Marketplace {
                    marketplace,
                    plugin: _,
                } => {
                    let registered = self
                        .marketplace
                        .service()
                        .list_sources()
                        .unwrap_or_default()
                        .into_iter()
                        .find(|source| {
                            source.source.url == marketplace.url
                                && source.source.ref_ == marketplace.ref_
                        });
                    match registered {
                        Some(source) => self.install(
                            InstallSource::Marketplace {
                                source_id: source.id,
                                plugin_key: entry.plugin_key.clone(),
                            },
                            target,
                        ),
                        None => Err(PluginError::new(
                            UNAVAILABLE,
                            format!("marketplace source is not registered: {}", marketplace.url),
                        )),
                    }
                }
                LockSource::Bundled { bundle_id } => Err(PluginError::new(
                    UNAVAILABLE,
                    format!("bundled plugin {bundle_id} cannot be reinstalled"),
                )),
            };
            results.push(result);
        }
        results
    }

    /// Language-pack catalogs contributed by approved+enabled plugins
    /// (approved status is the same gate `list`/activation use). Unreadable
    /// files and malformed catalogs are skipped, never fatal.
    pub fn language_packs(&self) -> Vec<LanguagePackRegistration> {
        let active: HashSet<(String, PluginScope, Option<String>)> = self
            .list()
            .into_iter()
            .filter(|entry| {
                matches!(
                    entry.status,
                    PluginStatus::Idle | PluginStatus::Running | PluginStatus::Restarting
                )
            })
            .map(|entry| (entry.plugin_key, entry.scope, entry.repo_id))
            .collect();
        if active.is_empty() {
            return Vec::new();
        }
        let mut packs = Vec::new();
        for discovered in self.discover_all() {
            let Some(key) = discovered.key.clone() else {
                continue;
            };
            if !discovered.is_valid()
                || !active.contains(&(key.clone(), discovered.scope, discovered.repo_id.clone()))
            {
                continue;
            }
            let Some(manifest) = discovered.manifest.as_ref() else {
                continue;
            };
            if manifest.contributes.language_packs.is_empty() {
                continue;
            }
            let Ok(artifacts) =
                crate::artifact::validate_declared_artifacts(&discovered.root, manifest)
            else {
                continue;
            };
            for artifact in artifacts.language_packs {
                let Ok(bytes) = std::fs::read(&artifact.path) else {
                    continue;
                };
                let Ok(catalog) = serde_json::from_slice::<Value>(&bytes) else {
                    continue;
                };
                let id = format!("plugin:{key}:{}", artifact.locale);
                packs.push(LanguagePackRegistration {
                    resource_language: plugin_language_resource_id(&id),
                    id,
                    plugin_key: key.clone(),
                    locale: artifact.locale,
                    catalog,
                });
            }
        }
        packs
    }

    /// The marketplace installer owned by the service (bridge command surface).
    pub fn marketplace(&self) -> &MarketplaceInstaller {
        &self.marketplace
    }

    /// Locates an installed-from-marketplace plugin (global first, then any
    /// registered project) and previews the marketplace's current listing.
    pub fn preview_marketplace_update(
        &self,
        plugin_key: &str,
    ) -> Result<MarketplacePreview, PluginError> {
        let (_, entry) = self.installed_lock_entry(plugin_key)?;
        let LockSource::Marketplace { marketplace, .. } = &entry.source else {
            return Err(PluginError::new(
                INVALID_PARAMS,
                format!("plugin {plugin_key} was not installed from a marketplace"),
            ));
        };
        let source = self
            .marketplace
            .service()
            .list_sources()?
            .into_iter()
            .find(|source| {
                source.source.url == marketplace.url && source.source.ref_ == marketplace.ref_
            })
            .ok_or_else(|| {
                PluginError::new(
                    UNAVAILABLE,
                    format!("marketplace source is not registered: {}", marketplace.url),
                )
            })?;
        self.marketplace.preview(&source.id, plugin_key)
    }

    /// Rolls the installed-from-marketplace plugin back to its previous
    /// immutable version in the scope it is installed under.
    pub fn rollback_marketplace_plugin(
        &self,
        plugin_key: &str,
    ) -> Result<InstallResult, PluginError> {
        let (repo_id, entry) = self.installed_lock_entry(plugin_key)?;
        if !matches!(entry.source, LockSource::Marketplace { .. }) {
            return Err(PluginError::new(
                INVALID_PARAMS,
                format!("plugin {plugin_key} was not installed from a marketplace"),
            ));
        }
        let target = ScopeTarget {
            scope: if repo_id.is_none() {
                PluginScope::Global
            } else {
                PluginScope::Project
            },
            repo_id,
        };
        let roots = self.roots_for_target(&target)?;
        let result = self
            .marketplace
            .rollback(plugin_key, &roots, target.scope)?;
        self.audit(
            &roots.data_dir,
            plugin_key,
            "plugins.rollback_marketplace_plugin",
            "rolled back",
        );
        self.config.event_sink.plugins_changed(false, true);
        Ok(result)
    }

    /// The lock entry of `plugin_key` (global preferred, then projects).
    fn installed_lock_entry(
        &self,
        plugin_key: &str,
    ) -> Result<(Option<String>, LockEntry), PluginError> {
        let projects = self
            .projects
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone();
        let entries = self.lock_entries(&projects);
        entries
            .get(&(None, plugin_key.to_string()))
            .cloned()
            .map(|entry| (None, entry))
            .or_else(|| {
                entries
                    .into_iter()
                    .filter(|((_, key), _)| key == plugin_key)
                    .min_by_key(|((repo_id, _), _)| repo_id.is_some())
                    .map(|((repo_id, _), entry)| (repo_id, entry))
            })
            .ok_or_else(|| {
                PluginError::new(UNAVAILABLE, format!("plugin {plugin_key} is not installed"))
            })
    }

    /// Serialized per-plugin-data storage write (panel/host mutation path).
    pub fn storage_set(
        &self,
        plugin_ref: &PluginRef,
        key: &str,
        value: Value,
    ) -> Result<(), PluginError> {
        let data_dir = self.data_dir_for_ref(plugin_ref)?;
        self.locks
            .with_plugin_data(&data_dir, &plugin_ref.plugin_key, || {
                self.storage.set(&plugin_ref.plugin_key, key, value)
            })
    }

    /// Serialized per-plugin-data storage delete.
    pub fn storage_delete(&self, plugin_ref: &PluginRef, key: &str) -> Result<(), PluginError> {
        let data_dir = self.data_dir_for_ref(plugin_ref)?;
        self.locks
            .with_plugin_data(&data_dir, &plugin_ref.plugin_key, || {
                self.storage.delete(&plugin_ref.plugin_key, key)
            })
    }

    /// Serialized per-plugin-data settings write.
    pub fn settings_set(
        &self,
        plugin_ref: &PluginRef,
        key: &str,
        value: Value,
    ) -> Result<(), PluginError> {
        let data_dir = self.data_dir_for_ref(plugin_ref)?;
        self.locks
            .with_plugin_data(&data_dir, &plugin_ref.plugin_key, || {
                self.settings.set(&plugin_ref.plugin_key, key, value)
            })
    }

    /// Serialized per-plugin-data secret write.
    pub fn secrets_set(
        &self,
        plugin_ref: &PluginRef,
        key: &str,
        value: &str,
    ) -> Result<(), PluginError> {
        let data_dir = self.data_dir_for_ref(plugin_ref)?;
        self.locks
            .with_plugin_data(&data_dir, &plugin_ref.plugin_key, || {
                self.config.secrets.set(
                    &plugin_ref.plugin_key,
                    plugin_ref.scope,
                    plugin_ref.repo_id.as_deref(),
                    key,
                    value,
                )
            })
    }

    /// Serialized per-plugin-data secret delete.
    pub fn secrets_delete(&self, plugin_ref: &PluginRef, key: &str) -> Result<(), PluginError> {
        let data_dir = self.data_dir_for_ref(plugin_ref)?;
        self.locks
            .with_plugin_data(&data_dir, &plugin_ref.plugin_key, || {
                self.config.secrets.delete(
                    &plugin_ref.plugin_key,
                    plugin_ref.scope,
                    plugin_ref.repo_id.as_deref(),
                    key,
                )
            })
    }

    fn install_locked(
        &self,
        source: InstallSource,
        target: ScopeTarget,
        roots: ScopeRoots,
    ) -> Result<InstallResult, PluginError> {
        let context = |consent_fingerprint: String| InstallContext {
            scope: target.scope,
            repo_id: target.repo_id.clone(),
            host_version: self.config.host_version.clone(),
            consent_fingerprint,
        };
        let summary = install_source_kind(&source);
        let result = match source {
            InstallSource::LocalPath { path } => {
                let manifest = read_install_manifest(&path, &self.config.host_version)?;
                let content_hash = hash_plugin_tree(&path)?;
                let fingerprint = fingerprint(&manifest, &content_hash);
                install_from_local(&path, &roots, &context(fingerprint))
            }
            InstallSource::Git { url, ref_ } => {
                let workspace = TempWorkspace::create("ade-plugin-git-install-")?;
                let destination = workspace.path().join("plugin");
                let reference = (!ref_.is_empty()).then_some(ref_.as_str());
                let resolved_commit = self.git.clone_depth1(&url, reference, &destination)?;
                let manifest = read_install_manifest(&destination, &self.config.host_version)?;
                let content_hash = hash_plugin_tree(&destination)?;
                let fingerprint = fingerprint(&manifest, &content_hash);
                let lock_source = LockSource::Git { url, ref_ };
                self.publish_tree(
                    &destination,
                    &roots,
                    &context(fingerprint),
                    lock_source,
                    Some(resolved_commit),
                )
            }
            InstallSource::Marketplace {
                source_id,
                plugin_key,
            } => {
                let preview = self.marketplace.preview(&source_id, &plugin_key)?;
                let install_context = context(preview.consent_fingerprint.clone());
                self.marketplace.install(&preview, &roots, &install_context)
            }
        };
        if let Ok(install) = &result {
            self.audit(
                &roots.data_dir,
                &install.plugin_key,
                "plugins.install",
                &summary,
            );
            self.config.event_sink.plugins_changed(false, true);
        }
        result
    }

    /// Stages and publishes a tree whose lock source is not `local-path`
    /// (mirrors [`install_from_local`] with a caller-supplied source).
    fn publish_tree(
        &self,
        source_root: &Path,
        roots: &ScopeRoots,
        context: &InstallContext,
        source: LockSource,
        resolved_commit: Option<String>,
    ) -> Result<InstallResult, PluginError> {
        let manifest = read_install_manifest(source_root, &context.host_version)?;
        let plugin_key = manifest.qualified_key();
        if context.scope == PluginScope::Project {
            ensure_project_gitignore(roots)?;
        }
        let plugin_dir = roots.install_dir.join(&plugin_key);
        let (content_hash, version_dir) =
            crate::install::staging::stage_plugin_tree(source_root, &plugin_dir, &manifest)?;
        let staged_manifest = read_install_manifest(&version_dir, &context.host_version)?;
        if staged_manifest.qualified_key() != plugin_key {
            return Err(PluginError::new(
                crate::types::INVALID_MANIFEST,
                "staged plugin manifest identity does not match its directory",
            ));
        }
        let entry = LockEntry {
            plugin_key: plugin_key.clone(),
            version: staged_manifest.version.clone(),
            source,
            resolved_commit: resolved_commit.clone(),
            content_hash: content_hash.clone(),
            consent_fingerprint: context.consent_fingerprint.clone(),
            installed_at: now_millis(),
        };
        publish_install(
            roots,
            &plugin_key,
            &content_hash,
            &entry,
            &roots.lockfile_path,
        )?;
        Ok(InstallResult {
            plugin_key,
            version: staged_manifest.version,
            content_hash: content_hash.clone(),
            consent_fingerprint: context.consent_fingerprint.clone(),
            install_hash: content_hash,
            scope: context.scope,
            resolved_commit,
        })
    }

    fn discover_all(&self) -> Vec<DiscoveredPlugin> {
        let projects = self
            .projects
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone();
        discover(&self.global, &projects, &self.config.dev_paths)
    }

    fn project_entries(&self, discovered: &[DiscoveredPlugin]) -> Vec<PluginListEntry> {
        let projects = self
            .projects
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone();
        let projections: Vec<DiscoveryProjection> = if projects.is_empty() {
            DiscoveryProjection::project(discovered, None)
        } else {
            let mut merged: Vec<DiscoveryProjection> = Vec::new();
            let mut seen: HashSet<(String, PluginScope, Option<String>)> = HashSet::new();
            for (repo_id, _) in &projects {
                for projection in DiscoveryProjection::project(discovered, Some(repo_id)) {
                    let identity = (
                        projection.plugin_key.clone(),
                        projection.scope,
                        projection.repo_id.clone(),
                    );
                    if seen.insert(identity) {
                        merged.push(projection);
                    }
                }
            }
            merged
        };

        let dev_roots: HashMap<&str, &Path> = discovered
            .iter()
            .filter(|entry| entry.is_dev())
            .filter_map(|entry| entry.key.as_deref().map(|key| (key, entry.root.as_path())))
            .collect();
        let lock_entries = self.lock_entries(&projects);

        projections
            .into_iter()
            .map(|projection| self.project_entry(&projection, &dev_roots, &lock_entries))
            .collect()
    }

    fn project_entry(
        &self,
        projection: &DiscoveryProjection,
        dev_roots: &HashMap<&str, &Path>,
        lock_entries: &HashMap<(Option<String>, String), LockEntry>,
    ) -> PluginListEntry {
        let key = projection.plugin_key.clone();
        let scope = projection.scope;
        let repo_id = projection.repo_id.clone();
        let manifest = projection.manifest.clone();

        let consent_fingerprint = manifest.as_ref().map(|manifest| {
            let content_key = projection
                .install_hash
                .clone()
                .or_else(|| {
                    dev_roots
                        .get(key.as_str())
                        .and_then(|root| hash_plugin_tree(root).ok())
                })
                .unwrap_or_else(|| "unresolved".to_string());
            fingerprint(manifest, &content_key)
        });

        let mut status = PluginStatus::Invalid;
        let mut error = projection.error.clone();
        let mut needs_reconsent = false;
        if let (Some(manifest), Some(current_fingerprint)) = (&manifest, &consent_fingerprint) {
            if !engine_satisfies(&manifest.engines.orca, &self.config.host_version) {
                error = Some(format!(
                    "plugin requires Orca {} (this is {})",
                    manifest.engines.orca, self.config.host_version
                ));
            } else {
                error = None;
                needs_reconsent = self.config.consent_state.needs_reconsent(
                    &key,
                    consent_scope_of(scope, repo_id.as_deref()),
                    current_fingerprint,
                );
                status = match self.config.consent_state.activation(
                    &key,
                    consent_scope_of(scope, repo_id.as_deref()),
                    current_fingerprint,
                ) {
                    Some(crate::consent::ActivationState::Disabled) => PluginStatus::Disabled,
                    Some(crate::consent::ActivationState::Pending) | None => PluginStatus::Pending,
                    Some(crate::consent::ActivationState::Approved) => PluginStatus::Idle,
                };
            }
        }

        let source = match lock_entries.get(&(repo_id.clone(), key.clone())) {
            Some(entry) => PluginSourceProjection {
                kind: lock_source_kind(&entry.source).to_string(),
                reference: lock_source_reference(&entry.source),
                resolved_commit: entry.resolved_commit.clone(),
                content_hash: Some(entry.content_hash.clone()),
            },
            None if projection.is_dev => PluginSourceProjection {
                kind: "dev".to_string(),
                reference: None,
                resolved_commit: None,
                content_hash: None,
            },
            None if projection.source_kind == PluginSourceKind::Bundled => PluginSourceProjection {
                kind: "bundled".to_string(),
                reference: Some(key.clone()),
                resolved_commit: None,
                content_hash: projection.install_hash.clone(),
            },
            None => PluginSourceProjection {
                kind: "installed".to_string(),
                reference: None,
                resolved_commit: None,
                content_hash: projection.install_hash.clone(),
            },
        };

        let contributes = manifest.as_ref().map(|manifest| &manifest.contributes);
        let panels = contributes
            .map(|contributes| {
                contributes
                    .panels
                    .iter()
                    .map(|panel| PluginPanelProjection {
                        id: panel.id.clone(),
                        title: panel.title.clone(),
                        icon: panel.icon.clone(),
                        tab_key: format!("plugin:{key}"),
                    })
                    .collect()
            })
            .unwrap_or_default();
        let tabs = contributes
            .map(|contributes| {
                contributes
                    .tabs
                    .iter()
                    .map(|tab| PluginTabProjection {
                        id: tab.id.clone(),
                        title: tab.title.clone(),
                        icon: tab.icon.clone(),
                    })
                    .collect()
            })
            .unwrap_or_default();
        let commands = contributes
            .map(|contributes| {
                contributes
                    .commands
                    .iter()
                    .map(|command| PluginCommandProjection {
                        id: command.id.clone(),
                        title: command.title.clone(),
                        context: command
                            .context
                            .unwrap_or(crate::manifest::CommandContext::Global),
                        handler: match &command.action {
                            Some(action) => PluginCommandHandler::BuiltIn {
                                action: action.clone(),
                            },
                            None => PluginCommandHandler::Worker,
                        },
                        visibility: command.visibility,
                        keybindings: contributes
                            .keybindings
                            .iter()
                            .filter(|binding| binding.command == command.id)
                            .map(|binding| PluginKeybindingProjection {
                                key: binding.key.clone(),
                                when: binding
                                    .when
                                    .unwrap_or(crate::manifest::CommandContext::Global),
                            })
                            .collect(),
                    })
                    .collect()
            })
            .unwrap_or_default();

        PluginListEntry {
            plugin_key: key.clone(),
            scope,
            repo_id: repo_id.clone(),
            name: manifest
                .as_ref()
                .map(|manifest| manifest.name.clone())
                .unwrap_or_else(|| key.clone()),
            version: manifest
                .as_ref()
                .map(|manifest| manifest.version.clone())
                .unwrap_or_else(|| "0.0.0".to_string()),
            publisher: manifest
                .as_ref()
                .map(|manifest| manifest.publisher.clone())
                .unwrap_or_else(|| key.split('.').next().unwrap_or_default().to_string()),
            description: manifest
                .as_ref()
                .and_then(|manifest| manifest.description.clone()),
            status,
            needs_reconsent,
            error,
            is_dev: projection.is_dev,
            official: is_official_plugin_identity(&key),
            bundled: projection.source_kind == PluginSourceKind::Bundled,
            capabilities: manifest
                .as_ref()
                .map(|manifest| {
                    manifest
                        .capabilities
                        .iter()
                        .map(|capability| PluginCapabilityProjection {
                            kind: capability.as_str().to_string(),
                            description: capability_description(*capability).to_string(),
                        })
                        .collect()
                })
                .unwrap_or_default(),
            panels,
            tabs,
            commands,
            has_worker: manifest
                .as_ref()
                .is_some_and(|manifest| manifest.main.is_some()),
            restarts: 0,
            source,
            install_hash: projection.install_hash.clone(),
            consent_fingerprint,
        }
    }

    fn lock_entries(
        &self,
        projects: &[(String, PathBuf)],
    ) -> HashMap<(Option<String>, String), LockEntry> {
        let mut map = HashMap::new();
        for entry in read_lockfile(&self.global.lockfile_path)
            .plugins
            .into_values()
        {
            map.insert((None, entry.plugin_key.clone()), entry);
        }
        for (repo_id, path) in projects {
            for entry in read_lockfile(&project_roots(path).lockfile_path)
                .plugins
                .into_values()
            {
                map.insert((Some(repo_id.clone()), entry.plugin_key.clone()), entry);
            }
        }
        map
    }

    fn current_fingerprint(&self, plugin_ref: &PluginRef) -> Result<String, PluginError> {
        let discovered = self.discover_all();
        let entry = discovered
            .iter()
            .find(|entry| {
                entry.is_valid()
                    && entry.key.as_deref() == Some(plugin_ref.plugin_key.as_str())
                    && entry.scope == plugin_ref.scope
                    && entry.repo_id == plugin_ref.repo_id
            })
            .ok_or_else(|| {
                PluginError::new(
                    UNAVAILABLE,
                    format!(
                        "plugin {} is not installed or is invalid",
                        plugin_ref.plugin_key
                    ),
                )
            })?;
        let manifest = entry
            .manifest
            .as_ref()
            .expect("valid entries carry a manifest");
        let content_key = entry
            .install_hash
            .clone()
            .or_else(|| hash_plugin_tree(&entry.root).ok())
            .unwrap_or_else(|| "unresolved".to_string());
        Ok(fingerprint(manifest, &content_key))
    }

    fn roots_for_target(&self, target: &ScopeTarget) -> Result<ScopeRoots, PluginError> {
        match target.scope {
            PluginScope::Global => {
                if target.repo_id.is_some() {
                    return Err(PluginError::new(
                        INVALID_PARAMS,
                        "global scope must not carry a repo id",
                    ));
                }
                Ok(self.global.clone())
            }
            PluginScope::Project => {
                let repo_id = target.repo_id.as_deref().ok_or_else(|| {
                    PluginError::new(INVALID_PARAMS, "project scope requires a repo id")
                })?;
                let path = self.project_path(repo_id).ok_or_else(|| {
                    PluginError::new(INVALID_PARAMS, format!("unknown project: {repo_id}"))
                })?;
                Ok(project_roots(&path))
            }
        }
    }

    fn roots_for_ref(&self, plugin_ref: &PluginRef) -> Result<ScopeRoots, PluginError> {
        self.roots_for_target(&ScopeTarget {
            scope: plugin_ref.scope,
            repo_id: plugin_ref.repo_id.clone(),
        })
    }

    fn data_dir_for_ref(&self, plugin_ref: &PluginRef) -> Result<PathBuf, PluginError> {
        Ok(self.roots_for_ref(plugin_ref)?.data_dir)
    }

    fn project_path(&self, repo_id: &str) -> Option<PathBuf> {
        let projects = self
            .projects
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        projects
            .iter()
            .find(|(id, _)| id == repo_id)
            .map(|(_, path)| path.clone())
    }

    fn audit(&self, data_dir: &Path, plugin_key: &str, method: &str, summary: &str) {
        let record = AuditRecord {
            ts: now_millis() as i64,
            actor: format!("plugin:{plugin_key}"),
            method: method.to_string(),
            summary: summary.to_string(),
            outcome: AuditOutcome::Ok,
        };
        let result = if data_dir == self.global.data_dir {
            self.audit.record(record)
        } else {
            AuditLog::new(data_dir).record(record)
        };
        let _ = result;
    }

    fn manager(&self) -> std::sync::MutexGuard<'_, WorkerManager> {
        self.manager
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Overlays live worker state (and fatal-frame errors) onto the
    /// consent-derived projection for entries that are approved (`idle`).
    fn project_runtime_status(&self, entries: &mut [PluginListEntry]) {
        let records = self.bridge.lock_records().clone();
        if records.is_empty() {
            return;
        }
        let manager = self.manager();
        for entry in entries {
            if entry.status != PluginStatus::Idle {
                continue;
            }
            let Some(record) = records.values().find(|record| {
                record.plugin_key == entry.plugin_key
                    && record.scope == entry.scope
                    && record.repo_id == entry.repo_id
            }) else {
                continue;
            };
            if let Some(error) = &record.error {
                entry.status = PluginStatus::Errored;
                entry.error = Some(error.clone());
            } else {
                match manager.state(&record.instance_key) {
                    WorkerState::Running { .. } => entry.status = PluginStatus::Running,
                    WorkerState::Starting | WorkerState::Restarting => {
                        entry.status = PluginStatus::Restarting;
                    }
                    WorkerState::Errored { reason } => {
                        entry.status = PluginStatus::Errored;
                        entry.error = Some(reason);
                    }
                    WorkerState::Inactive => {}
                }
            }
            entry.restarts = manager.restarts(&record.instance_key);
        }
    }

    /// Builds the worker spec/record for an approved `plugin_ref`, or the
    /// gate error explaining why it cannot activate.
    fn worker_spec_for(
        &self,
        plugin_ref: &PluginRef,
    ) -> Result<(WorkerSpec, WorkerRecord), PluginError> {
        let discovered = self.discover_all();
        let entry = find_discovered(&discovered, plugin_ref).ok_or_else(|| {
            PluginError::new(
                UNAVAILABLE,
                format!(
                    "plugin {} is not installed or is invalid",
                    plugin_ref.plugin_key
                ),
            )
        })?;
        let manifest = entry
            .manifest
            .as_ref()
            .expect("valid entries carry a manifest");
        let Some(main_entry) = manifest.main.clone() else {
            return Err(PluginError::new(
                INVALID_PARAMS,
                format!("plugin {} has no worker entry", plugin_ref.plugin_key),
            ));
        };
        if !engine_satisfies(&manifest.engines.orca, &self.config.host_version) {
            return Err(PluginError::new(
                crate::types::ENGINE_INCOMPATIBLE,
                format!(
                    "plugin {} requires Orca {} (this is {})",
                    plugin_ref.plugin_key, manifest.engines.orca, self.config.host_version
                ),
            ));
        }
        let content_key = entry
            .install_hash
            .clone()
            .or_else(|| hash_plugin_tree(&entry.root).ok())
            .unwrap_or_else(|| "unresolved".to_string());
        let current_fingerprint = fingerprint(manifest, &content_key);
        let activation = self.config.consent_state.activation(
            &plugin_ref.plugin_key,
            consent_scope_of(plugin_ref.scope, plugin_ref.repo_id.as_deref()),
            &current_fingerprint,
        );
        if activation != Some(ActivationState::Approved) {
            return Err(PluginError::new(
                gate::CONSENT_REQUIRED,
                format!(
                    "plugin {} is not approved for activation",
                    plugin_ref.plugin_key
                ),
            ));
        }
        // Installed trees are immutable and hash-addressed: lazily re-verify
        // the payload's bytes against the current pointer before the first
        // worker start, so a tampered tree can never execute with granted
        // capabilities (spec §3.1/§3.5). The verifier keys on the immutable
        // `(plugin_key, version_dir, install_hash)` identity, so worker
        // runtime artifacts in the tree cannot fail an already-proven
        // identity on later activations. Dev trees are live source
        // directories that change while Orca runs and are skipped here
        // (oracle parity; see the integrity module docs).
        if !entry.is_dev() {
            let content_hash = entry.install_hash.as_deref().unwrap_or_default();
            self.verifier
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .verify(&plugin_ref.plugin_key, &entry.root, content_hash)?;
        }
        let declared_commands = manifest
            .contributes
            .commands
            .iter()
            .filter(|command| command.action.is_none())
            .map(|command| command.id.clone())
            .collect();
        let data_dir = self.data_dir_for_ref(plugin_ref)?;
        let revision = manifest_revision(manifest);
        let instance_key = instance_key_of(plugin_ref)?;
        let spec = WorkerSpec {
            instance_key: instance_key.clone(),
            plugin_key: plugin_ref.plugin_key.clone(),
            plugin_root: entry.root.clone(),
            main_entry,
            granted_capabilities: manifest.capabilities.clone(),
            manifest_revision: revision.clone(),
            declared_commands,
        };
        let record = WorkerRecord {
            instance_key,
            plugin_key: plugin_ref.plugin_key.clone(),
            scope: plugin_ref.scope,
            repo_id: plugin_ref.repo_id.clone(),
            data_dir,
            granted: manifest.capabilities.clone(),
            revision,
            error: None,
        };
        Ok((spec, record))
    }

    /// Stops the worker, drops dynamic subscriptions and tab bookkeeping, and
    /// invalidates sessions of `plugin_ref` for exactly its own scope+repo
    /// (sessions can outlive a worker), so a same-key twin in another scope
    /// is never disturbed.
    fn deactivate_ref(&self, plugin_ref: &PluginRef) {
        let Ok(instance_key) = instance_key_of(plugin_ref) else {
            return;
        };
        let removed = self.bridge.lock_records().remove(&instance_key);
        let owned = removed.is_some_and(|record| {
            record.plugin_key == plugin_ref.plugin_key
                && record.scope == plugin_ref.scope
                && record.repo_id == plugin_ref.repo_id
        });
        self.bridge.clear_dynamic_subs(&instance_key);
        self.prune_tab_instance(&instance_key);
        if owned {
            self.manager().stop(&instance_key);
        }
        self.sessions.invalidate_plugin_scope(
            &plugin_ref.plugin_key,
            plugin_ref.scope,
            plugin_ref.repo_id.as_deref(),
        );
    }

    /// Drops the tab dedupe/limit bookkeeping of one worker instance.
    fn prune_tab_instance(&self, instance_key: &str) {
        self.tab_trackers
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .retain(|(key, _), _| key != instance_key);
    }

    /// Drops the tab dedupe/limit bookkeeping of one worktree.
    fn prune_tab_worktree(&self, worktree_id: &str) {
        self.tab_trackers
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .retain(|(_, tracked_worktree), _| tracked_worktree != worktree_id);
    }

    /// Dev-watcher callback: re-discovers and hot-restarts running dev
    /// workers whose manifest revision changed (spec §3.8).
    fn on_dev_change(&self, _paths: Vec<PathBuf>) {
        let discovered = self.discover_all();
        for entry in discovered
            .iter()
            .filter(|entry| entry.is_dev() && entry.is_valid())
        {
            let Some(key) = entry.key.as_deref() else {
                continue;
            };
            let Some(manifest) = entry.manifest.as_ref() else {
                continue;
            };
            let plugin_ref = PluginRef {
                plugin_key: key.to_string(),
                scope: entry.scope,
                repo_id: entry.repo_id.clone(),
            };
            let Ok(instance_key) = instance_key_of(&plugin_ref) else {
                continue;
            };
            let Some(record_revision) = self
                .bridge
                .lock_records()
                .get(&instance_key)
                .map(|record| record.revision.clone())
            else {
                continue;
            };
            let running = matches!(
                self.manager().state(&instance_key),
                WorkerState::Running { .. } | WorkerState::Restarting
            );
            if !running || record_revision == manifest_revision(manifest) {
                continue;
            }
            let _ = self.ensure_active(&plugin_ref);
        }
        self.config.event_sink.plugins_changed(false, true);
    }
}

/// The discovery entry matching one plugin instance (valid entries only).
fn find_discovered<'a>(
    discovered: &'a [DiscoveredPlugin],
    plugin_ref: &PluginRef,
) -> Option<&'a DiscoveredPlugin> {
    discovered.iter().find(|entry| {
        entry.is_valid()
            && entry.key.as_deref() == Some(plugin_ref.plugin_key.as_str())
            && entry.scope == plugin_ref.scope
            && entry.repo_id == plugin_ref.repo_id
    })
}

/// Manifest revision: a stable hash of the serialized manifest. A revision
/// change (dev manifest edit) restarts a running worker; file edits that do
/// not touch the manifest (worker logs, pid markers) never do.
fn manifest_revision(manifest: &crate::manifest::Manifest) -> String {
    use sha2::{Digest, Sha256};
    let encoded = serde_json::to_vec(manifest).unwrap_or_default();
    format!("manifest-sha256-{}", hex::encode(Sha256::digest(encoded)))
}

fn consent_scope_of(scope: PluginScope, repo_id: Option<&str>) -> ConsentKeyScope<'_> {
    match scope {
        PluginScope::Global => ConsentKeyScope::Global,
        PluginScope::Project => ConsentKeyScope::Project {
            repo_id: repo_id.unwrap_or_default(),
        },
    }
}

/// Scope-qualified instance identity (`global:<key>` /
/// `project:<repoId>:<key>`); the same scoped-key format consent records use.
fn instance_key_of(plugin_ref: &PluginRef) -> Result<String, PluginError> {
    scoped_key(&plugin_ref.plugin_key, scope_key_scope(plugin_ref)?)
        .ok_or_else(|| PluginError::new(INVALID_PARAMS, "invalid plugin scope for instance key"))
}

fn scope_key_scope(plugin_ref: &PluginRef) -> Result<ConsentKeyScope<'_>, PluginError> {
    match plugin_ref.scope {
        PluginScope::Global => Ok(ConsentKeyScope::Global),
        PluginScope::Project => Ok(ConsentKeyScope::Project {
            repo_id: plugin_ref.repo_id.as_deref().ok_or_else(|| {
                PluginError::new(INVALID_PARAMS, "project scope requires a repo id")
            })?,
        }),
    }
}

fn lock_source_kind(source: &LockSource) -> &'static str {
    match source {
        LockSource::LocalPath { .. } => "local-path",
        LockSource::Git { .. } => "git",
        LockSource::Marketplace { .. } => "marketplace",
        LockSource::Bundled { .. } => "bundled",
    }
}

fn lock_source_reference(source: &LockSource) -> Option<String> {
    match source {
        LockSource::LocalPath { path } => Some(path.clone()),
        LockSource::Git { url, .. } => Some(url.clone()),
        LockSource::Marketplace { plugin, .. } => Some(plugin.url.clone()),
        LockSource::Bundled { bundle_id } => Some(bundle_id.clone()),
    }
}

fn install_source_kind(source: &InstallSource) -> String {
    match source {
        InstallSource::LocalPath { .. } => "installed from local path".to_string(),
        InstallSource::Git { url, .. } => format!("installed from git {url}"),
        InstallSource::Marketplace { source_id, .. } => {
            format!("installed from marketplace {source_id}")
        }
    }
}

/// Plain-language consent copy per capability, verbatim from
/// `orca:src/shared/plugins/plugin-capabilities.ts` (plus the fork's
/// `workspace:open-tab`).
fn capability_description(capability: Capability) -> &'static str {
    match capability {
        Capability::GitRead => {
            "Read the commit history and per-commit changed files of the current worktree (read-only)"
        }
        Capability::WorkspaceRead => {
            "Read the name, branch, and terminal list of your focused worktree"
        }
        Capability::TerminalSend => {
            "Type text into a terminal you can see (always a specific terminal)"
        }
        Capability::NotificationsShow => "Show desktop notifications labeled with the plugin name",
        Capability::Storage => "Store data in the plugin's own storage folder",
        Capability::Secrets => "Store and read secrets in the plugin's own encrypted vault",
        Capability::EventsSubscribe => {
            "Get notified when worktrees are created or removed and when agent status changes"
        }
        Capability::SettingsOwn => "Read and change the plugin's own settings",
        Capability::WorkspaceOpenTab => "Open a new tab in the main workspace area",
        Capability::WorkerCall => {
            "Invoke the plugin's own background commands from its panels and tabs"
        }
        Capability::SessionPush => {
            "Push live updates from the plugin's background worker into its panels and tabs"
        }
    }
}

fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::consent::test_support::FakeConsentState;
    use crate::host::services::{
        HostServiceError, PluginSettingsAccess, PluginStorageAccess, TerminalRef, WorktreeContext,
    };
    use serde_json::json;
    use std::sync::mpsc;
    use std::thread;
    use std::time::{Duration, Instant};
    use tempfile::tempdir;

    #[derive(Default)]
    struct FakeSecrets {
        values: Mutex<HashMap<String, String>>,
    }

    impl FakeSecrets {
        /// Scope-qualified account mirroring `KeychainSecrets`.
        fn account(
            plugin_key: &str,
            scope: PluginScope,
            repo_id: Option<&str>,
            key: &str,
        ) -> String {
            match scope {
                PluginScope::Global => format!("global:{plugin_key}:{key}"),
                PluginScope::Project => {
                    format!("project:{}:{plugin_key}:{key}", repo_id.unwrap_or_default())
                }
            }
        }
    }

    impl SecretStore for FakeSecrets {
        fn get(
            &self,
            plugin_key: &str,
            scope: PluginScope,
            repo_id: Option<&str>,
            key: &str,
        ) -> Result<Option<String>, PluginError> {
            let values = self
                .values
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            Ok(values
                .get(&FakeSecrets::account(plugin_key, scope, repo_id, key))
                .cloned())
        }

        fn set(
            &self,
            plugin_key: &str,
            scope: PluginScope,
            repo_id: Option<&str>,
            key: &str,
            value: &str,
        ) -> Result<(), PluginError> {
            self.values
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .insert(
                    FakeSecrets::account(plugin_key, scope, repo_id, key),
                    value.to_string(),
                );
            Ok(())
        }

        fn delete(
            &self,
            plugin_key: &str,
            scope: PluginScope,
            repo_id: Option<&str>,
            key: &str,
        ) -> Result<(), PluginError> {
            self.values
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .remove(&FakeSecrets::account(plugin_key, scope, repo_id, key));
            Ok(())
        }
    }

    struct FakeHostServices {
        storage: StorageStore,
        settings: PluginSettingsStore,
        secrets: FakeSecrets,
        notifications: Mutex<Vec<(String, String, Option<String>)>>,
        subscriptions: Mutex<HashMap<String, Vec<String>>>,
        open_tabs: Mutex<Vec<(PluginScope, Option<String>, String)>>,
    }

    impl Default for FakeHostServices {
        fn default() -> Self {
            let data_dir = std::env::temp_dir().join("ade-fake-host-services");
            Self {
                storage: StorageStore::new(&data_dir),
                settings: PluginSettingsStore::new(&data_dir),
                secrets: FakeSecrets::default(),
                notifications: Mutex::new(Vec::new()),
                subscriptions: Mutex::new(HashMap::new()),
                open_tabs: Mutex::new(Vec::new()),
            }
        }
    }

    impl PluginHostServices for FakeHostServices {
        fn active_worktree_context(&self) -> Option<WorktreeContext> {
            Some(WorktreeContext {
                worktree_id: "wt-1".to_string(),
                branch: "main".to_string(),
                display_name: "main".to_string(),
            })
        }

        fn git_log(&self, _limit: Option<u32>) -> Result<serde_json::Value, HostServiceError> {
            Ok(serde_json::json!({ "items": [], "hasIncomingChanges": false, "hasOutgoingChanges": false, "hasMore": false, "limit": 50 }))
        }

        fn git_commit_files(&self, _oid: &str) -> Result<serde_json::Value, HostServiceError> {
            Ok(serde_json::json!([]))
        }

        fn list_worktree_terminals(&self, _worktree_id: &str) -> Vec<TerminalRef> {
            Vec::new()
        }

        fn send_terminal_text(
            &self,
            _terminal_id: &str,
            _text: &str,
            _enter: bool,
        ) -> Result<bool, HostServiceError> {
            Err(HostServiceError::new("unavailable", "fake host"))
        }

        fn dispatch_notification(
            &self,
            plugin_key: &str,
            title: &str,
            body: Option<&str>,
        ) -> Result<bool, HostServiceError> {
            self.notifications
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .push((
                    plugin_key.to_string(),
                    title.to_string(),
                    body.map(str::to_string),
                ));
            Ok(true)
        }

        fn storage(&self) -> &dyn PluginStorageAccess {
            &self.storage
        }

        fn settings(&self) -> &dyn PluginSettingsAccess {
            &self.settings
        }

        fn secrets(&self) -> &dyn SecretStore {
            &self.secrets
        }

        fn subscribe_events(&self, plugin_key: &str, events: &[String]) -> Vec<String> {
            self.subscriptions
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .entry(plugin_key.to_string())
                .or_default()
                .extend(events.iter().cloned());
            events.to_vec()
        }

        fn subscribed_events(&self, plugin_key: &str) -> Vec<String> {
            self.subscriptions
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .get(plugin_key)
                .cloned()
                .unwrap_or_default()
        }

        fn open_tab(
            &self,
            _plugin_key: &str,
            scope: PluginScope,
            repo_id: Option<&str>,
            tab_id: &str,
            _title: Option<&str>,
            _params: Option<Value>,
            _worktree_hint: Option<&str>,
        ) -> Result<crate::host::services::OpenTabResult, HostServiceError> {
            self.open_tabs
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .push((scope, repo_id.map(str::to_string), tab_id.to_string()));
            Ok(crate::host::services::OpenTabResult {
                instance_id: format!("{scope:?}::{tab_id}"),
                worktree_id: "wt-1".to_string(),
            })
        }

        fn declared_tabs(
            &self,
            _plugin_key: &str,
            _scope: PluginScope,
            _repo_id: Option<&str>,
        ) -> Vec<String> {
            vec!["board".to_string()]
        }

        fn session_post(
            &self,
            _plugin_key: &str,
            _session_token: &str,
            _topic: Option<&str>,
            _payload: Value,
        ) -> Result<(), HostServiceError> {
            Ok(())
        }

        fn invoke_worker_command(
            &self,
            _plugin_key: &str,
            _scope: PluginScope,
            _repo_id: Option<&str>,
            _command_id: &str,
            _args: Option<Value>,
            _session_token: Option<&str>,
        ) -> Result<Value, HostServiceError> {
            Ok(serde_json::json!({ "started": true }))
        }
    }

    #[derive(Default)]
    struct RecordingSink {
        changes: Mutex<Vec<(bool, bool)>>,
        tabs: Mutex<Vec<OpenTabRequest>>,
        pushes: Mutex<Vec<PanelPushRequest>>,
    }

    impl ServiceEventSink for RecordingSink {
        fn plugins_changed(&self, content_packs_changed: bool, plugins_changed: bool) {
            self.changes
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .push((content_packs_changed, plugins_changed));
        }

        fn open_tab_requested(&self, request: OpenTabRequest) {
            self.tabs
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .push(request);
        }

        fn panel_push_requested(&self, request: PanelPushRequest) {
            self.pushes
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .push(request);
        }
    }

    fn write_plugin(root: &Path, id: &str, version: &str, engines: &str, extra: Value) -> PathBuf {
        let mut manifest = json!({
            "manifestVersion": 1,
            "id": id,
            "publisher": "orca-samples",
            "name": id,
            "version": version,
            "engines": { "orca": engines },
            "pluginApi": 1
        });
        if let Value::Object(extra) = extra {
            for (key, value) in extra {
                manifest[key] = value;
            }
        }
        let path = root.to_path_buf();
        std::fs::create_dir_all(&path).unwrap();
        std::fs::write(
            path.join("ade-plugin.json"),
            serde_json::to_vec(&manifest).unwrap(),
        )
        .unwrap();
        std::fs::write(path.join("panel.html"), b"<h1>panel</h1>").unwrap();
        path
    }

    struct Harness {
        service: Arc<PluginService>,
        consent: Arc<FakeConsentState>,
        sink: Arc<RecordingSink>,
        host: Arc<FakeHostServices>,
        app_data: PathBuf,
    }

    fn harness(temp: &Path, host_version: &str) -> Harness {
        harness_with_dev(temp, host_version, Vec::new())
    }

    fn harness_with_dev(temp: &Path, host_version: &str, dev_paths: Vec<PathBuf>) -> Harness {
        harness_full(
            temp,
            host_version,
            dev_paths,
            fast_supervisor(),
            Duration::from_millis(40),
        )
    }

    fn fast_supervisor() -> SupervisorConfig {
        SupervisorConfig {
            ready_timeout: Duration::from_secs(2),
            invoke_timeout: Duration::from_millis(300),
            event_timeout: Duration::from_millis(300),
            idle_timeout: Duration::from_secs(30),
            grace: Duration::from_millis(100),
            max_restarts: 2,
            backoff: [
                Duration::from_millis(10),
                Duration::from_millis(20),
                Duration::from_millis(30),
            ],
        }
    }

    fn harness_full(
        temp: &Path,
        host_version: &str,
        dev_paths: Vec<PathBuf>,
        supervisor: SupervisorConfig,
        watch_batch: Duration,
    ) -> Harness {
        let app_data = temp.join("app-data");
        let consent = Arc::new(FakeConsentState::default());
        let sink = Arc::new(RecordingSink::default());
        let data_dir = app_data.join("plugin-data");
        let host = Arc::new(FakeHostServices {
            storage: StorageStore::new(&data_dir),
            settings: PluginSettingsStore::new(&data_dir),
            secrets: FakeSecrets::default(),
            ..FakeHostServices::default()
        });
        let service = PluginService::new(ServiceConfig {
            app_data_dir: app_data.clone(),
            dev_paths,
            projects: Vec::new(),
            host_entry_path: temp.join("host-entry.mjs"),
            node_override: None,
            consent_state: consent.clone(),
            secrets: Arc::new(FakeSecrets::default()),
            host_services: host.clone() as Arc<dyn PluginHostServices>,
            event_sink: sink.clone(),
            host_version: host_version.to_string(),
            workers: WorkerRuntimeConfig {
                max_active: 5,
                supervisor,
                watch_batch,
            },
        });
        Harness {
            service,
            consent,
            sink,
            host,
            app_data,
        }
    }

    fn global_target() -> ScopeTarget {
        ScopeTarget {
            scope: PluginScope::Global,
            repo_id: None,
        }
    }

    fn project_target(repo_id: &str) -> ScopeTarget {
        ScopeTarget {
            scope: PluginScope::Project,
            repo_id: Some(repo_id.to_string()),
        }
    }

    fn install_local(harness: &Harness, source: &Path, target: ScopeTarget) -> InstallResult {
        harness
            .service
            .install(
                InstallSource::LocalPath {
                    path: source.to_path_buf(),
                },
                target,
            )
            .expect("install succeeds")
    }

    fn entry<'a>(entries: &'a [PluginListEntry], key: &str) -> &'a PluginListEntry {
        entries
            .iter()
            .find(|entry| entry.plugin_key == key)
            .unwrap_or_else(|| panic!("missing entry for {key}"))
    }

    #[test]
    fn projects_command_visibility() {
        let temp = tempdir().unwrap();
        let source = write_plugin(
            &temp.path().join("visibility-cmd"),
            "visibility-cmd",
            "1.0.0",
            ">=1.0.0",
            json!({
                "main": "main.mjs",
                "contributes": {
                    "commands": [
                        { "id": "check", "title": "Check" },
                        { "id": "snapshot", "title": "Snapshot", "visibility": "internal" }
                    ]
                }
            }),
        );
        std::fs::write(source.join("main.mjs"), b"// fake worker main\n").unwrap();
        let harness = harness_with_dev(temp.path(), "1.0.0", vec![source]);
        let listed = harness.service.list();
        let commands = &entry(&listed, "orca-samples.visibility-cmd").commands;
        assert_eq!(
            commands[0].visibility,
            crate::manifest::CommandVisibility::Palette
        );
        assert_eq!(
            commands[1].visibility,
            crate::manifest::CommandVisibility::Internal
        );
    }

    #[test]
    fn list_engine_gate_consent_states_and_project_masking() {
        let temp = tempdir().unwrap();
        let approved = write_plugin(
            &temp.path().join("dev-approved"),
            "approved",
            "1.0.0",
            ">=1.0.0",
            json!({ "description": "Approved plugin" }),
        );
        let pending = write_plugin(
            &temp.path().join("dev-pending"),
            "pending",
            "1.0.0",
            ">=1.0.0",
            json!({}),
        );
        let incompatible = write_plugin(
            &temp.path().join("dev-incompatible"),
            "incompatible",
            "1.0.0",
            ">=99.0.0",
            json!({}),
        );
        let harness = harness_with_dev(temp.path(), "1.0.0", vec![approved, pending, incompatible]);
        let service = &harness.service;

        let first = service.list();
        let approved_entry = entry(&first, "orca-samples.approved");
        assert_eq!(approved_entry.status, PluginStatus::Pending);
        assert!(!approved_entry.needs_reconsent);
        assert!(approved_entry.is_dev);
        assert_eq!(
            approved_entry.description.as_deref(),
            Some("Approved plugin")
        );
        assert_eq!(approved_entry.source.kind, "dev");
        assert!(approved_entry.consent_fingerprint.is_some());

        let incompatible_entry = entry(&first, "orca-samples.incompatible");
        assert_eq!(incompatible_entry.status, PluginStatus::Invalid);
        assert!(incompatible_entry
            .error
            .as_deref()
            .unwrap()
            .contains(">=99.0.0"));

        let approved_fingerprint = approved_entry.consent_fingerprint.clone().unwrap();
        harness
            .consent
            .set_consent("global:orca-samples.approved", &approved_fingerprint);
        harness
            .consent
            .set_disabled("global:orca-samples.pending", true);

        let second = service.list();
        assert_eq!(
            entry(&second, "orca-samples.approved").status,
            PluginStatus::Idle
        );
        assert_eq!(
            entry(&second, "orca-samples.pending").status,
            PluginStatus::Disabled
        );
        assert_eq!(
            entry(&second, "orca-samples.incompatible").status,
            PluginStatus::Invalid
        );
        let pending_fingerprint = entry(&second, "orca-samples.pending")
            .consent_fingerprint
            .clone()
            .unwrap();
        harness
            .consent
            .set_consent("global:orca-samples.pending", "sha256-other");
        assert!(entry(&service.list(), "orca-samples.pending").needs_reconsent);
        assert_eq!(
            entry(&service.list(), "orca-samples.pending").status,
            PluginStatus::Disabled,
            "disabled wins over a drifted fingerprint"
        );
        assert_ne!(
            pending_fingerprint, "sha256-other",
            "the fingerprint is content-derived"
        );

        // Project scope masks the same-key global plugin for its repo.
        let repo = temp.path().join("repo");
        service.set_projects(vec![("repo-1".to_string(), repo.clone())]);
        let global_source = write_plugin(
            &temp.path().join("global-source"),
            "masked",
            "1.0.0",
            ">=1.0.0",
            json!({}),
        );
        install_local(&harness, &global_source, global_target());
        let project_source = write_plugin(
            &temp.path().join("project-source"),
            "masked",
            "2.0.0",
            ">=1.0.0",
            json!({}),
        );
        install_local(&harness, &project_source, project_target("repo-1"));

        let masked = service.list();
        let matches: Vec<&PluginListEntry> = masked
            .iter()
            .filter(|entry| entry.plugin_key == "orca-samples.masked")
            .collect();
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].scope, PluginScope::Project);
        assert_eq!(matches[0].repo_id.as_deref(), Some("repo-1"));
        assert_eq!(matches[0].version, "2.0.0");
        assert_eq!(matches[0].source.kind, "local-path");

        // The global view still lists it (dev entries are separate contexts).
        let global_view: Vec<DiscoveryProjection> =
            DiscoveryProjection::project(&service.discover_all(), None)
                .into_iter()
                .filter(|entry| entry.plugin_key == "orca-samples.masked")
                .collect();
        assert_eq!(global_view.len(), 1);
        assert_eq!(global_view[0].scope, PluginScope::Global);
    }

    #[test]
    fn install_local_global_and_project_round_trips_consent_and_enablement() {
        let temp = tempdir().unwrap();
        let harness = harness(temp.path(), "1.0.0");
        let repo = temp.path().join("repo");
        harness
            .service
            .set_projects(vec![("repo-1".to_string(), repo.clone())]);

        let source = write_plugin(
            &temp.path().join("source"),
            "demo",
            "1.0.0",
            ">=1.0.0",
            json!({ "capabilities": [{ "kind": "storage" }] }),
        );

        let global = install_local(&harness, &source, global_target());
        assert_eq!(global.plugin_key, "orca-samples.demo");
        assert_eq!(global.scope, PluginScope::Global);
        let global_lock = read_lockfile(&global_roots(&harness.app_data).lockfile_path);
        assert!(global_lock.plugins.contains_key("orca-samples.demo"));

        let project = install_local(&harness, &source, project_target("repo-1"));
        assert_eq!(project.scope, PluginScope::Project);
        assert_eq!(
            std::fs::read(repo.join(".ade/.gitignore")).unwrap(),
            b"plugins/\nplugin-data/\n"
        );
        let project_lock = read_lockfile(&project_roots(&repo).lockfile_path);
        assert!(project_lock.plugins.contains_key("orca-samples.demo"));

        // The registered project masks the global install in its context.
        let listed = harness.service.list();
        let entries: Vec<&PluginListEntry> = listed
            .iter()
            .filter(|entry| entry.plugin_key == "orca-samples.demo")
            .collect();
        assert_eq!(entries.len(), 1);
        let project_entry = entries[0];
        assert_eq!(project_entry.scope, PluginScope::Project);
        assert_eq!(project_entry.repo_id.as_deref(), Some("repo-1"));
        assert_eq!(project_entry.status, PluginStatus::Pending);
        assert_eq!(project_entry.capabilities.len(), 1);
        assert_eq!(project_entry.capabilities[0].kind, "storage");
        assert!(project_entry.capabilities[0]
            .description
            .contains("storage folder"));
        assert_eq!(
            project_entry.install_hash.as_deref(),
            Some(project.content_hash.as_str())
        );
        let fingerprint = project_entry.consent_fingerprint.clone().unwrap();

        // Consent approves only the project scope.
        let listed = harness
            .service
            .consent(ConsentRequest {
                plugin_ref: PluginRef {
                    plugin_key: "orca-samples.demo".to_string(),
                    scope: PluginScope::Project,
                    repo_id: Some("repo-1".to_string()),
                },
                reviewed_fingerprint: fingerprint.clone(),
                decision: ConsentDecision::Approve,
            })
            .unwrap();
        assert_eq!(
            entry(&listed, "orca-samples.demo").status,
            PluginStatus::Idle
        );
        assert_eq!(
            harness
                .consent
                .consent_for("project:repo-1:orca-samples.demo"),
            Some(fingerprint.clone())
        );
        assert_eq!(
            harness.consent.consent_for("global:orca-samples.demo"),
            None
        );

        // A stale reviewed fingerprint is refused.
        let error = harness
            .service
            .consent(ConsentRequest {
                plugin_ref: PluginRef {
                    plugin_key: "orca-samples.demo".to_string(),
                    scope: PluginScope::Global,
                    repo_id: None,
                },
                reviewed_fingerprint: "sha256-stale".to_string(),
                decision: ConsentDecision::Approve,
            })
            .unwrap_err();
        assert_eq!(error.code(), crate::types::CONSENT_STALE);

        // Enablement round-trips through the scoped disable bit.
        let listed = harness
            .service
            .set_enabled(
                PluginRef {
                    plugin_key: "orca-samples.demo".to_string(),
                    scope: PluginScope::Project,
                    repo_id: Some("repo-1".to_string()),
                },
                false,
            )
            .unwrap();
        assert_eq!(
            entry(&listed, "orca-samples.demo").status,
            PluginStatus::Disabled
        );
        let listed = harness
            .service
            .set_enabled(
                PluginRef {
                    plugin_key: "orca-samples.demo".to_string(),
                    scope: PluginScope::Project,
                    repo_id: Some("repo-1".to_string()),
                },
                true,
            )
            .unwrap();
        assert_eq!(
            entry(&listed, "orca-samples.demo").status,
            PluginStatus::Idle
        );

        // Removal drops install, data, and lock entry.
        harness
            .service
            .storage_set(
                &PluginRef {
                    plugin_key: "orca-samples.demo".to_string(),
                    scope: PluginScope::Global,
                    repo_id: None,
                },
                "k",
                json!("v"),
            )
            .unwrap();
        let listed = harness
            .service
            .remove(PluginRef {
                plugin_key: "orca-samples.demo".to_string(),
                scope: PluginScope::Global,
                repo_id: None,
            })
            .unwrap();
        assert!(!listed
            .iter()
            .any(|entry| entry.plugin_key == "orca-samples.demo"
                && entry.scope == PluginScope::Global));
        let global_lock = read_lockfile(&global_roots(&harness.app_data).lockfile_path);
        assert!(!global_lock.plugins.contains_key("orca-samples.demo"));

        let changes = harness
            .sink
            .changes
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone();
        assert!(changes.contains(&(false, true)));
    }

    #[test]
    fn mutation_locks_serialize_same_key_and_independent_maps() {
        let locks = Arc::new(MutationLocks::default());
        let install_root = PathBuf::from("/tmp/ade-test-root");
        let data_dir = PathBuf::from("/tmp/ade-test-data");

        let (held_tx, held_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel::<()>();
        let (acquired_tx, acquired_rx) = mpsc::channel::<()>();
        let (attempt_tx, attempt_rx) = mpsc::channel::<()>();

        let holder_locks = locks.clone();
        let holder_root = install_root.clone();
        let holder = thread::spawn(move || {
            holder_locks.with_install_root(&holder_root, || {
                held_tx.send(()).unwrap();
                release_rx.recv().unwrap();
            });
        });
        held_rx.recv().unwrap();

        let waiter_locks = locks.clone();
        let waiter_root = install_root.clone();
        let waiter_acquired = acquired_tx.clone();
        let waiter = thread::spawn(move || {
            attempt_tx.send(()).unwrap();
            waiter_locks.with_install_root(&waiter_root, || {
                waiter_acquired.send(()).unwrap();
            });
        });
        attempt_rx.recv().unwrap();
        thread::sleep(Duration::from_millis(50));
        assert!(
            acquired_rx
                .recv_timeout(Duration::from_millis(100))
                .is_err(),
            "a second mutation on the same install root must wait"
        );

        // An unrelated map (plugin data) is not blocked by the install root.
        locks.with_plugin_data(&data_dir, "orca-samples.demo", || {
            acquired_tx.send(()).unwrap();
        });
        acquired_rx
            .recv_timeout(Duration::from_millis(100))
            .expect("plugin-data lock is independent of the install-root lock");

        release_tx.send(()).unwrap();
        holder.join().unwrap();
        acquired_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("waiting mutation acquires after release");
        waiter.join().unwrap();
    }

    #[test]
    fn concurrent_same_key_installs_serialize_and_stay_consistent() {
        let temp = tempdir().unwrap();
        let harness = harness(temp.path(), "1.0.0");
        let source = write_plugin(
            &temp.path().join("source"),
            "demo",
            "1.0.0",
            ">=1.0.0",
            json!({}),
        );

        let mut handles = Vec::new();
        for _ in 0..2 {
            let service = harness.service.clone();
            let path = source.clone();
            handles.push(thread::spawn(move || {
                service.install(
                    InstallSource::LocalPath { path },
                    ScopeTarget {
                        scope: PluginScope::Global,
                        repo_id: None,
                    },
                )
            }));
        }
        for handle in handles {
            handle.join().unwrap().expect("concurrent install succeeds");
        }

        let lock = read_lockfile(&global_roots(&harness.app_data).lockfile_path);
        assert_eq!(lock.plugins.len(), 1);
        assert_eq!(lock.plugins["orca-samples.demo"].version, "1.0.0");
    }

    #[test]
    fn lockfile_state_matrix_and_install_missing_restore() {
        let temp = tempdir().unwrap();
        let harness = harness(temp.path(), "1.0.0");
        let repo = temp.path().join("repo");
        harness
            .service
            .set_projects(vec![("repo-1".to_string(), repo.clone())]);

        let source = write_plugin(
            &temp.path().join("source"),
            "demo",
            "1.0.0",
            ">=1.0.0",
            json!({}),
        );
        let installed = install_local(&harness, &source, project_target("repo-1"));
        let roots = project_roots(&repo);

        // Fresh-clone simulation: drop the install directory, keep the lockfile.
        std::fs::remove_dir_all(roots.install_dir.join("orca-samples.demo")).unwrap();
        let state = harness.service.lockfile_state("repo-1");
        assert_eq!(state.repo_id, "repo-1");
        assert_eq!(state.lockfile_path, roots.lockfile_path.to_string_lossy());
        assert_eq!(state.entries.len(), 1);
        assert_eq!(state.entries[0].status, LockEntryStatus::Missing);

        let results = harness.service.install_missing_from_lockfile("repo-1");
        assert_eq!(results.len(), 1);
        let restored = results[0]
            .as_ref()
            .expect("restored from local lock source");
        assert_eq!(restored.content_hash, installed.content_hash);
        let state = harness.service.lockfile_state("repo-1");
        assert_eq!(state.entries[0].status, LockEntryStatus::Installed);

        // Hash mismatch: tamper the recorded content hash.
        let lock_path = &roots.lockfile_path;
        let mut raw: Value = serde_json::from_slice(&std::fs::read(lock_path).unwrap()).unwrap();
        raw["plugins"]["orca-samples.demo"]["contentHash"] = json!("ab".repeat(32));
        std::fs::write(lock_path, serde_json::to_vec(&raw).unwrap()).unwrap();
        let state = harness.service.lockfile_state("repo-1");
        assert_eq!(state.entries[0].status, LockEntryStatus::HashMismatch);
        let results = harness.service.install_missing_from_lockfile("repo-1");
        assert_eq!(results.len(), 1);
        assert!(results[0].is_ok());
        assert_eq!(
            harness.service.lockfile_state("repo-1").entries[0].status,
            LockEntryStatus::Installed
        );

        // Missing + unavailable sources: craft two extra lock entries.
        let missing_source = write_plugin(
            &temp.path().join("missing-source"),
            "second",
            "1.0.0",
            ">=1.0.0",
            json!({}),
        );
        let mut raw: Value = serde_json::from_slice(&std::fs::read(lock_path).unwrap()).unwrap();
        raw["plugins"]["orca-samples.second"] = json!({
            "pluginKey": "orca-samples.second",
            "version": "1.0.0",
            "source": { "kind": "local-path", "path": missing_source.to_string_lossy() },
            "contentHash": "cd".repeat(32),
            "capabilityHash": "sha256-x",
            "installedAt": 0
        });
        raw["plugins"]["orca-samples.gone"] = json!({
            "pluginKey": "orca-samples.gone",
            "version": "1.0.0",
            "source": { "kind": "local-path", "path": temp.path().join("nope").to_string_lossy() },
            "contentHash": "ef".repeat(32),
            "capabilityHash": "sha256-x",
            "installedAt": 0
        });
        std::fs::write(lock_path, serde_json::to_vec(&raw).unwrap()).unwrap();

        let state = harness.service.lockfile_state("repo-1");
        let by_key = |key: &str| {
            state
                .entries
                .iter()
                .find(|entry| entry.plugin_key == key)
                .unwrap()
        };
        assert_eq!(
            by_key("orca-samples.second").status,
            LockEntryStatus::Missing
        );
        assert_eq!(
            by_key("orca-samples.gone").status,
            LockEntryStatus::SourceUnavailable
        );
        assert!(by_key("orca-samples.gone")
            .detail
            .as_deref()
            .unwrap()
            .contains("unavailable"));

        let results = harness.service.install_missing_from_lockfile("repo-1");
        assert_eq!(results.len(), 2);
        assert!(results.iter().any(|result| result.is_ok()));
        assert!(results
            .iter()
            .any(|result| result.as_ref().unwrap_err().code() == UNAVAILABLE));
    }

    #[test]
    fn get_logs_reads_audit_entries_written_by_mutations() {
        let temp = tempdir().unwrap();
        let harness = harness(temp.path(), "1.0.0");
        let source = write_plugin(
            &temp.path().join("source"),
            "demo",
            "1.0.0",
            ">=1.0.0",
            json!({}),
        );
        assert!(harness.service.get_logs("orca-samples.demo").is_empty());

        install_local(&harness, &source, global_target());
        let lines = harness.service.get_logs("orca-samples.demo");
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].method, "plugins.install");
        assert_eq!(lines[0].outcome, AuditOutcome::Ok);

        harness
            .service
            .set_enabled(
                PluginRef {
                    plugin_key: "orca-samples.demo".to_string(),
                    scope: PluginScope::Global,
                    repo_id: None,
                },
                true,
            )
            .unwrap();
        let lines = harness.service.get_logs("orca-samples.demo");
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[1].method, "plugins.set_enabled");
        assert!(lines[0].ts <= lines[1].ts);
    }

    #[test]
    fn open_tab_request_serializes_camel_case() {
        let request = OpenTabRequest {
            plugin_key: "orca-samples.demo".to_string(),
            tab_id: "board".to_string(),
            instance_id: "inst-1".to_string(),
            title: Some("Board".to_string()),
            params: json!({ "filter": "mine" }),
            worktree_id: "wt-1".to_string(),
            scope: PluginScope::Project,
            repo_id: Some("repo-1".to_string()),
        };
        assert_eq!(
            serde_json::to_value(&request).unwrap(),
            json!({
                "pluginKey": "orca-samples.demo",
                "tabId": "board",
                "instanceId": "inst-1",
                "title": "Board",
                "params": { "filter": "mine" },
                "worktreeId": "wt-1",
                "scope": "project",
                "repoId": "repo-1"
            })
        );

        let sink = RecordingSink::default();
        sink.open_tab_requested(request.clone());
        assert_eq!(
            sink.tabs
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .len(),
            1
        );
    }

    #[test]
    fn storage_and_secret_wrappers_round_trip_per_scope() {
        let temp = tempdir().unwrap();
        let harness = harness(temp.path(), "1.0.0");
        let plugin_ref = PluginRef {
            plugin_key: "orca-samples.demo".to_string(),
            scope: PluginScope::Global,
            repo_id: None,
        };
        harness
            .service
            .storage_set(&plugin_ref, "k", json!(1))
            .unwrap();
        harness
            .service
            .settings_set(&plugin_ref, "s", json!("v"))
            .unwrap();
        harness
            .service
            .secrets_set(&plugin_ref, "token", "secret")
            .unwrap();
        assert_eq!(
            harness
                .service
                .storage
                .get("orca-samples.demo", "k")
                .unwrap(),
            Some(json!(1))
        );
        assert_eq!(
            harness
                .service
                .settings
                .get_all("orca-samples.demo")
                .unwrap()
                .get("s"),
            Some(&json!("v"))
        );
        harness.service.storage_delete(&plugin_ref, "k").unwrap();
        harness
            .service
            .secrets_delete(&plugin_ref, "token")
            .unwrap();
        assert_eq!(
            harness
                .service
                .storage
                .get("orca-samples.demo", "k")
                .unwrap(),
            None
        );
    }

    // ------------------------------------------------------------------
    // T11b2 worker activation orchestration
    // ------------------------------------------------------------------

    /// Stand-in for `host-entry.mjs`: registers exactly the manifest's
    /// worker commands, writes `worker.pid` before `ready`, logs each handled
    /// command, never replies to `slow`, answers `host-call` through
    /// `notifications.show`, and appends delivered events to `events.log`.
    const FAKE_WORKER: &str = r#"import { createInterface } from 'node:readline';
import { appendFileSync, readFileSync, writeFileSync } from 'node:fs';

const write = (frame) => process.stdout.write(JSON.stringify(frame) + '\n');
const manifest = JSON.parse(readFileSync('ade-plugin.json', 'utf8'));
const commands = (manifest.contributes?.commands ?? []).map((command) => command.id);
const pending = new Map();

writeFileSync('worker.pid', String(process.pid));
write({ type: 'ready', commands });

const rl = createInterface({ input: process.stdin });
rl.on('line', (line) => {
  let frame;
  try { frame = JSON.parse(line); } catch { return; }
  if (frame.type === 'invokeCommand') {
    if (frame.commandId === 'slow') return;
    if (frame.commandId === 'open-tab') {
      void (async () => {
        const value = await new Promise((resolve) => {
          pending.set(9200, resolve);
          write({ type: 'hostCall', callId: 9200, method: 'workspace.openTab', params: { tabId: 'board' } });
        });
        write({ type: 'commandResult', callId: frame.callId, ok: true, value: { opened: value } });
      })();
      return;
    }
    if (frame.commandId === 'host-call') {
      void (async () => {
        const value = await new Promise((resolve) => {
          pending.set(9100, resolve);
          write({ type: 'hostCall', callId: 9100, method: 'notifications.show', params: { title: 'plugin says hi' } });
        });
        write({ type: 'commandResult', callId: frame.callId, ok: true, value: { host: value } });
      })();
      return;
    }
    if (frame.commandId === 'subscribe') {
      void (async () => {
        const value = await new Promise((resolve) => {
          pending.set(9300, resolve);
          write({ type: 'hostCall', callId: 9300, method: 'events.subscribe', params: { events: ['worktree.created'] } });
        });
        write({ type: 'commandResult', callId: frame.callId, ok: true, value: { subscribed: value } });
      })();
      return;
    }
    write({ type: 'log', level: 'info', message: `${frame.commandId} handled` });
    write({ type: 'commandResult', callId: frame.callId, ok: true, value: { command: frame.commandId, args: frame.args ?? null } });
    return;
  }
  if (frame.type === 'deliverEvent') {
    appendFileSync('events.log', JSON.stringify({ event: frame.event, payload: frame.payload }) + '\n');
    write({ type: 'eventAck', eventId: frame.eventId });
    return;
  }
  if (frame.type === 'hostResult') {
    const resolve = pending.get(frame.callId);
    if (resolve) { pending.delete(frame.callId); resolve(frame.ok ? frame.value : null); }
    return;
  }
  if (frame.type === 'shutdown') process.exit(0);
});
"#;

    fn node_available() -> bool {
        resolve_node(None, None).is_ok()
    }

    fn install_fake_host_entry(temp: &Path) -> PathBuf {
        let path = temp.join("host-entry.mjs");
        std::fs::write(&path, FAKE_WORKER).unwrap();
        path
    }

    /// Dev/installed plugin fixture with a worker entry, declared commands,
    /// event contributions, and capabilities.
    fn write_worker_plugin(
        root: &Path,
        id: &str,
        commands: &[&str],
        events: &[&str],
        capabilities: &[&str],
    ) -> PathBuf {
        let commands: Vec<Value> = commands
            .iter()
            .map(|id| json!({ "id": id, "title": id }))
            .collect();
        let events: Vec<Value> = events.iter().map(|on| json!({ "on": on })).collect();
        let capabilities: Vec<Value> = capabilities
            .iter()
            .map(|kind| json!({ "kind": kind }))
            .collect();
        let path = write_plugin(
            root,
            id,
            "1.0.0",
            ">=1.0.0",
            json!({
                "main": "main.mjs",
                "contributes": { "commands": commands, "events": events },
                "capabilities": capabilities
            }),
        );
        std::fs::write(path.join("main.mjs"), b"// fake worker main\n").unwrap();
        path
    }

    fn approve(harness: &Harness, plugin_ref: &PluginRef) {
        let entry = harness
            .service
            .list()
            .into_iter()
            .find(|entry| {
                entry.plugin_key == plugin_ref.plugin_key
                    && entry.scope == plugin_ref.scope
                    && entry.repo_id == plugin_ref.repo_id
            })
            .expect("a discovered entry to approve");
        let scoped = scoped_key(
            &plugin_ref.plugin_key,
            consent_scope_of(plugin_ref.scope, plugin_ref.repo_id.as_deref()),
        )
        .unwrap();
        harness
            .consent
            .set_consent(&scoped, &entry.consent_fingerprint.unwrap());
    }

    fn wait_until(deadline: Duration, mut condition: impl FnMut() -> bool) -> bool {
        let stop = Instant::now() + deadline;
        while Instant::now() < stop {
            if condition() {
                return true;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        condition()
    }

    fn worker_pid(root: &Path) -> Option<u32> {
        std::fs::read_to_string(root.join("worker.pid"))
            .ok()
            .and_then(|pid| pid.trim().parse().ok())
    }

    fn global_ref(key: &str) -> PluginRef {
        PluginRef {
            plugin_key: key.to_string(),
            scope: PluginScope::Global,
            repo_id: None,
        }
    }

    #[test]
    fn activation_gating_spawns_only_approved_and_projects_status() {
        if !node_available() {
            return;
        }
        let temp = tempdir().unwrap();
        install_fake_host_entry(temp.path());
        let root =
            write_worker_plugin(&temp.path().join("dev-gated"), "gated", &["echo"], &[], &[]);
        let harness = harness_with_dev(temp.path(), "1.0.0", vec![root.clone()]);
        let plugin_ref = global_ref("orca-samples.gated");

        assert_eq!(
            entry(&harness.service.list(), "orca-samples.gated").status,
            PluginStatus::Pending
        );
        let error = harness.service.ensure_active(&plugin_ref).unwrap_err();
        assert_eq!(error.code(), gate::CONSENT_REQUIRED);
        assert!(!root.join("worker.pid").exists());

        harness
            .consent
            .set_disabled("global:orca-samples.gated", true);
        assert_eq!(
            entry(&harness.service.list(), "orca-samples.gated").status,
            PluginStatus::Disabled
        );
        assert!(harness.service.ensure_active(&plugin_ref).is_err());
        assert!(!root.join("worker.pid").exists());

        harness
            .consent
            .set_disabled("global:orca-samples.gated", false);
        approve(&harness, &plugin_ref);
        assert_eq!(
            entry(&harness.service.list(), "orca-samples.gated").status,
            PluginStatus::Idle
        );
        harness.service.ensure_active(&plugin_ref).unwrap();
        assert_eq!(
            entry(&harness.service.list(), "orca-samples.gated").status,
            PluginStatus::Running
        );
        assert!(root.join("worker.pid").exists());

        harness
            .service
            .set_enabled(plugin_ref.clone(), false)
            .unwrap();
        assert_eq!(
            entry(&harness.service.list(), "orca-samples.gated").status,
            PluginStatus::Disabled
        );
        assert!(
            wait_until(Duration::from_secs(2), || matches!(
                harness.service.manager().state("global:orca-samples.gated"),
                WorkerState::Inactive
            )),
            "disabling must stop the worker"
        );
        harness.service.shutdown();
    }

    #[test]
    fn invoke_command_end_to_end_with_timeout_and_undeclared_command() {
        if !node_available() {
            return;
        }
        let temp = tempdir().unwrap();
        install_fake_host_entry(temp.path());
        let _root = write_worker_plugin(
            &temp.path().join("dev-invoke"),
            "invoker",
            &["echo", "slow"],
            &[],
            &[],
        );
        let harness = harness_with_dev(temp.path(), "1.0.0", vec![_root.clone()]);
        let plugin_ref = global_ref("orca-samples.invoker");
        approve(&harness, &plugin_ref);

        let value = harness
            .service
            .invoke_command(&plugin_ref, "echo", Some(json!({ "n": 3 })))
            .unwrap();
        assert_eq!(value, json!({ "command": "echo", "args": { "n": 3 } }));

        let logs = harness.service.get_logs("orca-samples.invoker");
        assert!(
            logs.iter()
                .any(|line| line.method == "worker.info" && line.summary == "echo handled"),
            "worker log frames must merge into get_logs: {logs:?}"
        );

        let error = harness
            .service
            .invoke_command(&plugin_ref, "not-declared", None)
            .unwrap_err();
        assert_eq!(error.code(), UNKNOWN_COMMAND);

        let error = harness
            .service
            .invoke_command(&plugin_ref, "slow", None)
            .unwrap_err();
        assert!(
            error.message().contains("did not reply"),
            "timeout error must name the missing reply: {}",
            error.message()
        );
        harness.service.shutdown();
    }

    /// Overwrites the immutable installed worker payload of `plugin_key` and
    /// returns its version directory (the worker's spawn cwd).
    fn tamper_installed_worker(harness: &Harness, plugin_key: &str) -> PathBuf {
        let install_hash = harness
            .service
            .list()
            .into_iter()
            .find(|entry| entry.plugin_key == plugin_key)
            .and_then(|entry| entry.install_hash)
            .expect("installed content hash");
        let version_dir = harness
            .app_data
            .join("plugins")
            .join(plugin_key)
            .join(install_hash);
        std::fs::write(version_dir.join("main.mjs"), b"// tampered worker bytes\n").unwrap();
        version_dir
    }

    #[test]
    fn worker_activation_rejects_a_tampered_installed_payload() {
        let temp = tempdir().unwrap();
        let source = write_worker_plugin(
            &temp.path().join("source"),
            "tampered-worker",
            &["echo"],
            &["worktree.created"],
            &["events:subscribe"],
        );
        let harness = harness(temp.path(), "1.0.0");
        install_local(&harness, &source, global_target());
        let plugin_ref = global_ref("orca-samples.tampered-worker");
        approve(&harness, &plugin_ref);
        let version_dir = tamper_installed_worker(&harness, "orca-samples.tampered-worker");

        let error = harness.service.ensure_active(&plugin_ref).unwrap_err();
        assert_eq!(error.code(), crate::types::INTEGRITY_MISMATCH);
        assert!(
            !version_dir.join("worker.pid").exists(),
            "a tampered payload must not spawn a worker"
        );
        assert!(matches!(
            harness
                .service
                .manager()
                .state("global:orca-samples.tampered-worker"),
            WorkerState::Inactive
        ));

        // Static-event delivery activates subscribers through the same
        // chokepoint: a tampered subscriber must never be started.
        assert_eq!(
            harness.service.deliver_plugin_event(
                "worktree.created",
                json!({ "worktreeId": "w1", "path": "/tmp/wt", "branch": "main" })
            ),
            0,
            "a tampered static subscriber must not receive the event"
        );
        assert!(!version_dir.join("worker.pid").exists());
    }

    #[test]
    fn invoke_command_rejects_a_tampered_installed_payload() {
        let temp = tempdir().unwrap();
        let source = write_worker_plugin(
            &temp.path().join("source"),
            "tampered-invoker",
            &["echo"],
            &[],
            &[],
        );
        let harness = harness(temp.path(), "1.0.0");
        install_local(&harness, &source, global_target());
        let plugin_ref = global_ref("orca-samples.tampered-invoker");
        approve(&harness, &plugin_ref);
        let version_dir = tamper_installed_worker(&harness, "orca-samples.tampered-invoker");

        let error = harness
            .service
            .invoke_command(&plugin_ref, "echo", Some(json!({ "n": 1 })))
            .unwrap_err();
        assert_eq!(error.code(), crate::types::INTEGRITY_MISMATCH);
        assert!(!version_dir.join("worker.pid").exists());
        assert!(matches!(
            harness
                .service
                .manager()
                .state("global:orca-samples.tampered-invoker"),
            WorkerState::Inactive
        ));
    }

    #[test]
    fn healthy_installed_worker_activates_and_invokes() {
        if !node_available() {
            return;
        }
        let temp = tempdir().unwrap();
        install_fake_host_entry(temp.path());
        let source = write_worker_plugin(
            &temp.path().join("source"),
            "installed-worker",
            &["echo"],
            &[],
            &[],
        );
        let harness = harness(temp.path(), "1.0.0");
        install_local(&harness, &source, global_target());
        let plugin_ref = global_ref("orca-samples.installed-worker");
        approve(&harness, &plugin_ref);

        harness.service.ensure_active(&plugin_ref).unwrap();
        assert!(matches!(
            harness
                .service
                .manager()
                .state("global:orca-samples.installed-worker"),
            WorkerState::Running { .. }
        ));
        let value = harness
            .service
            .invoke_command(&plugin_ref, "echo", Some(json!({ "n": 1 })))
            .unwrap();
        assert_eq!(value, json!({ "command": "echo", "args": { "n": 1 } }));
        harness.service.shutdown();
    }

    #[test]
    fn dev_worker_skips_integrity_verification_for_live_edits() {
        if !node_available() {
            return;
        }
        let temp = tempdir().unwrap();
        install_fake_host_entry(temp.path());
        let root = write_worker_plugin(&temp.path().join("dev-live"), "live", &["echo"], &[], &[]);
        let harness = harness_with_dev(temp.path(), "1.0.0", vec![root.clone()]);
        let plugin_ref = global_ref("orca-samples.live");
        approve(&harness, &plugin_ref);

        // Dev trees are not hash-addressed and change on disk while Orca
        // runs, so they bypass integrity verification (oracle parity).
        std::fs::write(root.join("main.mjs"), b"// live dev edit\n").unwrap();
        harness.service.ensure_active(&plugin_ref).unwrap();
        assert!(matches!(
            harness.service.manager().state("global:orca-samples.live"),
            WorkerState::Running { .. }
        ));
        harness.service.shutdown();
    }

    #[test]
    fn host_calls_route_through_host_methods_with_the_activation_context() {
        if !node_available() {
            return;
        }
        let temp = tempdir().unwrap();
        install_fake_host_entry(temp.path());
        let _root = write_worker_plugin(
            &temp.path().join("dev-host-call"),
            "caller",
            &["host-call"],
            &[],
            &["notifications:show"],
        );
        let harness = harness_with_dev(temp.path(), "1.0.0", vec![_root.clone()]);
        let plugin_ref = global_ref("orca-samples.caller");
        approve(&harness, &plugin_ref);

        let value = harness
            .service
            .invoke_command(&plugin_ref, "host-call", None)
            .unwrap();
        assert_eq!(value, json!({ "host": { "delivered": true } }));
        let notifications = harness
            .host
            .notifications
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        assert_eq!(
            notifications.as_slice(),
            [(
                "orca-samples.caller".to_string(),
                "plugin says hi".to_string(),
                None
            )]
        );
        harness.service.shutdown();
    }

    #[test]
    fn deliver_plugin_event_reaches_subscribers_with_projected_payloads() {
        if !node_available() {
            return;
        }
        let temp = tempdir().unwrap();
        install_fake_host_entry(temp.path());
        let subscriber = write_worker_plugin(
            &temp.path().join("dev-sub"),
            "subscriber",
            &["echo"],
            &["worktree.created"],
            &["events:subscribe"],
        );
        let other = write_worker_plugin(
            &temp.path().join("dev-other"),
            "other",
            &["echo"],
            &["worktree.removed"],
            &["events:subscribe"],
        );
        let harness = harness_with_dev(
            temp.path(),
            "1.0.0",
            vec![subscriber.clone(), other.clone()],
        );
        let subscriber_ref = global_ref("orca-samples.subscriber");
        approve(&harness, &subscriber_ref);
        approve(&harness, &global_ref("orca-samples.other"));

        assert_eq!(
            harness
                .service
                .deliver_plugin_event("plugin.log", json!({})),
            0
        );
        assert_eq!(
            harness.service.deliver_plugin_event(
                "worktree.created",
                json!({ "worktreeId": "", "path": "/tmp/wt", "branch": "main" })
            ),
            0,
            "invalid payloads are dropped before any delivery"
        );

        let delivered = harness.service.deliver_plugin_event(
            "worktree.created",
            json!({
                "worktreeId": "w1",
                "path": "/tmp/wt",
                "branch": "main",
                "extra": "dropped"
            }),
        );
        assert_eq!(
            delivered, 1,
            "only the subscribed plugin receives the event"
        );

        let marker = subscriber.join("events.log");
        assert!(
            wait_until(Duration::from_secs(2), || marker.exists()),
            "subscriber worker must record the delivery"
        );
        let recorded: Value = serde_json::from_str(
            std::fs::read_to_string(&marker)
                .unwrap()
                .lines()
                .next()
                .unwrap(),
        )
        .unwrap();
        assert_eq!(recorded["event"], json!("worktree.created"));
        assert_eq!(
            recorded["payload"],
            json!({ "worktreeId": "w1", "path": "/tmp/wt", "branch": "main" }),
            "the worker receives the bounded projection, not the raw payload"
        );
        assert!(
            !other.join("worker.pid").exists(),
            "an unsubscribed plugin must not be activated"
        );
        harness.service.shutdown();
    }

    #[test]
    fn set_projects_activates_and_removal_stops_workers_and_sessions() {
        if !node_available() {
            return;
        }
        let temp = tempdir().unwrap();
        install_fake_host_entry(temp.path());
        let harness = harness_with_dev(temp.path(), "1.0.0", vec![]);
        let repo = temp.path().join("repo");
        harness
            .service
            .set_projects(vec![("repo-1".to_string(), repo.clone())]);

        let source = write_worker_plugin(
            &temp.path().join("source"),
            "project-worker",
            &["echo"],
            &[],
            &[],
        );
        install_local(&harness, &source, project_target("repo-1"));
        let plugin_ref = PluginRef {
            plugin_key: "orca-samples.project-worker".to_string(),
            scope: PluginScope::Project,
            repo_id: Some("repo-1".to_string()),
        };
        approve(&harness, &plugin_ref);

        // Re-registering the project activates its approved plugins.
        harness
            .service
            .set_projects(vec![("repo-1".to_string(), repo.clone())]);
        assert_eq!(harness.service.manager().active_count(), 1);
        assert!(matches!(
            harness
                .service
                .manager()
                .state("project:repo-1:orca-samples.project-worker"),
            WorkerState::Running { .. }
        ));

        let token = harness
            .service
            .sessions
            .issue(crate::sessions::SessionBinding {
                plugin_key: "orca-samples.project-worker".to_string(),
                kind: crate::sessions::SessionKind::MainTab,
                panel_id: None,
                tab_id: Some("board".to_string()),
                instance_id: Some("inst-1".to_string()),
                scope: PluginScope::Project,
                repo_id: Some("repo-1".to_string()),
                worktree_id: Some("wt-1".to_string()),
                manifest_revision: "rev-1".to_string(),
                owner: crate::sessions::SessionOwner::Worker,
            });
        assert_eq!(harness.service.sessions.len(), 1);

        harness.service.set_projects(vec![]);
        assert!(
            wait_until(Duration::from_secs(2), || harness
                .service
                .manager()
                .active_count()
                == 0),
            "removing a project must stop its workers"
        );
        assert_eq!(harness.service.sessions.len(), 0);
        assert!(harness.service.sessions.resolve(&token).is_none());
        harness.service.shutdown();
    }

    #[test]
    fn dev_watcher_hot_reloads_running_workers_on_revision_change() {
        if !node_available() {
            return;
        }
        let temp = tempdir().unwrap();
        install_fake_host_entry(temp.path());
        let root = write_worker_plugin(&temp.path().join("dev-hot"), "hot", &["echo"], &[], &[]);
        let harness = harness_with_dev(temp.path(), "1.0.0", vec![root.clone()]);
        let plugin_ref = global_ref("orca-samples.hot");
        approve(&harness, &plugin_ref);

        harness.service.start().unwrap();
        assert!(
            wait_until(Duration::from_secs(5), || worker_pid(&root).is_some()),
            "start() must activate the approved global plugin"
        );
        let first_pid = worker_pid(&root).unwrap();

        let manifest_path = root.join("ade-plugin.json");
        let mut manifest: Value =
            serde_json::from_slice(&std::fs::read(&manifest_path).unwrap()).unwrap();
        manifest["description"] = json!("revision two");
        std::fs::write(&manifest_path, serde_json::to_vec(&manifest).unwrap()).unwrap();

        assert!(
            wait_until(Duration::from_secs(10), || worker_pid(&root)
                .is_some_and(|pid| pid != first_pid)),
            "a manifest revision change must restart the dev worker"
        );
        assert!(matches!(
            harness.service.manager().state("global:orca-samples.hot"),
            WorkerState::Running { .. }
        ));
        let changes = harness
            .sink
            .changes
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone();
        assert!(
            changes.contains(&(false, true)),
            "hot reload must announce plugins_changed"
        );
        harness.service.shutdown();
    }

    #[test]
    fn pump_reaps_idle_workers_and_get_logs_merges_project_audit() {
        if !node_available() {
            return;
        }
        let temp = tempdir().unwrap();
        install_fake_host_entry(temp.path());
        let mut supervisor = fast_supervisor();
        supervisor.idle_timeout = Duration::from_millis(80);
        let harness = harness_full(
            temp.path(),
            "1.0.0",
            vec![],
            supervisor,
            Duration::from_millis(40),
        );
        let repo = temp.path().join("repo");
        harness
            .service
            .set_projects(vec![("repo-1".to_string(), repo.clone())]);
        let source = write_worker_plugin(&temp.path().join("source"), "idler", &["echo"], &[], &[]);
        install_local(&harness, &source, project_target("repo-1"));
        let plugin_ref = PluginRef {
            plugin_key: "orca-samples.idler".to_string(),
            scope: PluginScope::Project,
            repo_id: Some("repo-1".to_string()),
        };
        approve(&harness, &plugin_ref);
        harness.service.ensure_active(&plugin_ref).unwrap();
        assert_eq!(harness.service.manager().active_count(), 1);
        harness
            .service
            .invoke_command(&plugin_ref, "echo", None)
            .unwrap();

        std::thread::sleep(Duration::from_millis(140));
        harness.service.pump();
        assert_eq!(
            harness.service.manager().active_count(),
            0,
            "pump must reap the idle worker"
        );

        let logs = harness.service.get_logs("orca-samples.idler");
        assert!(
            logs.iter().any(|line| line.method == "worker.info"),
            "the worker log ring must merge into get_logs: {logs:?}"
        );
        assert!(
            logs.iter().any(|line| line.method == "plugins.install"),
            "the project-scope audit file is resolved for get_logs: {logs:?}"
        );
        harness.service.shutdown();
    }

    // ------------------------------------------------------------------
    // T11b2 fix round 1: scope-qualified worker instance identity
    // ------------------------------------------------------------------

    /// Plugin fixture declaring a worker command plus a `board` tab and the
    /// `workspace:open-tab` capability (used to observe scope routing).
    fn write_dual_scope_plugin(root: &Path, id: &str) -> PathBuf {
        let path = write_plugin(
            root,
            id,
            "1.0.0",
            ">=1.0.0",
            json!({
                "main": "main.mjs",
                "contributes": {
                    "commands": [
                        { "id": "echo", "title": "echo" },
                        { "id": "open-tab", "title": "open-tab" }
                    ],
                    "tabs": [{ "id": "board", "title": "Board", "entry": "tab.html" }]
                },
                "capabilities": [{ "kind": "workspace:open-tab" }]
            }),
        );
        std::fs::write(path.join("main.mjs"), b"// fake worker main\n").unwrap();
        std::fs::write(path.join("tab.html"), b"<h1>board</h1>").unwrap();
        path
    }

    /// Approves `plugin_ref` using the raw discovery projection (works even
    /// when the same-key global entry is masked from `list` by a project one).
    fn approve_raw(harness: &Harness, plugin_ref: &PluginRef) {
        let discovered = harness.service.discover_all();
        let entry = find_discovered(&discovered, plugin_ref).expect("installed plugin");
        let manifest = entry.manifest.as_ref().unwrap();
        let content_key = entry
            .install_hash
            .clone()
            .or_else(|| hash_plugin_tree(&entry.root).ok())
            .unwrap_or_else(|| "unresolved".to_string());
        let scoped = scoped_key(
            &plugin_ref.plugin_key,
            consent_scope_of(plugin_ref.scope, plugin_ref.repo_id.as_deref()),
        )
        .unwrap();
        harness
            .consent
            .set_consent(&scoped, &fingerprint(manifest, &content_key));
    }

    fn dual_scope_harness(temp: &Path) -> (Harness, PathBuf, PluginRef, PluginRef) {
        install_fake_host_entry(temp);
        let repo = temp.join("repo");
        let source = write_dual_scope_plugin(&temp.join("dual-source"), "dual");
        let harness = harness_with_dev(temp, "1.0.0", vec![source.clone()]);
        harness
            .service
            .set_projects(vec![("repo-1".to_string(), repo.clone())]);
        install_local(&harness, &source, project_target("repo-1"));
        let global_ref = global_ref("orca-samples.dual");
        let project_ref = PluginRef {
            plugin_key: "orca-samples.dual".to_string(),
            scope: PluginScope::Project,
            repo_id: Some("repo-1".to_string()),
        };
        approve_raw(&harness, &global_ref);
        approve_raw(&harness, &project_ref);
        (harness, repo, global_ref, project_ref)
    }

    #[test]
    fn same_key_dual_scope_workers_run_with_independent_instances() {
        if !node_available() {
            return;
        }
        let temp = tempdir().unwrap();
        let (harness, _repo, global_ref, project_ref) = dual_scope_harness(temp.path());
        let service = &harness.service;

        service.ensure_active(&global_ref).unwrap();
        service.ensure_active(&project_ref).unwrap();
        assert_eq!(
            service.manager().active_count(),
            2,
            "both scopes hold independent workers"
        );
        assert!(matches!(
            service.manager().state("global:orca-samples.dual"),
            WorkerState::Running { .. }
        ));
        assert!(matches!(
            service.manager().state("project:repo-1:orca-samples.dual"),
            WorkerState::Running { .. }
        ));

        // Both instances answer commands through their own manager entry.
        assert_eq!(
            service
                .invoke_command(&global_ref, "echo", Some(json!({ "scope": "global" })))
                .unwrap(),
            json!({ "command": "echo", "args": { "scope": "global" } })
        );
        assert_eq!(
            service
                .invoke_command(&project_ref, "echo", Some(json!({ "scope": "project" })))
                .unwrap(),
            json!({ "command": "echo", "args": { "scope": "project" } })
        );

        // Both list entries project `running` for the same key.
        let listed = service.list();
        let dual: Vec<&PluginListEntry> = listed
            .iter()
            .filter(|entry| entry.plugin_key == "orca-samples.dual")
            .collect();
        assert_eq!(dual.len(), 1, "the project entry masks the global twin");
        assert_eq!(dual[0].status, PluginStatus::Running);
        service.shutdown();
    }

    #[test]
    fn project_removal_stops_only_the_project_worker() {
        if !node_available() {
            return;
        }
        let temp = tempdir().unwrap();
        let (harness, _repo, global_ref, project_ref) = dual_scope_harness(temp.path());
        let service = &harness.service;
        service.ensure_active(&global_ref).unwrap();
        service.ensure_active(&project_ref).unwrap();
        assert_eq!(service.manager().active_count(), 2);

        service.set_projects(vec![]);
        assert_eq!(
            service.manager().active_count(),
            1,
            "only the project instance is stopped"
        );
        assert!(matches!(
            service.manager().state("global:orca-samples.dual"),
            WorkerState::Running { .. }
        ));
        assert_eq!(
            service.manager().state("project:repo-1:orca-samples.dual"),
            WorkerState::Inactive
        );

        // The surviving global worker still serves commands and host calls.
        assert_eq!(
            service.invoke_command(&global_ref, "echo", None).unwrap(),
            json!({ "command": "echo", "args": null })
        );
        service.shutdown();
    }

    #[test]
    fn pending_project_twin_does_not_disturb_the_running_global_worker() {
        if !node_available() {
            return;
        }
        let temp = tempdir().unwrap();
        install_fake_host_entry(temp.path());
        let repo = temp.path().join("repo");
        let source = write_dual_scope_plugin(&temp.path().join("dual-source"), "twin");
        let harness = harness_with_dev(temp.path(), "1.0.0", vec![source.clone()]);
        harness
            .service
            .set_projects(vec![("repo-1".to_string(), repo.clone())]);
        install_local(&harness, &source, project_target("repo-1"));
        let global_ref = global_ref("orca-samples.twin");
        let project_ref = PluginRef {
            plugin_key: "orca-samples.twin".to_string(),
            scope: PluginScope::Project,
            repo_id: Some("repo-1".to_string()),
        };
        approve_raw(&harness, &global_ref);
        harness.service.ensure_active(&global_ref).unwrap();
        assert_eq!(harness.service.manager().active_count(), 1);

        // The project twin is installed but unapproved: its failed ensure must
        // not deactivate the running global instance.
        let error = harness.service.ensure_active(&project_ref).unwrap_err();
        assert_eq!(error.code(), gate::CONSENT_REQUIRED);
        assert_eq!(harness.service.manager().active_count(), 1);
        assert!(matches!(
            harness.service.manager().state("global:orca-samples.twin"),
            WorkerState::Running { .. }
        ));
        assert_eq!(
            harness
                .service
                .manager()
                .state("project:repo-1:orca-samples.twin"),
            WorkerState::Inactive
        );
        assert_eq!(
            harness
                .service
                .invoke_command(&global_ref, "echo", None)
                .unwrap(),
            json!({ "command": "echo", "args": null })
        );
        harness.service.shutdown();
    }

    #[test]
    fn host_calls_route_to_the_calling_workers_scope() {
        if !node_available() {
            return;
        }
        let temp = tempdir().unwrap();
        let (harness, _repo, global_ref, project_ref) = dual_scope_harness(temp.path());
        let service = &harness.service;
        service.ensure_active(&global_ref).unwrap();
        service.ensure_active(&project_ref).unwrap();

        service
            .invoke_command(&global_ref, "open-tab", None)
            .unwrap();
        service
            .invoke_command(&project_ref, "open-tab", None)
            .unwrap();

        let open_tabs = harness
            .host
            .open_tabs
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        assert_eq!(
            open_tabs.as_slice(),
            [
                (PluginScope::Global, None, "board".to_string()),
                (
                    PluginScope::Project,
                    Some("repo-1".to_string()),
                    "board".to_string()
                )
            ],
            "each hostCall must carry its own instance scope/repo context"
        );
        service.shutdown();
    }

    // ------------------------------------------------------------------
    // T11b3 panels, global view, panel actions, open-tab limits
    // ------------------------------------------------------------------

    fn panel_plugin(root: &Path, id: &str, extra: Value) -> PathBuf {
        let mut contributes = json!({
            "panels": [{ "id": "dashboard", "title": "Dashboard", "entry": "panel.html" }]
        });
        if let Value::Object(extra) = extra {
            for (key, value) in extra {
                contributes[key] = value;
            }
        }
        write_plugin(
            root,
            id,
            "1.0.0",
            ">=1.0.0",
            json!({ "contributes": contributes }),
        )
    }

    #[test]
    fn panel_entry_requires_approval_and_issues_scoped_sessions() {
        let temp = tempdir().unwrap();
        let root = write_plugin(
            &temp.path().join("dev-panels"),
            "paneled",
            "1.0.0",
            ">=1.0.0",
            json!({
                "contributes": {
                    "panels": [
                        { "id": "dashboard", "title": "Dashboard", "entry": "panel.html" },
                        { "id": "settings", "title": "Settings", "entry": "panel.html" }
                    ]
                }
            }),
        );
        let harness = harness_with_dev(temp.path(), "1.0.0", vec![root]);
        let plugin_ref = global_ref("orca-samples.paneled");

        let error = harness
            .service
            .panel_entry(&plugin_ref, "dashboard", "main")
            .unwrap_err();
        assert_eq!(error.code(), gate::CONSENT_REQUIRED);
        assert!(harness.service.sessions.is_empty());

        approve(&harness, &plugin_ref);
        let panel = harness
            .service
            .panel_entry(&plugin_ref, "dashboard", "main")
            .unwrap();
        assert!(
            panel.html.contains(crate::panel_shell::PLUGIN_PANEL_CSP),
            "shell CSP"
        );
        assert!(
            panel.html.contains("<h1>panel</h1>"),
            "entry bytes are wrapped"
        );
        assert!(panel.html.contains("__ORCA_COLOR_SCHEME__"));
        let binding = harness
            .service
            .sessions
            .resolve(&panel.session_token)
            .expect("issued token resolves");
        assert_eq!(binding.kind, SessionKind::Panel);
        assert_eq!(binding.panel_id.as_deref(), Some("dashboard"));
        assert_eq!(binding.scope, PluginScope::Global);
        assert_eq!(binding.repo_id, None);
        assert_eq!(
            binding.owner,
            SessionOwner::Renderer {
                window_label: "main".to_string()
            }
        );
        assert!(binding.manifest_revision.starts_with("manifest-sha256-"));

        let missing = harness
            .service
            .panel_entry(&plugin_ref, "nope", "main")
            .unwrap_err();
        assert_eq!(missing.code(), PANEL_NOT_FOUND);

        let view = harness
            .service
            .global_view_entry("orca-samples.paneled", "rail")
            .unwrap();
        let view_binding = harness
            .service
            .sessions
            .resolve(&view.session_token)
            .unwrap();
        assert_eq!(view_binding.kind, SessionKind::GlobalView);
        assert_eq!(view_binding.panel_id.as_deref(), Some("dashboard"));
    }

    #[test]
    fn global_view_entry_requires_a_declared_panel() {
        let temp = tempdir().unwrap();
        let root = write_plugin(
            &temp.path().join("dev-bare"),
            "bare",
            "1.0.0",
            ">=1.0.0",
            json!({}),
        );
        let harness = harness_with_dev(temp.path(), "1.0.0", vec![root]);
        let plugin_ref = global_ref("orca-samples.bare");
        approve(&harness, &plugin_ref);

        let error = harness
            .service
            .global_view_entry("orca-samples.bare", "rail")
            .unwrap_err();
        assert_eq!(error.code(), PANEL_NOT_FOUND);

        let absent = harness
            .service
            .global_view_entry("orca-samples.absent", "rail")
            .unwrap_err();
        assert_eq!(absent.code(), UNAVAILABLE);
    }

    #[test]
    fn panel_entry_rejects_tampered_trees_and_unsafe_entries() {
        let temp = tempdir().unwrap();
        let source = panel_plugin(&temp.path().join("source"), "tampered", json!({}));
        let harness = harness(temp.path(), "1.0.0");
        install_local(&harness, &source, global_target());
        let plugin_ref = global_ref("orca-samples.tampered");
        approve(&harness, &plugin_ref);
        let install_hash = harness
            .service
            .list()
            .into_iter()
            .find(|entry| entry.plugin_key == "orca-samples.tampered")
            .and_then(|entry| entry.install_hash)
            .expect("installed content hash");
        let version_dir = harness
            .app_data
            .join("plugins")
            .join("orca-samples.tampered")
            .join(install_hash);
        std::fs::write(version_dir.join("panel.html"), b"<h1>tampered</h1>").unwrap();
        let error = harness
            .service
            .panel_entry(&plugin_ref, "dashboard", "main")
            .unwrap_err();
        assert_eq!(error.code(), crate::types::INTEGRITY_MISMATCH);

        // A dev entry that escapes the plugin root is rejected at discovery.
        let outside = temp.path().join("outside");
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(outside.join("panel.html"), b"<h1>outside</h1>").unwrap();
        let escape = panel_plugin(
            &temp.path().join("dev-escape"),
            "escape",
            json!({
                "panels": [
                    { "id": "dashboard", "title": "D", "entry": "escape/panel.html" }
                ]
            }),
        );
        std::os::unix::fs::symlink(&outside, escape.join("escape")).unwrap();
        let escaped = harness_with_dev(temp.path(), "1.0.0", vec![escape]);
        let error = escaped
            .service
            .panel_entry(&global_ref("orca-samples.escape"), "dashboard", "main")
            .unwrap_err();
        assert_eq!(error.code(), UNAVAILABLE);

        // An oversize dev entry is rejected at discovery as well.
        let big = panel_plugin(&temp.path().join("dev-big"), "big", json!({}));
        std::fs::OpenOptions::new()
            .write(true)
            .open(big.join("panel.html"))
            .unwrap()
            .set_len(PLUGIN_PANEL_ENTRY_MAX_BYTES + 1)
            .unwrap();
        let oversize = harness_with_dev(temp.path(), "1.0.0", vec![big]);
        let error = oversize
            .service
            .panel_entry(&global_ref("orca-samples.big"), "dashboard", "main")
            .unwrap_err();
        assert_eq!(error.code(), UNAVAILABLE);
    }

    #[test]
    fn tab_entry_requires_approval_and_issues_main_tab_sessions() {
        let temp = tempdir().unwrap();
        let root = write_plugin(
            &temp.path().join("dev-tabbed"),
            "tabbed",
            "1.0.0",
            ">=1.0.0",
            json!({
                "contributes": {
                    "tabs": [
                        { "id": "board", "title": "Board", "entry": "panel.html" }
                    ]
                }
            }),
        );
        let harness = harness_with_dev(temp.path(), "1.0.0", vec![root]);
        let plugin_ref = global_ref("orca-samples.tabbed");

        let error = harness
            .service
            .tab_entry(
                "orca-samples.tabbed",
                PluginScope::Global,
                None,
                "board",
                "inst-1",
                Some("wt-1"),
                "main",
            )
            .unwrap_err();
        assert_eq!(error.code(), gate::CONSENT_REQUIRED);
        assert!(harness.service.sessions.is_empty());

        approve(&harness, &plugin_ref);
        let tab = harness
            .service
            .tab_entry(
                "orca-samples.tabbed",
                PluginScope::Global,
                None,
                "board",
                "inst-1",
                Some("wt-1"),
                "main",
            )
            .unwrap();
        assert!(
            tab.html.contains(crate::panel_shell::PLUGIN_PANEL_CSP),
            "shell CSP"
        );
        assert!(
            tab.html.contains("<h1>panel</h1>"),
            "entry bytes are wrapped"
        );
        let binding = harness
            .service
            .sessions
            .resolve(&tab.session_token)
            .expect("issued token resolves");
        assert_eq!(binding.kind, SessionKind::MainTab);
        assert_eq!(binding.panel_id, None);
        assert_eq!(binding.tab_id.as_deref(), Some("board"));
        assert_eq!(binding.instance_id.as_deref(), Some("inst-1"));
        assert_eq!(binding.worktree_id.as_deref(), Some("wt-1"));
        assert_eq!(binding.scope, PluginScope::Global);
        assert_eq!(binding.repo_id, None);
        assert_eq!(
            binding.owner,
            SessionOwner::Renderer {
                window_label: "main".to_string()
            }
        );
        assert!(binding.manifest_revision.starts_with("manifest-sha256-"));

        let missing = harness
            .service
            .tab_entry(
                "orca-samples.tabbed",
                PluginScope::Global,
                None,
                "nope",
                "inst-1",
                None,
                "main",
            )
            .unwrap_err();
        assert_eq!(missing.code(), TAB_NOT_FOUND);
    }

    #[test]
    fn tab_entry_rejects_escaping_and_oversize_entries() {
        let temp = tempdir().unwrap();
        let outside = temp.path().join("outside-tab");
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(outside.join("board.html"), b"<h1>outside</h1>").unwrap();
        let escape = write_plugin(
            &temp.path().join("dev-tab-escape"),
            "tabescape",
            "1.0.0",
            ">=1.0.0",
            json!({
                "contributes": {
                    "tabs": [
                        { "id": "board", "title": "Board", "entry": "escape/board.html" }
                    ]
                }
            }),
        );
        std::os::unix::fs::symlink(&outside, escape.join("escape")).unwrap();
        let escaped = harness_with_dev(temp.path(), "1.0.0", vec![escape]);
        let error = escaped
            .service
            .tab_entry(
                "orca-samples.tabescape",
                PluginScope::Global,
                None,
                "board",
                "inst-1",
                None,
                "main",
            )
            .unwrap_err();
        assert_eq!(error.code(), UNAVAILABLE);

        let big = write_plugin(
            &temp.path().join("dev-tab-big"),
            "tabbig",
            "1.0.0",
            ">=1.0.0",
            json!({
                "contributes": {
                    "tabs": [
                        { "id": "board", "title": "Board", "entry": "board.html" }
                    ]
                }
            }),
        );
        std::fs::write(big.join("board.html"), b"<h1>board</h1>").unwrap();
        std::fs::OpenOptions::new()
            .write(true)
            .open(big.join("board.html"))
            .unwrap()
            .set_len(PLUGIN_TAB_ENTRY_MAX_BYTES + 1)
            .unwrap();
        let oversize = harness_with_dev(temp.path(), "1.0.0", vec![big]);
        let error = oversize
            .service
            .tab_entry(
                "orca-samples.tabbig",
                PluginScope::Global,
                None,
                "board",
                "inst-1",
                None,
                "main",
            )
            .unwrap_err();
        assert_eq!(error.code(), UNAVAILABLE);
    }

    #[test]
    fn panel_action_resolves_gate_admission_and_audit() {
        let temp = tempdir().unwrap();
        let root = write_plugin(
            &temp.path().join("dev-action"),
            "actor",
            "1.0.0",
            ">=1.0.0",
            json!({
                "contributes": {
                    "panels": [{ "id": "dashboard", "title": "D", "entry": "panel.html" }],
                    "tabs": [{ "id": "board", "title": "Board", "entry": "panel.html" }]
                },
                "capabilities": [
                    { "kind": "workspace:read" },
                    { "kind": "workspace:open-tab" }
                ]
            }),
        );
        let harness = harness_with_dev(temp.path(), "1.0.0", vec![root]);
        let plugin_ref = global_ref("orca-samples.actor");
        approve(&harness, &plugin_ref);
        let token = harness
            .service
            .panel_entry(&plugin_ref, "dashboard", "main")
            .unwrap()
            .session_token;

        assert_eq!(
            harness
                .service
                .panel_action("bogus", "workspace.readContext", json!({})),
            Outcome::err(UNAVAILABLE, "panel session is not available")
        );

        match harness
            .service
            .panel_action(&token, "storage.get", json!({ "key": "k" }))
        {
            Outcome::Err { code, .. } => assert_eq!(code, gate::PANEL_FORBIDDEN),
            other => panic!("storage.get via panel must be forbidden: {other:?}"),
        }

        let outcome =
            harness
                .service
                .panel_action(&token, "workspace.openTab", json!({ "tabId": "board" }));
        assert!(matches!(outcome, Outcome::Ok(_)), "{outcome:?}");
        let logs = harness.service.get_logs("orca-samples.actor");
        assert!(
            logs.iter().any(|line| line.method == "workspace.openTab"),
            "panel mutations are audited: {logs:?}"
        );

        let token = harness
            .service
            .panel_entry(&plugin_ref, "dashboard", "main")
            .unwrap()
            .session_token;
        for _ in 0..PANEL_ACTION_MAX_PER_WINDOW {
            assert!(matches!(
                harness
                    .service
                    .panel_action(&token, "workspace.readContext", json!({})),
                Outcome::Ok(_)
            ));
        }
        match harness
            .service
            .panel_action(&token, "workspace.readContext", json!({}))
        {
            Outcome::Err { code, .. } => assert_eq!(code, gate::RATE_LIMITED),
            other => panic!("the 31st action must be rate limited: {other:?}"),
        }
    }

    fn issue_ui_session(service: &PluginService, plugin_key: &str, kind: SessionKind) -> String {
        service.sessions.issue(SessionBinding {
            plugin_key: plugin_key.to_string(),
            kind,
            panel_id: Some("updater".to_string()),
            tab_id: None,
            instance_id: None,
            scope: PluginScope::Global,
            repo_id: None,
            worktree_id: None,
            manifest_revision: "rev".to_string(),
            owner: SessionOwner::Renderer {
                window_label: "main".to_string(),
            },
        })
    }

    #[test]
    fn session_post_validates_binding_and_emits_pushes() {
        let temp = tempdir().unwrap();
        let harness = harness(temp.path(), "1.0.0");
        let token = issue_ui_session(&harness.service, "itsuka.updater", SessionKind::MainTab);

        harness
            .service
            .session_post("itsuka.updater", &token, Some("state"), json!({ "n": 1 }))
            .expect("push accepted");
        assert_eq!(harness.sink.pushes.lock().unwrap().len(), 1);

        assert_eq!(
            harness
                .service
                .session_post("other.plugin", &token, None, json!({}))
                .unwrap_err()
                .code(),
            UNAVAILABLE
        );
        assert_eq!(
            harness
                .service
                .session_post("itsuka.updater", "unknown-token", None, json!({}))
                .unwrap_err()
                .code(),
            UNAVAILABLE
        );
    }

    #[test]
    fn session_post_budget_is_per_token() {
        let temp = tempdir().unwrap();
        let harness = harness(temp.path(), "1.0.0");
        let token = issue_ui_session(&harness.service, "itsuka.updater", SessionKind::Panel);
        for _ in 0..PANEL_PUSH_MAX_PER_WINDOW {
            harness
                .service
                .session_post("itsuka.updater", &token, None, json!({}))
                .expect("within budget");
        }
        let error = harness
            .service
            .session_post("itsuka.updater", &token, None, json!({}))
            .unwrap_err();
        assert_eq!(error.code(), gate::RATE_LIMITED);

        let other = issue_ui_session(&harness.service, "itsuka.updater", SessionKind::Panel);
        harness
            .service
            .session_post("itsuka.updater", &other, None, json!({}))
            .expect("per-token buckets are independent");
    }

    #[test]
    fn open_tab_dedupes_caps_rates_and_prunes() {
        let temp = tempdir().unwrap();
        let root = write_plugin(
            &temp.path().join("dev-tabs"),
            "tabber",
            "1.0.0",
            ">=1.0.0",
            json!({
                "contributes": {
                    "tabs": [{ "id": "board", "title": "Board", "entry": "panel.html" }]
                },
                "capabilities": [{ "kind": "workspace:open-tab" }]
            }),
        );
        let harness = harness_with_dev(temp.path(), "1.0.0", vec![root]);
        let service = &harness.service;
        let plugin_ref = global_ref("orca-samples.tabber");

        let undeclared = service
            .open_tab(&plugin_ref, "nope", None, None, "wt-1")
            .unwrap_err();
        assert_eq!(undeclared.code(), INVALID_PARAMS);

        let first = service
            .open_tab(
                &plugin_ref,
                "board",
                Some("Board"),
                Some(json!({ "filter": "mine" })),
                "wt-1",
            )
            .unwrap();
        let again = service
            .open_tab(
                &plugin_ref,
                "board",
                None,
                Some(json!({ "filter": "mine" })),
                "wt-1",
            )
            .unwrap();
        assert_eq!(
            again.instance_id, first.instance_id,
            "deep-equal params dedupe"
        );
        let second = service
            .open_tab(
                &plugin_ref,
                "board",
                None,
                Some(json!({ "filter": "all" })),
                "wt-1",
            )
            .unwrap();
        assert_ne!(second.instance_id, first.instance_id);

        service
            .open_tab(
                &plugin_ref,
                "board",
                None,
                Some(json!({ "page": 3 })),
                "wt-1",
            )
            .unwrap();
        service
            .open_tab(
                &plugin_ref,
                "board",
                None,
                Some(json!({ "page": 4 })),
                "wt-1",
            )
            .unwrap();
        let limited = service
            .open_tab(
                &plugin_ref,
                "board",
                None,
                Some(json!({ "page": 5 })),
                "wt-1",
            )
            .unwrap_err();
        assert_eq!(limited.code(), gate::RATE_LIMITED);

        let emitted = harness
            .sink
            .tabs
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone();
        assert_eq!(emitted.len(), 4, "only new instances emit openTab requests");
        assert!(emitted.iter().any(|request| {
            request.worktree_id == "wt-1"
                && request.params == json!({ "filter": "mine" })
                && request.instance_id == first.instance_id
        }));

        // Cap: seed a saturated tracker (the rate window makes reaching 8
        // live instances organically impossible in a fast test).
        let instance_key = instance_key_of(&plugin_ref).unwrap();
        {
            let mut trackers = service
                .tab_trackers
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let tracker = trackers
                .entry((instance_key.clone(), "wt-2".to_string()))
                .or_default();
            for page in 0..OPEN_TAB_MAX_INSTANCES {
                tracker.tabs.push(TrackedTab {
                    tab_id: "board".to_string(),
                    params: json!({ "page": page }),
                    instance_id: format!("seeded-{page}"),
                });
            }
        }
        let capped = service
            .open_tab(
                &plugin_ref,
                "board",
                None,
                Some(json!({ "page": 99 })),
                "wt-2",
            )
            .unwrap_err();
        assert_eq!(capped.code(), TAB_LIMIT);

        // Worktree removal prunes tracked identities (cap and rate reset).
        service.on_worktree_removed(crate::events::WorktreeRemovedPayload {
            worktree_id: "wt-2".to_string(),
            path: "/tmp/wt2".to_string(),
        });
        service
            .open_tab(
                &plugin_ref,
                "board",
                None,
                Some(json!({ "page": 0 })),
                "wt-2",
            )
            .unwrap();

        // Deactivation prunes that instance's dedupe state.
        service.set_enabled(plugin_ref.clone(), false).unwrap();
        let after = service
            .open_tab(
                &plugin_ref,
                "board",
                None,
                Some(json!({ "filter": "mine" })),
                "wt-1",
            )
            .unwrap();
        assert_ne!(
            after.instance_id, first.instance_id,
            "deactivation drops tracked tabs"
        );
    }

    #[test]
    fn project_deactivation_keeps_the_global_twins_sessions() {
        let temp = tempdir().unwrap();
        let source = panel_plugin(&temp.path().join("source"), "twin", json!({}));
        let harness = harness(temp.path(), "1.0.0");
        harness
            .service
            .set_projects(vec![("repo-1".to_string(), temp.path().join("repo"))]);
        install_local(&harness, &source, global_target());
        install_local(&harness, &source, project_target("repo-1"));
        let global = global_ref("orca-samples.twin");
        let project = PluginRef {
            plugin_key: "orca-samples.twin".to_string(),
            scope: PluginScope::Project,
            repo_id: Some("repo-1".to_string()),
        };
        approve_raw(&harness, &global);
        approve_raw(&harness, &project);

        let global_entry = harness
            .service
            .panel_entry(&global, "dashboard", "main")
            .unwrap();
        let project_entry = harness
            .service
            .panel_entry(&project, "dashboard", "main")
            .unwrap();

        harness.service.set_enabled(project.clone(), false).unwrap();
        assert!(
            harness
                .service
                .sessions
                .resolve(&global_entry.session_token)
                .is_some(),
            "the global twin keeps its sessions"
        );
        assert!(harness
            .service
            .sessions
            .resolve(&project_entry.session_token)
            .is_none());

        harness.service.remove(global).unwrap();
        assert!(harness
            .service
            .sessions
            .resolve(&global_entry.session_token)
            .is_none());
    }

    #[test]
    fn dynamic_event_subscriptions_deliver_only_while_the_worker_runs() {
        if !node_available() {
            return;
        }
        let temp = tempdir().unwrap();
        install_fake_host_entry(temp.path());
        let root = write_worker_plugin(
            &temp.path().join("dev-dynamic"),
            "dynamic",
            &["subscribe"],
            &[],
            &["events:subscribe"],
        );
        let harness = harness_with_dev(temp.path(), "1.0.0", vec![root.clone()]);
        let plugin_ref = global_ref("orca-samples.dynamic");
        approve(&harness, &plugin_ref);

        let value = harness
            .service
            .invoke_command(&plugin_ref, "subscribe", None)
            .unwrap();
        assert_eq!(
            value,
            json!({ "subscribed": { "subscribed": ["worktree.created"] } })
        );

        let payload = json!({ "worktreeId": "w1", "path": "/tmp/wt", "branch": "main" });
        assert_eq!(
            harness
                .service
                .deliver_plugin_event("worktree.created", payload.clone()),
            1,
            "a per-instance dynamic subscription reaches the running worker"
        );
        assert!(
            wait_until(Duration::from_secs(2), || root.join("events.log").exists()),
            "the worker records the delivered event"
        );

        harness.service.shutdown();
        assert_eq!(
            harness
                .service
                .deliver_plugin_event("worktree.created", payload),
            0,
            "stopped workers keep no live dynamic subscription"
        );
    }
}
