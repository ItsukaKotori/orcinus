//! Plugin service assembly for the Tauri bridge (spec §3.3/§3.9, T12a).
//!
//! Owns the *app-side* seams of `ade-plugins::service::PluginService`:
//!
//! - [`SettingsConsentState`] records consent/enablement in the main-owned
//!   settings keys (`pluginConsents` / `disabledPlugins`) under the scoped
//!   keys `global:<key>` / `project:<repoId>:<key>` (spec §3.3). The renderer
//!   command surface strips these keys, so only this path may write them.
//! - [`BridgePluginHostServices`] implements the host-service surface over
//!   live stores: the ui-state "last active worktree", discovered manifests
//!   (declared tabs / static event subs), pty sessions, and an injected
//!   notification dispatcher. Pty and notification work are traits/closures so
//!   unit tests bind fakes and never spawn node, git, or pty sessions.
//! - [`BridgeServiceEventSink`] projects service callbacks onto the
//!   `plugins:changed` / `plugins:open-tab` Tauri events.
//! - [`build_plugin_service`] assembles everything from injected handles.
//!   A missing `host-entry.mjs` or node executable never fails assembly —
//!   worker activation reports those errors later (spec §3.11).
//! - [`PluginTicker`] is the bridge-owned 10s `pump()` loop (idle reap inside);
//!   stopping it closes the channel so shutdown never waits a full interval.
//!
//! The service's `storage`/`settings` stores are file-backed path wrappers
//! (each operation reads/writes the JSON document). [`ScopedPluginStores`]
//! resolves the root per call — global → `<app_data_dir>/plugin-data`,
//! project → `<repo>/.ade/plugin-data` — so dual-scope installs of one key and
//! two repos never share bytes (spec §3.3). Secrets are keyed by the same
//! scope-qualified identity inside one global Keychain service and share one
//! `Arc<dyn SecretStore>` with the service.

use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread::{self, JoinHandle};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use ade_fs::FsService;
use ade_plugins::consent::ConsentState;
use ade_plugins::discovery::{discover, DiscoveredPlugin};
use ade_plugins::events::is_plugin_event_name;
use ade_plugins::host::services::{
    HostServiceError, OpenTabResult, PluginHostServices, PluginSettingsAccess,
    PluginStorageAccess, TerminalRef, WorktreeContext,
};
use ade_plugins::paths::{global_roots, project_roots, ScopeRoots};
use ade_plugins::plugin_settings::PluginSettingsStore;
use ade_plugins::secrets::{KeychainSecrets, SecretStore, KEYCHAIN_SERVICE};
use ade_plugins::service::{
    OpenTabRequest, PanelPushRequest, PluginRef, PluginService, ServiceConfig, ServiceEventSink,
    WorkerRuntimeConfig,
};
use ade_plugins::storage::StorageStore;
use ade_plugins::types::{PluginError, PluginScope, INVALID_PARAMS, UNAVAILABLE};
use ade_pty::PtyHost;
use ade_store::projects_store::ProjectsStore;
use ade_store::settings_store::SettingsStore;
use ade_store::ui_state_store::UiStateStore;
use ade_store::worktree_meta_store::WorktreeMetaStore;
use serde_json::{Map, Value};

use crate::events;
use crate::state::{lock, PtyWorktreeIds};

/// Bridge-owned `pump()` cadence (spec §3.1: idle reap tick).
pub const PLUGIN_PUMP_INTERVAL: Duration = Duration::from_secs(10);

/// Injected event emitter: `(event_name, payload)` into the Tauri event bus.
pub type EventEmitter = Arc<dyn Fn(&str, Value) + Send + Sync>;
/// Injected notification dispatcher: `(plugin_key, title, body) -> delivered`.
pub type NotificationDispatcher = Arc<dyn Fn(&str, &str, Option<&str>) -> bool + Send + Sync>;

/// Resolves the packaged host entry, falling back to the source tree so a dev
/// build (`cargo tauri dev`) finds `resources/plugins/host-entry.mjs` without
/// a bundle. A missing file is returned as-is; worker activation reports it.
pub fn resolve_host_entry_path(resource_dir: Option<&Path>) -> PathBuf {
    let packaged = resource_dir.map(|dir| dir.join("plugins").join("host-entry.mjs"));
    let dev =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../resources/plugins/host-entry.mjs");
    [packaged.clone(), Some(dev)]
        .into_iter()
        .flatten()
        .find(|path| path.is_file())
        .or(packaged)
        .unwrap_or_else(|| PathBuf::from("host-entry.mjs"))
}

/// Project registry snapshots as `(repo_id, path)` pairs for discovery and
/// `set_projects`; folder workspaces count as project roots too.
pub fn project_pairs(projects: &ProjectsStore) -> Vec<(String, PathBuf)> {
    fn push(pairs: &mut Vec<(String, PathBuf)>, id: Option<&str>, path: Option<&str>) {
        if let (Some(id), Some(path)) = (id, path) {
            if !id.is_empty() && !path.is_empty() {
                pairs.push((id.to_string(), PathBuf::from(path)));
            }
        }
    }
    let mut pairs = Vec::new();
    for repo in projects.repos() {
        push(
            &mut pairs,
            repo.get("id").and_then(Value::as_str),
            repo.get("path").and_then(Value::as_str),
        );
    }
    for workspace in projects.folder_workspaces() {
        push(
            &mut pairs,
            workspace.get("id").and_then(Value::as_str),
            workspace.get("folderPath").and_then(Value::as_str),
        );
    }
    pairs
}

/// Main-owned consent/enablement records over the fork settings store.
pub struct SettingsConsentState {
    settings: Arc<Mutex<SettingsStore>>,
}

impl SettingsConsentState {
    pub fn new(settings: Arc<Mutex<SettingsStore>>) -> Self {
        Self { settings }
    }

    fn consents(&self, store: &SettingsStore) -> Map<String, Value> {
        store
            .get()
            .get("pluginConsents")
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default()
    }

    fn disabled(store: &SettingsStore) -> Vec<String> {
        store
            .get()
            .get("disabledPlugins")
            .and_then(Value::as_array)
            .map(|list| {
                list.iter()
                    .filter_map(Value::as_str)
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default()
    }
}

impl ConsentState for SettingsConsentState {
    fn consent_for(&self, scoped_key: &str) -> Option<String> {
        lock(&self.settings)
            .get()
            .get("pluginConsents")
            .and_then(|consents| consents.get(scoped_key))
            .and_then(Value::as_str)
            .map(str::to_string)
    }

    fn is_disabled(&self, scoped_key: &str) -> bool {
        Self::disabled(&lock(&self.settings))
            .iter()
            .any(|entry| entry == scoped_key)
    }

    fn set_consent(&self, scoped_key: &str, fingerprint: &str) {
        let mut store = lock(&self.settings);
        let mut consents = self.consents(&store);
        consents.insert(
            scoped_key.to_string(),
            Value::String(fingerprint.to_string()),
        );
        if let Err(error) = store.set_main_owned(serde_json::json!({ "pluginConsents": consents }))
        {
            eprintln!("[ade-bridge] failed to persist plugin consent for {scoped_key}: {error}");
        }
    }

    fn set_disabled(&self, scoped_key: &str, disabled: bool) {
        let mut store = lock(&self.settings);
        let mut list = Self::disabled(&store);
        list.retain(|entry| entry != scoped_key);
        if disabled {
            list.push(scoped_key.to_string());
        }
        if let Err(error) = store.set_main_owned(serde_json::json!({ "disabledPlugins": list })) {
            eprintln!(
                "[ade-bridge] failed to persist plugin disablement for {scoped_key}: {error}"
            );
        }
    }
}

/// Terminal facts the plugin host needs from the pty layer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PtySessionInfo {
    /// Pty session id; this is the opaque, stable id plugins see.
    pub id: String,
    pub worktree_id: Option<String>,
}

/// Injectable pty surface (tests bind a fake; production wraps [`PtyHost`]).
pub trait PtyTerminals: Send + Sync {
    fn sessions(&self) -> Vec<PtySessionInfo>;
    fn write(&self, terminal_id: &str, bytes: &[u8]) -> Result<(), String>;
}

/// Production adapter: `PtyHost` sessions tagged with the bridge's
/// `ptyId -> worktreeId` map.
pub struct HostPtyTerminals {
    host: Arc<PtyHost>,
    worktree_ids: PtyWorktreeIds,
}

impl HostPtyTerminals {
    pub fn new(host: Arc<PtyHost>, worktree_ids: PtyWorktreeIds) -> Self {
        Self { host, worktree_ids }
    }
}

impl PtyTerminals for HostPtyTerminals {
    fn sessions(&self) -> Vec<PtySessionInfo> {
        self.host
            .list_sessions()
            .into_iter()
            .map(|session| PtySessionInfo {
                worktree_id: self.worktree_ids.get(&session.id),
                id: session.id,
            })
            .collect()
    }

    fn write(&self, terminal_id: &str, bytes: &[u8]) -> Result<(), String> {
        self.host
            .write(terminal_id, bytes.to_vec())
            .map_err(|error| error.to_string())
    }
}

/// Resolves `(branch, display_name)` for one worktree id.
pub trait WorktreeContexts: Send + Sync {
    fn resolve(&self, worktree_id: &str) -> Option<(String, String)>;
    /// The worktree's filesystem path, when known. Used by read-only git host
    /// methods to run `git` in the right directory. Default: unavailable.
    fn path_for(&self, _worktree_id: &str) -> Option<String> {
        None
    }
}

/// Production resolver: worktree metadata plus the worktrees projection
/// (`git worktree list` for git repos, folder rows for folder workspaces).
pub struct WorkspaceWorktrees {
    projects: Arc<Mutex<ProjectsStore>>,
    meta: Arc<WorktreeMetaStore>,
    fs: Arc<FsService>,
}

impl WorkspaceWorktrees {
    pub fn new(
        projects: Arc<Mutex<ProjectsStore>>,
        meta: Arc<WorktreeMetaStore>,
        fs: Arc<FsService>,
    ) -> Self {
        Self { projects, meta, fs }
    }
}

impl WorktreeContexts for WorkspaceWorktrees {
    fn path_for(&self, worktree_id: &str) -> Option<String> {
        let (repos, folders, items) = {
            let projects = lock(&self.projects);
            (projects.repos(), projects.folder_workspaces(), self.meta.items())
        };
        crate::commands::worktrees::list_all_worktrees(&repos, &folders, &items, &self.fs)
            .ok()?
            .into_iter()
            .find(|worktree| worktree.id == worktree_id)
            .map(|worktree| worktree.path)
    }

    fn resolve(&self, worktree_id: &str) -> Option<(String, String)> {
        let meta = self.meta.get(worktree_id);
        let (repos, folders, items) = {
            let projects = lock(&self.projects);
            (
                projects.repos(),
                projects.folder_workspaces(),
                self.meta.items(),
            )
        };
        if let Ok(worktrees) =
            crate::commands::worktrees::list_all_worktrees(&repos, &folders, &items, &self.fs)
        {
            if let Some(found) = worktrees
                .into_iter()
                .find(|worktree| worktree.id == worktree_id)
            {
                return Some((found.branch, found.display_name));
            }
        }
        let display_name = meta
            .as_ref()
            .and_then(|meta| meta.get("displayName"))
            .and_then(Value::as_str)
            .filter(|name| !name.trim().is_empty())
            .map(str::to_string)
            .or_else(|| {
                worktree_id
                    .split_once("::")
                    .and_then(|(_, path)| Path::new(path).file_name())
                    .map(|name| name.to_string_lossy().into_owned())
            })
            .unwrap_or_else(|| worktree_id.to_string());
        Some((String::new(), display_name))
    }
}

/// Injected dependencies of [`BridgePluginHostServices`].
pub struct BridgeHostServicesDeps {
    pub app_data_dir: PathBuf,
    pub ui: Arc<Mutex<UiStateStore>>,
    pub projects: Arc<Mutex<ProjectsStore>>,
    pub worktree_meta: Arc<WorktreeMetaStore>,
    pub fs: Arc<FsService>,
    pub dev_paths: Vec<PathBuf>,
    pub pty: Arc<dyn PtyTerminals>,
    pub secrets: Arc<dyn SecretStore>,
    pub notify: NotificationDispatcher,
    /// Test seam; production leaves `None` (the workspace resolver over the
    /// injected projects/meta/fs handles).
    pub worktrees: Option<Arc<dyn WorktreeContexts>>,
}

/// Per-scope `storage.*`/`settings.*` store resolution (spec §3.3).
///
/// The stores are file-backed path wrappers, so each call opens the store for
/// the caller's scope: global → `<app_data_dir>/plugin-data`, project →
/// `<repo>/.ade/plugin-data` with the repo path resolved from the live
/// projects registry. A project call without a registered repo fails
/// `unavailable` and never touches global bytes; a project call without a
/// repo id fails `invalid_params`.
pub struct ScopedPluginStores {
    app_data_dir: PathBuf,
    projects: Arc<Mutex<ProjectsStore>>,
}

impl ScopedPluginStores {
    pub fn new(app_data_dir: PathBuf, projects: Arc<Mutex<ProjectsStore>>) -> Self {
        Self {
            app_data_dir,
            projects,
        }
    }

    fn data_dir(&self, scope: PluginScope, repo_id: Option<&str>) -> Result<PathBuf, PluginError> {
        match scope {
            PluginScope::Global => Ok(global_roots(&self.app_data_dir).data_dir),
            PluginScope::Project => {
                let repo_id = repo_id.filter(|id| !id.trim().is_empty()).ok_or_else(|| {
                    PluginError::new(INVALID_PARAMS, "project scope requires a repo id")
                })?;
                let repo_path = project_pairs(&lock(&self.projects))
                    .into_iter()
                    .find(|(id, _)| id == repo_id)
                    .map(|(_, path)| path);
                let repo_path = repo_path.ok_or_else(|| {
                    PluginError::new(UNAVAILABLE, format!("unknown project: {repo_id}"))
                })?;
                Ok(project_roots(&repo_path).data_dir)
            }
        }
    }
}

impl PluginStorageAccess for ScopedPluginStores {
    fn get(
        &self,
        plugin_key: &str,
        scope: PluginScope,
        repo_id: Option<&str>,
        key: &str,
    ) -> Result<Option<Value>, PluginError> {
        let store = StorageStore::new(self.data_dir(scope, repo_id)?);
        store.get(plugin_key, key)
    }

    fn set(
        &self,
        plugin_key: &str,
        scope: PluginScope,
        repo_id: Option<&str>,
        key: &str,
        value: Value,
    ) -> Result<(), PluginError> {
        let store = StorageStore::new(self.data_dir(scope, repo_id)?);
        store.set(plugin_key, key, value)
    }

    fn delete(
        &self,
        plugin_key: &str,
        scope: PluginScope,
        repo_id: Option<&str>,
        key: &str,
    ) -> Result<(), PluginError> {
        let store = StorageStore::new(self.data_dir(scope, repo_id)?);
        store.delete(plugin_key, key)
    }

    fn keys(
        &self,
        plugin_key: &str,
        scope: PluginScope,
        repo_id: Option<&str>,
    ) -> Result<Vec<String>, PluginError> {
        let store = StorageStore::new(self.data_dir(scope, repo_id)?);
        store.keys(plugin_key)
    }
}

impl PluginSettingsAccess for ScopedPluginStores {
    fn get_all(
        &self,
        plugin_key: &str,
        scope: PluginScope,
        repo_id: Option<&str>,
    ) -> Result<Map<String, Value>, PluginError> {
        let store = PluginSettingsStore::new(self.data_dir(scope, repo_id)?);
        store.get_all(plugin_key)
    }

    fn set(
        &self,
        plugin_key: &str,
        scope: PluginScope,
        repo_id: Option<&str>,
        key: &str,
        value: Value,
    ) -> Result<(), PluginError> {
        let store = PluginSettingsStore::new(self.data_dir(scope, repo_id)?);
        store.set(plugin_key, key, value)
    }
}

/// The bridge-side [`PluginHostServices`] implementation.
pub struct BridgePluginHostServices {
    stores: ScopedPluginStores,
    secrets: Arc<dyn SecretStore>,
    ui: Arc<Mutex<UiStateStore>>,
    worktrees: Arc<dyn WorktreeContexts>,
    pty: Arc<dyn PtyTerminals>,
    notify: NotificationDispatcher,
    global_roots: ScopeRoots,
    projects: Arc<Mutex<ProjectsStore>>,
    dev_paths: Vec<PathBuf>,
    /// Bound after the service is constructed (the service owns this adapter,
    /// so a direct `Arc` would be a cycle). `open_tab` only needs it at call
    /// time. Stores resolve their directory per call, so they need no
    /// back-reference.
    service: OnceLock<Arc<PluginService>>,
}

impl BridgePluginHostServices {
    pub fn new(deps: BridgeHostServicesDeps) -> Self {
        let global_roots = global_roots(&deps.app_data_dir);
        let worktrees = deps.worktrees.unwrap_or_else(|| {
            Arc::new(WorkspaceWorktrees::new(
                Arc::clone(&deps.projects),
                Arc::clone(&deps.worktree_meta),
                Arc::clone(&deps.fs),
            ))
        });
        Self {
            stores: ScopedPluginStores::new(deps.app_data_dir.clone(), Arc::clone(&deps.projects)),
            secrets: deps.secrets,
            ui: deps.ui,
            worktrees,
            pty: deps.pty,
            notify: deps.notify,
            global_roots,
            projects: deps.projects,
            dev_paths: deps.dev_paths,
            service: OnceLock::new(),
        }
    }

    /// Late-binds the owning service (idempotent; first bind wins).
    pub fn bind(&self, service: Arc<PluginService>) {
        let _ = self.service.set(service);
    }

    fn project_pairs(&self) -> Vec<(String, PathBuf)> {
        project_pairs(&lock(&self.projects))
    }

    fn discovered(&self) -> Vec<DiscoveredPlugin> {
        discover(&self.global_roots, &self.project_pairs(), &self.dev_paths)
    }

    /// Manifest-declared static event subscriptions across every scope.
    fn declared_events(&self, plugin_key: &str) -> Vec<String> {
        let mut names = Vec::new();
        for entry in self.discovered() {
            if entry.key.as_deref() != Some(plugin_key) {
                continue;
            }
            if let Some(manifest) = &entry.manifest {
                for contribution in &manifest.contributes.events {
                    if !names.contains(&contribution.on) {
                        names.push(contribution.on.clone());
                    }
                }
            }
        }
        names
    }
}

impl PluginHostServices for BridgePluginHostServices {
    fn active_worktree_context(&self) -> Option<WorktreeContext> {
        let worktree_id = lock(&self.ui)
            .get()
            .get("lastActiveWorktreeId")
            .and_then(Value::as_str)
            .filter(|id| !id.is_empty())
            .map(str::to_string)?;
        let (branch, display_name) = self
            .worktrees
            .resolve(&worktree_id)
            .unwrap_or_else(|| (String::new(), worktree_id.clone()));
        Some(WorktreeContext {
            worktree_id,
            branch,
            display_name,
        })
    }

    fn list_worktree_terminals(&self, worktree_id: &str) -> Vec<TerminalRef> {
        self.pty
            .sessions()
            .into_iter()
            .filter(|session| session.worktree_id.as_deref() == Some(worktree_id))
            .map(|session| TerminalRef { id: session.id })
            .collect()
    }

    fn send_terminal_text(
        &self,
        terminal_id: &str,
        text: &str,
        enter: bool,
    ) -> Result<bool, HostServiceError> {
        if !self
            .pty
            .sessions()
            .iter()
            .any(|session| session.id == terminal_id)
        {
            return Err(HostServiceError::new(
                "unavailable",
                format!("unknown terminal: {terminal_id}"),
            ));
        }
        let mut bytes = text.as_bytes().to_vec();
        if enter {
            bytes.push(b'\n');
        }
        self.pty
            .write(terminal_id, &bytes)
            .map(|()| true)
            .map_err(|error| HostServiceError::new("unavailable", error))
    }

    fn dispatch_notification(
        &self,
        plugin_key: &str,
        title: &str,
        body: Option<&str>,
    ) -> Result<bool, HostServiceError> {
        Ok((self.notify)(plugin_key, title, body))
    }

    fn storage(&self) -> &dyn PluginStorageAccess {
        &self.stores
    }

    fn settings(&self) -> &dyn PluginSettingsAccess {
        &self.stores
    }

    fn secrets(&self) -> &dyn SecretStore {
        self.secrets.as_ref()
    }

    fn subscribe_events(&self, plugin_key: &str, events: &[String]) -> Vec<String> {
        // Default (static) subscriptions come from the manifest; a plugin with
        // no readable manifest may still register the known v0 event names.
        let declared = self.declared_events(plugin_key);
        let mut accepted = Vec::new();
        for name in events {
            if !is_plugin_event_name(name) {
                continue;
            }
            if !declared.is_empty() && !declared.iter().any(|declared| declared == name) {
                continue;
            }
            if !accepted.contains(name) {
                accepted.push(name.clone());
            }
        }
        accepted
    }

    fn open_tab(
        &self,
        plugin_key: &str,
        scope: PluginScope,
        repo_id: Option<&str>,
        tab_id: &str,
        title: Option<&str>,
        params: Option<Value>,
        worktree_hint: Option<&str>,
    ) -> Result<OpenTabResult, HostServiceError> {
        let service = self.service.get().ok_or_else(|| {
            HostServiceError::new("unavailable", "plugin service is not assembled")
        })?;
        let worktree_id = match worktree_hint.filter(|id| !id.is_empty()) {
            Some(id) => id.to_string(),
            None => self
                .active_worktree_context()
                .map(|context| context.worktree_id)
                .ok_or_else(|| {
                    HostServiceError::new(
                        "action_failed",
                        "no active worktree is available to open a tab",
                    )
                })?,
        };
        service
            .open_tab(
                &PluginRef {
                    plugin_key: plugin_key.to_string(),
                    scope,
                    repo_id: repo_id.map(str::to_string),
                },
                tab_id,
                title,
                params,
                &worktree_id,
            )
            .map(|result| OpenTabResult {
                instance_id: result.instance_id,
                worktree_id: result.worktree_id,
            })
            .map_err(|error| HostServiceError::new(error.code(), error.message()))
    }

    fn declared_tabs(
        &self,
        plugin_key: &str,
        scope: PluginScope,
        repo_id: Option<&str>,
    ) -> Vec<String> {
        self.discovered()
            .into_iter()
            .filter(|entry| entry.key.as_deref() == Some(plugin_key))
            .filter(|entry| entry.scope == scope && entry.repo_id.as_deref() == repo_id)
            .filter_map(|entry| entry.manifest)
            .flat_map(|manifest| manifest.contributes.tabs)
            .map(|tab| tab.id)
            .collect()
    }

    fn session_post(
        &self,
        plugin_key: &str,
        session_token: &str,
        topic: Option<&str>,
        payload: Value,
    ) -> Result<(), HostServiceError> {
        let service = self
            .service
            .get()
            .ok_or_else(|| HostServiceError::new("unavailable", "plugin service is not assembled"))?;
        service
            .session_post(plugin_key, session_token, topic, payload)
            .map_err(|error| HostServiceError::new(error.code(), error.message()))
    }

    fn invoke_worker_command(
        &self,
        plugin_key: &str,
        scope: PluginScope,
        repo_id: Option<&str>,
        command_id: &str,
        args: Option<Value>,
        session_token: Option<&str>,
    ) -> Result<Value, HostServiceError> {
        let service = self
            .service
            .get()
            .ok_or_else(|| HostServiceError::new("unavailable", "plugin service is not assembled"))?;
        service
            .invoke_command_with_session(
                &PluginRef {
                    plugin_key: plugin_key.to_string(),
                    scope,
                    repo_id: repo_id.map(str::to_string),
                },
                command_id,
                args,
                session_token,
            )
            .map_err(|error| HostServiceError::new(error.code(), error.message()))
    }
}

/// Emits service callbacks as Tauri events through an injected emitter.
pub struct BridgeServiceEventSink {
    emit: EventEmitter,
}

impl BridgeServiceEventSink {
    pub fn new(emit: EventEmitter) -> Self {
        Self { emit }
    }

    fn emit_payload(&self, event: &str, payload: Value) {
        (self.emit)(event, payload);
    }
}

impl ServiceEventSink for BridgeServiceEventSink {
    fn plugins_changed(&self, content_packs_changed: bool, plugins_changed: bool) {
        let payload = events::PluginsChangedPayload {
            content_packs_changed,
            plugins_changed,
        };
        self.emit_payload(
            events::PLUGINS_CHANGED,
            serde_json::to_value(payload).unwrap_or(Value::Null),
        );
    }

    fn open_tab_requested(&self, request: OpenTabRequest) {
        let payload = events::PluginOpenTabPayload::from(request);
        self.emit_payload(
            events::PLUGINS_OPEN_TAB,
            serde_json::to_value(payload).unwrap_or(Value::Null),
        );
    }

    fn panel_push_requested(&self, request: PanelPushRequest) {
        let payload = events::PluginPanelPushPayload::from(request);
        self.emit_payload(
            events::PLUGINS_PANEL_PUSH,
            serde_json::to_value(payload).unwrap_or(Value::Null),
        );
    }
}

/// Bridge-owned `pump()` loop. `stop` (or drop) closes the channel so the
/// thread wakes immediately; joining never waits out the interval.
pub struct PluginTicker {
    stop: Option<Sender<()>>,
    handle: Mutex<Option<JoinHandle<()>>>,
}

impl PluginTicker {
    pub fn spawn(service: Arc<PluginService>) -> Self {
        Self::spawn_with_interval(service, PLUGIN_PUMP_INTERVAL)
    }

    /// Injection seam for tests (short interval).
    pub fn spawn_with_interval(service: Arc<PluginService>, interval: Duration) -> Self {
        let (stop, receiver) = mpsc::channel::<()>();
        let handle = thread::Builder::new()
            .name("ade-bridge-plugin-ticker".to_string())
            .spawn(move || loop {
                match receiver.recv_timeout(interval) {
                    Ok(()) | Err(RecvTimeoutError::Disconnected) => break,
                    Err(RecvTimeoutError::Timeout) => service.pump(),
                }
            });
        let handle = match handle {
            Ok(handle) => Some(handle),
            Err(error) => {
                eprintln!("[ade-bridge] failed to spawn the plugin ticker: {error}");
                None
            }
        };
        Self {
            stop: Some(stop),
            handle: Mutex::new(handle),
        }
    }

    /// Signals the loop to exit and joins it; idempotent.
    pub fn stop(&self) {
        if let Some(stop) = self.stop.as_ref() {
            let _ = stop.send(());
        }
        if let Some(handle) = lock(&self.handle).take() {
            let _ = handle.join();
        }
    }
}

impl Drop for PluginTicker {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Injected dependencies of [`build_plugin_service`]. `Clone` so `initialize`
/// can retry over a degraded app-data directory if the first assembly fails.
#[derive(Clone)]
pub struct PluginServiceDeps {
    pub app_data_dir: PathBuf,
    pub resource_dir: Option<PathBuf>,
    pub node_override: Option<String>,
    pub host_version: String,
    pub settings: Arc<Mutex<SettingsStore>>,
    pub ui: Arc<Mutex<UiStateStore>>,
    pub projects: Arc<Mutex<ProjectsStore>>,
    pub worktree_meta: Arc<WorktreeMetaStore>,
    pub fs: Arc<FsService>,
    pub pty: Arc<dyn PtyTerminals>,
    pub secrets: Arc<dyn SecretStore>,
    pub notify: NotificationDispatcher,
    pub emit: EventEmitter,
}

/// Assembles the plugin service: consent/secrets adapters, the host-service
/// implementation, the event sink, and the initial project/global activation
/// plus dev watcher. A missing host entry or node executable is recorded by
/// the worker manager and surfaced on activation, never here.
pub fn build_plugin_service(deps: PluginServiceDeps) -> io::Result<Arc<PluginService>> {
    std::fs::create_dir_all(&deps.app_data_dir)?;
    let host_entry_path = resolve_host_entry_path(deps.resource_dir.as_deref());
    let dev_paths: Vec<PathBuf> = {
        let settings = lock(&deps.settings);
        settings
            .get()
            .get("devPluginPaths")
            .and_then(Value::as_array)
            .map(|paths| {
                paths
                    .iter()
                    .filter_map(Value::as_str)
                    .filter(|path| !path.trim().is_empty())
                    .map(PathBuf::from)
                    .collect()
            })
            .unwrap_or_default()
    };
    let projects = project_pairs(&lock(&deps.projects));

    let consent_state = Arc::new(SettingsConsentState::new(Arc::clone(&deps.settings)));
    let host_services = Arc::new(BridgePluginHostServices::new(BridgeHostServicesDeps {
        app_data_dir: deps.app_data_dir.clone(),
        ui: Arc::clone(&deps.ui),
        projects: Arc::clone(&deps.projects),
        worktree_meta: Arc::clone(&deps.worktree_meta),
        fs: Arc::clone(&deps.fs),
        dev_paths: dev_paths.clone(),
        pty: Arc::clone(&deps.pty),
        secrets: Arc::clone(&deps.secrets),
        notify: Arc::clone(&deps.notify),
        worktrees: None,
    }));
    let event_sink = Arc::new(BridgeServiceEventSink::new(Arc::clone(&deps.emit)));

    let service = PluginService::new(ServiceConfig {
        app_data_dir: deps.app_data_dir.clone(),
        dev_paths,
        // Projects are registered through `set_projects` below, which is also
        // the activation reconciliation entry point.
        projects: Vec::new(),
        host_entry_path,
        node_override: deps.node_override.clone(),
        consent_state,
        secrets: Arc::clone(&deps.secrets),
        host_services: Arc::clone(&host_services) as Arc<dyn PluginHostServices>,
        event_sink,
        host_version: deps.host_version.clone(),
        workers: WorkerRuntimeConfig::default(),
    });
    host_services.bind(Arc::clone(&service));
    service.set_projects(projects);
    if let Err(error) = service.start() {
        eprintln!("[ade-bridge] plugin dev watcher failed to start: {error}");
    }
    Ok(service)
}

/// Production `KeychainSecrets` over the system Keychain (inert until used).
pub fn keychain_secrets() -> Arc<dyn SecretStore> {
    Arc::new(KeychainSecrets::new(KEYCHAIN_SERVICE))
}

/// Process-local monotonic suffix so ids stay unique even when two
/// notifications land inside the same millisecond.
static NOTIFICATION_SEQ: AtomicU64 = AtomicU64::new(0);

/// Unique notification id (`plugin:<key>:<millis>-<seq>`).
pub fn notification_id(plugin_key: &str) -> String {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or(0);
    let sequence = NOTIFICATION_SEQ.fetch_add(1, Ordering::Relaxed);
    format!("plugin:{plugin_key}:{millis}-{sequence}")
}
