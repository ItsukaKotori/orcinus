//! Plugin system core: manifest parsing/validation, the capability model,
//! deterministic content hashing, dual-scope paths, declared-artifact
//! validation, discovery, the install pipeline (staging, atomic publication,
//! lockfile, rollback, uninstall), consent fingerprinting/enablement, lazy
//! content-integrity verification, per-plugin storage/settings, the JSONL
//! audit log, and the Keychain-backed secret vault. Pure Rust; no Tauri or
//! `/` store dependencies.

pub mod artifact;
pub mod audit;
pub mod consent;
pub mod content_hash;
pub mod discovery;
pub mod enablement;
pub mod install;
pub mod integrity;
pub mod manifest;
pub mod paths;
pub mod plugin_settings;
pub mod secrets;
pub mod storage;
pub mod types;

pub use artifact::{resolve_declared_artifact, validate_declared_artifacts, ArtifactSet};
pub use audit::{AuditLog, AuditOutcome, AuditRecord, LogLine, AUDIT_LOG_MAX_BYTES};
pub use consent::{
    activation_state, fingerprint, has_instructional_contributions, needs_reconsent,
    parse_scoped_key, scoped_key, ActivationState, ConsentKeyScope, ConsentState,
};
pub use content_hash::hash_plugin_tree;
pub use discovery::{discover, DiscoveredPlugin, DiscoveryProjection, PluginSourceKind};
pub use enablement::{approve, decline, set_enabled};
pub use install::lockfile::{LockEntry, LockSource, Lockfile};
pub use install::publication::publish_install;
pub use install::staging::{read_install_manifest, stage_plugin_tree};
pub use install::{
    ensure_project_gitignore, install_from_local, rollback, uninstall, InstallContext,
    InstallResult, UninstallResult,
};
pub use integrity::{verify_installed, ContentHasher, FsContentHasher, PluginContentVerifier};
pub use manifest::{engine_satisfies, parse_manifest, Manifest};
pub use paths::{global_roots, project_roots, validate_containment, ScopeRoots};
pub use plugin_settings::PluginSettingsStore;
pub use secrets::{
    Keychain, KeychainError, KeychainSecrets, SecretStore, KEYCHAIN_SERVICE, SECRET_VALUE_MAX_BYTES,
};
pub use storage::{
    StorageStore, STORAGE_KEY_LIMIT, STORAGE_TOTAL_MAX_BYTES, STORAGE_VALUE_MAX_BYTES,
};
pub use types::{Capability, PluginError, PluginScope};
