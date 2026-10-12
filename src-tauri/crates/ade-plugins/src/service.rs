//! Service orchestration core: discovery/status projection, mutation-locked
//! install/remove/consent/enablement, lockfile state and batch restore, audit
//! tailing, and the event-sink seam (spec §3.3/§3.9).
//!
//! This module deliberately stops at the T11b1 boundary: worker activation,
//! panels/tabs, and watcher wiring arrive in T11b2/T11b3. Everything here is
//! synchronous and Node-free; the only processes a mutation can spawn are Git
//! children for `git` install sources.
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

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::audit::{AuditLog, AuditOutcome, AuditRecord, LogLine};
use crate::consent::{fingerprint, scoped_key, ConsentKeyScope, ConsentState};
use crate::content_hash::hash_plugin_tree;
use crate::discovery::{discover, DiscoveredPlugin, DiscoveryProjection, PluginSourceKind};
use crate::enablement;
use crate::git::{GitRunner, DEFAULT_PLUGIN_GIT_TIMEOUT};
use crate::host::services::PluginHostServices;
use crate::install::lockfile::{read_lockfile, LockEntry, LockSource};
use crate::install::publication::publish_install;
use crate::install::staging::read_install_manifest;
use crate::install::{ensure_project_gitignore, install_from_local, uninstall};
use crate::install::{InstallContext, InstallResult};
use crate::manifest::engine_satisfies;
use crate::marketplace::installer::MarketplaceInstaller;
use crate::marketplace::service::{is_official_plugin_identity, MarketplaceService};
use crate::marketplace::TempWorkspace;
use crate::paths::{global_roots, project_roots, ScopeRoots};
use crate::plugin_settings::PluginSettingsStore;
use crate::secrets::SecretStore;
use crate::storage::StorageStore;
use crate::types::{Capability, PluginError, PluginScope, INVALID_PARAMS, UNAVAILABLE};

/// How many audit lines `get_logs` returns at most.
pub const PLUGIN_LOG_TAIL_LIMIT: usize = 200;

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

/// Outbound seam to the host application (mirrors the automations service
/// sink). Implemented by ade-bridge over Tauri events.
pub trait ServiceEventSink: Send + Sync {
    fn plugins_changed(&self, content_packs_changed: bool, plugins_changed: bool);
    fn open_tab_requested(&self, request: OpenTabRequest);
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

/// Orchestrates discovery, projection, installs, consent, lockfile restore,
/// and audit reads for both scopes.
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
        })
    }

    /// Replaces the registered project set (activation is T11b2).
    pub fn set_projects(&self, projects: Vec<(String, PathBuf)>) {
        let mut current = self
            .projects
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        *current = projects;
    }

    /// Discovery plus status projection for every registered context
    /// (spec §3.3 masking: a project entry hides the same-key global entry in
    /// its own repo context; with several repos a global stays listed while at
    /// least one context can still see it).
    pub fn list(&self) -> Vec<PluginListEntry> {
        let discovered = self.discover_all();
        self.project_entries(&discovered)
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
        let plugin_key = plugin_ref.plugin_key.clone();
        self.locks.with_install_root(&roots.install_dir, || {
            self.locks
                .with_plugin_data(&roots.data_dir, &plugin_key, || {
                    uninstall(&plugin_key, &roots)
                })
        })?;
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
            ConsentDecision::Decline => enablement::decline(&*self.config.consent_state, &scoped),
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

    /// Audit tail for one plugin (global scope; project audit lookup is
    /// extended when panel/tab mutations arrive).
    pub fn get_logs(&self, plugin_key: &str) -> Vec<LogLine> {
        self.audit.read_tail(plugin_key, PLUGIN_LOG_TAIL_LIMIT)
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
                self.config.secrets.set(&plugin_ref.plugin_key, key, value)
            })
    }

    /// Serialized per-plugin-data secret delete.
    pub fn secrets_delete(&self, plugin_ref: &PluginRef, key: &str) -> Result<(), PluginError> {
        let data_dir = self.data_dir_for_ref(plugin_ref)?;
        self.locks
            .with_plugin_data(&data_dir, &plugin_ref.plugin_key, || {
                self.config.secrets.delete(&plugin_ref.plugin_key, key)
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
            resolved_commit,
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
}

fn consent_scope_of(scope: PluginScope, repo_id: Option<&str>) -> ConsentKeyScope<'_> {
    match scope {
        PluginScope::Global => ConsentKeyScope::Global,
        PluginScope::Project => ConsentKeyScope::Project {
            repo_id: repo_id.unwrap_or_default(),
        },
    }
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
    use std::time::Duration;
    use tempfile::tempdir;

    #[derive(Default)]
    struct FakeSecrets {
        values: Mutex<HashMap<String, String>>,
    }

    impl SecretStore for FakeSecrets {
        fn get(&self, plugin_key: &str, key: &str) -> Result<Option<String>, PluginError> {
            let values = self
                .values
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            Ok(values.get(&format!("{plugin_key}:{key}")).cloned())
        }

        fn set(&self, plugin_key: &str, key: &str, value: &str) -> Result<(), PluginError> {
            self.values
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .insert(format!("{plugin_key}:{key}"), value.to_string());
            Ok(())
        }

        fn delete(&self, plugin_key: &str, key: &str) -> Result<(), PluginError> {
            self.values
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .remove(&format!("{plugin_key}:{key}"));
            Ok(())
        }
    }

    struct FakeHostServices {
        storage: StorageStore,
        settings: PluginSettingsStore,
        secrets: FakeSecrets,
    }

    impl PluginHostServices for FakeHostServices {
        fn active_worktree_context(&self) -> Option<WorktreeContext> {
            None
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
            _plugin_key: &str,
            _title: &str,
            _body: Option<&str>,
        ) -> Result<bool, HostServiceError> {
            Ok(false)
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

        fn subscribe_events(&self, _plugin_key: &str, events: &[String]) -> Vec<String> {
            events.to_vec()
        }

        fn open_tab(
            &self,
            _plugin_key: &str,
            _scope: PluginScope,
            _repo_id: Option<&str>,
            _tab_id: &str,
            _title: Option<&str>,
            _params: Option<Value>,
            _worktree_hint: Option<&str>,
        ) -> Result<crate::host::services::OpenTabResult, HostServiceError> {
            Err(HostServiceError::new("unavailable", "fake host"))
        }

        fn declared_tabs(
            &self,
            _plugin_key: &str,
            _scope: PluginScope,
            _repo_id: Option<&str>,
        ) -> Vec<String> {
            Vec::new()
        }
    }

    #[derive(Default)]
    struct RecordingSink {
        changes: Mutex<Vec<(bool, bool)>>,
        tabs: Mutex<Vec<OpenTabRequest>>,
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
        app_data: PathBuf,
    }

    fn harness(temp: &Path, host_version: &str) -> Harness {
        harness_with_dev(temp, host_version, Vec::new())
    }

    fn harness_with_dev(temp: &Path, host_version: &str, dev_paths: Vec<PathBuf>) -> Harness {
        let app_data = temp.join("app-data");
        let consent = Arc::new(FakeConsentState::default());
        let sink = Arc::new(RecordingSink::default());
        let data_dir = app_data.join("plugin-data");
        let host = Arc::new(FakeHostServices {
            storage: StorageStore::new(&data_dir),
            settings: PluginSettingsStore::new(&data_dir),
            secrets: FakeSecrets::default(),
        });
        let service = PluginService::new(ServiceConfig {
            app_data_dir: app_data.clone(),
            dev_paths,
            projects: Vec::new(),
            host_entry_path: temp.join("host-entry.mjs"),
            node_override: None,
            consent_state: consent.clone(),
            secrets: Arc::new(FakeSecrets::default()),
            host_services: host,
            event_sink: sink.clone(),
            host_version: host_version.to_string(),
        });
        Harness {
            service,
            consent,
            sink,
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
}
