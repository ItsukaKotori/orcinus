//! The plugin command surface (spec §3.9): thin, DTO-shaped wrappers over
//! `ade_plugins::service::PluginService` and its marketplace installer.
//!
//! Every command has a state-free `*_impl` helper so integration tests can
//! exercise the surface without a Tauri runtime. Field names are taken from
//! the authoritative TS contract (`src/shared/preload-api/api/plugin-host-api.ts`,
//! T13); the DTOs are specta types so bindings stay generated from one source.
//!
//! ## Feature gate (`pluginSystemEnabled`)
//!
//! The setting's default is `false`. Parity with the oracle's
//! `plugin-service.ts` gate: when the switch is off, *activation* paths
//! (`plugins_invoke_command`, `plugins_read_panel_entry`, `plugins_panel_action`)
//! fail with the stable `disabled_system` error, while worker-less operations
//! (list/consent/enable/install/marketplaces/logs/lockfile) stay available so
//! the UI can still enable and manage the system. The gate is applied in the
//! command wrapper only — service-level calls (worker activation inside the
//! service) remain the service's concern.
//!
//! ## Blocking paths
//!
//! Install (local copy/git clone), marketplace add/refresh/preview/install,
//! rollback, and lockfile restore shell to `git` or stage trees on disk, so
//! they run through [`run_blocking`]. Read-only projections run inline.

use std::sync::Arc;

use ade_plugins::audit::AuditOutcome;
use ade_plugins::marketplace::installer::MarketplacePreview;
use ade_plugins::marketplace::service::{MarketplaceListing, MarketplaceProjection, SourceState};
use ade_plugins::marketplace::store::MarketplaceSource;
use ade_plugins::service::{
    ConsentDecision, ConsentRequest, InstallSource, LanguagePackRegistration, LockEntryStatus,
    LockState, PanelEntry, PluginListEntry, PluginRef, PluginStatus, ScopeTarget,
};
use ade_plugins::types::{PluginError, PluginScope};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::State;

use crate::commands::automations::now_millis;
use crate::commands::run_blocking;
use crate::errors::BridgeError;
use crate::json::Json;
use crate::state::AppState;

/// Stable error code for activation attempts while `pluginSystemEnabled` is
/// off (mirrors the oracle gate).
pub const DISABLED_SYSTEM: &str = "disabled_system";

fn disabled_system_error() -> BridgeError {
    BridgeError::message(format!("{DISABLED_SYSTEM}: plugin system is disabled"))
}

fn bridge_error(error: PluginError) -> BridgeError {
    BridgeError::message(format!("{}: {}", error.code(), error.message()))
}

fn plugin_result<T>(result: Result<T, PluginError>) -> Result<T, BridgeError> {
    result.map_err(bridge_error)
}

// ===== DTOs =====

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "lowercase")]
pub enum PluginScopeDto {
    Global,
    Project,
}

impl From<PluginScopeDto> for PluginScope {
    fn from(scope: PluginScopeDto) -> Self {
        match scope {
            PluginScopeDto::Global => PluginScope::Global,
            PluginScopeDto::Project => PluginScope::Project,
        }
    }
}

impl From<PluginScope> for PluginScopeDto {
    fn from(scope: PluginScope) -> Self {
        match scope {
            PluginScope::Global => PluginScopeDto::Global,
            PluginScope::Project => PluginScopeDto::Project,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "lowercase")]
pub enum PluginStatusDto {
    Running,
    Restarting,
    Idle,
    Pending,
    Disabled,
    Errored,
    Invalid,
}

impl From<PluginStatus> for PluginStatusDto {
    fn from(status: PluginStatus) -> Self {
        match status {
            PluginStatus::Running => PluginStatusDto::Running,
            PluginStatus::Restarting => PluginStatusDto::Restarting,
            PluginStatus::Idle => PluginStatusDto::Idle,
            PluginStatus::Pending => PluginStatusDto::Pending,
            PluginStatus::Disabled => PluginStatusDto::Disabled,
            PluginStatus::Errored => PluginStatusDto::Errored,
            PluginStatus::Invalid => PluginStatusDto::Invalid,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "lowercase")]
pub enum PluginCommandContextDto {
    Global,
    Worktree,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum PluginCommandHandlerDto {
    BuiltIn { action: String },
    Worker,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct PluginKeybindingDto {
    pub key: String,
    pub when: PluginCommandContextDto,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct PluginCommandDto {
    pub id: String,
    pub title: String,
    pub context: PluginCommandContextDto,
    pub handler: PluginCommandHandlerDto,
    pub keybindings: Vec<PluginKeybindingDto>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct PluginCapabilityDto {
    pub kind: String,
    pub description: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct PluginPanelDto {
    pub id: String,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    pub tab_key: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct PluginTabDto {
    pub id: String,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct PluginSourceDto {
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reference: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolved_commit: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content_hash: Option<String>,
}

/// `plugins:list` entry (TS `PluginHostListEntry`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct PluginListEntryDto {
    pub plugin_key: String,
    pub scope: PluginScopeDto,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repo_id: Option<String>,
    pub consent_fingerprint: Option<String>,
    pub name: String,
    pub version: String,
    pub publisher: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub status: PluginStatusDto,
    pub needs_reconsent: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    pub is_dev: bool,
    pub official: bool,
    pub bundled: bool,
    pub capabilities: Vec<PluginCapabilityDto>,
    pub panels: Vec<PluginPanelDto>,
    pub tabs: Vec<PluginTabDto>,
    pub commands: Vec<PluginCommandDto>,
    pub has_worker: bool,
    pub restarts: u32,
    pub source: PluginSourceDto,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub install_hash: Option<String>,
}

impl From<PluginListEntry> for PluginListEntryDto {
    fn from(entry: PluginListEntry) -> Self {
        Self {
            plugin_key: entry.plugin_key,
            scope: entry.scope.into(),
            repo_id: entry.repo_id,
            consent_fingerprint: entry.consent_fingerprint,
            name: entry.name,
            version: entry.version,
            publisher: entry.publisher,
            description: entry.description,
            status: entry.status.into(),
            needs_reconsent: entry.needs_reconsent,
            error: entry.error,
            is_dev: entry.is_dev,
            official: entry.official,
            bundled: entry.bundled,
            capabilities: entry
                .capabilities
                .into_iter()
                .map(|capability| PluginCapabilityDto {
                    kind: capability.kind,
                    description: capability.description,
                })
                .collect(),
            panels: entry
                .panels
                .into_iter()
                .map(|panel| PluginPanelDto {
                    id: panel.id,
                    title: panel.title,
                    icon: panel.icon,
                    tab_key: panel.tab_key,
                })
                .collect(),
            tabs: entry
                .tabs
                .into_iter()
                .map(|tab| PluginTabDto {
                    id: tab.id,
                    title: tab.title,
                    icon: tab.icon,
                })
                .collect(),
            commands: entry
                .commands
                .into_iter()
                .map(|command| PluginCommandDto {
                    id: command.id,
                    title: command.title,
                    context: match command.context {
                        ade_plugins::manifest::CommandContext::Global => {
                            PluginCommandContextDto::Global
                        }
                        ade_plugins::manifest::CommandContext::Worktree => {
                            PluginCommandContextDto::Worktree
                        }
                    },
                    handler: match command.handler {
                        ade_plugins::service::PluginCommandHandler::BuiltIn { action } => {
                            PluginCommandHandlerDto::BuiltIn { action }
                        }
                        ade_plugins::service::PluginCommandHandler::Worker => {
                            PluginCommandHandlerDto::Worker
                        }
                    },
                    keybindings: command
                        .keybindings
                        .into_iter()
                        .map(|binding| PluginKeybindingDto {
                            key: binding.key,
                            when: match binding.when {
                                ade_plugins::manifest::CommandContext::Global => {
                                    PluginCommandContextDto::Global
                                }
                                ade_plugins::manifest::CommandContext::Worktree => {
                                    PluginCommandContextDto::Worktree
                                }
                            },
                        })
                        .collect(),
                })
                .collect(),
            has_worker: entry.has_worker,
            restarts: entry.restarts,
            source: PluginSourceDto {
                kind: entry.source.kind,
                reference: entry.source.reference,
                resolved_commit: entry.source.resolved_commit,
                content_hash: entry.source.content_hash,
            },
            install_hash: entry.install_hash,
        }
    }
}

/// TS `PluginLanguagePackRegistration`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct PluginLanguagePackRegistrationDto {
    pub id: String,
    pub resource_language: String,
    pub plugin_key: String,
    pub locale: String,
    pub catalog: Json,
}

impl From<LanguagePackRegistration> for PluginLanguagePackRegistrationDto {
    fn from(pack: LanguagePackRegistration) -> Self {
        Self {
            id: pack.id,
            resource_language: pack.resource_language,
            plugin_key: pack.plugin_key,
            locale: pack.locale,
            catalog: Json::new(pack.catalog),
        }
    }
}

/// TS `PluginHostLogLine`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct PluginHostLogLineDto {
    pub ts: i64,
    pub level: String,
    pub line: String,
}

/// TS `PluginPanelEntry` (`{html, sessionToken}`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct PluginPanelEntryDto {
    pub html: String,
    pub session_token: String,
}

impl From<PanelEntry> for PluginPanelEntryDto {
    fn from(entry: PanelEntry) -> Self {
        Self {
            html: entry.html,
            session_token: entry.session_token,
        }
    }
}

/// TS `PluginPanelActionOutcome` as one flat wire object (`ok: true` carries
/// `value`; `ok: false` carries `code` + `error`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct PluginPanelActionOutcomeDto {
    pub ok: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<Json>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl From<ade_plugins::host::methods::Outcome> for PluginPanelActionOutcomeDto {
    fn from(outcome: ade_plugins::host::methods::Outcome) -> Self {
        match outcome {
            ade_plugins::host::methods::Outcome::Ok(value) => Self {
                ok: true,
                value: Some(Json::new(value)),
                code: None,
                error: None,
            },
            ade_plugins::host::methods::Outcome::Err { code, error } => Self {
                ok: false,
                value: None,
                code: Some(code),
                error: Some(error),
            },
        }
    }
}

/// TS `PluginHostInstallSource` (plus the marketplace variant the plan names,
/// kept in the Rust union so `plugins_install` can accept it).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(
    tag = "kind",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase"
)]
pub enum PluginHostInstallSourceDto {
    LocalPath {
        path: String,
        scope: PluginScopeDto,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        repo_id: Option<String>,
    },
    Git {
        url: String,
        #[serde(rename = "ref", default)]
        ref_: String,
        scope: PluginScopeDto,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        repo_id: Option<String>,
    },
    Marketplace {
        source_id: String,
        plugin_key: String,
        scope: PluginScopeDto,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        repo_id: Option<String>,
    },
}

impl PluginHostInstallSourceDto {
    fn into_parts(self) -> (InstallSource, ScopeTarget) {
        match self {
            Self::LocalPath {
                path,
                scope,
                repo_id,
            } => (
                InstallSource::LocalPath { path: path.into() },
                ScopeTarget {
                    scope: scope.into(),
                    repo_id,
                },
            ),
            Self::Git {
                url,
                ref_,
                scope,
                repo_id,
            } => (
                InstallSource::Git { url, ref_ },
                ScopeTarget {
                    scope: scope.into(),
                    repo_id,
                },
            ),
            Self::Marketplace {
                source_id,
                plugin_key,
                scope,
                repo_id,
            } => (
                InstallSource::Marketplace {
                    source_id,
                    plugin_key,
                },
                ScopeTarget {
                    scope: scope.into(),
                    repo_id,
                },
            ),
        }
    }
}

/// TS `PluginHostInstallResult` (`{ok:true,...} | {ok:false,error}`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct PluginInstallResultDto {
    pub ok: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plugin_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content_hash: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub consent_fingerprint: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolved_commit: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl PluginInstallResultDto {
    pub fn ok(
        plugin_key: String,
        version: String,
        content_hash: String,
        consent_fingerprint: String,
    ) -> Self {
        Self {
            ok: true,
            plugin_key: Some(plugin_key),
            version: Some(version),
            content_hash: Some(content_hash),
            consent_fingerprint: Some(consent_fingerprint),
            resolved_commit: None,
            error: None,
        }
    }

    pub fn failed(error: impl Into<String>) -> Self {
        Self {
            ok: false,
            plugin_key: None,
            version: None,
            content_hash: None,
            consent_fingerprint: None,
            resolved_commit: None,
            error: Some(error.into()),
        }
    }
}

impl From<ade_plugins::install::InstallResult> for PluginInstallResultDto {
    fn from(result: ade_plugins::install::InstallResult) -> Self {
        Self {
            ok: true,
            plugin_key: Some(result.plugin_key),
            version: Some(result.version),
            content_hash: Some(result.content_hash),
            consent_fingerprint: Some(result.consent_fingerprint),
            resolved_commit: None,
            error: None,
        }
    }
}

/// TS `PluginMarketplaceGitSource` (`{kind: 'git', url, ref}`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct PluginMarketplaceGitSourceDto {
    #[serde(default = "default_git_kind")]
    pub kind: String,
    pub url: String,
    #[serde(rename = "ref")]
    pub ref_: String,
}

fn default_git_kind() -> String {
    "git".to_string()
}

impl From<MarketplaceSource> for PluginMarketplaceGitSourceDto {
    fn from(source: MarketplaceSource) -> Self {
        Self {
            kind: source.kind,
            url: source.url,
            ref_: source.ref_,
        }
    }
}

impl From<PluginMarketplaceGitSourceDto> for MarketplaceSource {
    fn from(source: PluginMarketplaceGitSourceDto) -> Self {
        Self {
            kind: if source.kind.trim().is_empty() {
                default_git_kind()
            } else {
                source.kind
            },
            url: source.url,
            ref_: source.ref_,
        }
    }
}

/// TS `PluginMarketplaceHostSourceState`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct PluginMarketplaceHostSourceStateDto {
    pub id: String,
    pub source: PluginMarketplaceGitSourceDto,
    pub added_at: u64,
    pub marketplace: Option<PluginMarketplaceProjectionDto>,
    pub stale: bool,
    pub official: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct PluginMarketplaceProjectionDto {
    pub name: String,
    pub owner: String,
    pub resolved_commit: String,
    pub fetched_at: u64,
}

impl From<MarketplaceProjection> for PluginMarketplaceProjectionDto {
    fn from(projection: MarketplaceProjection) -> Self {
        Self {
            name: projection.name,
            owner: projection.owner,
            resolved_commit: projection.resolved_commit,
            fetched_at: projection.fetched_at,
        }
    }
}

impl From<SourceState> for PluginMarketplaceHostSourceStateDto {
    fn from(state: SourceState) -> Self {
        Self {
            id: state.id,
            source: state.source.into(),
            added_at: state.added_at,
            marketplace: state.marketplace.map(Into::into),
            stale: false,
            official: state.official,
            error: None,
        }
    }
}

/// TS `PluginMarketplaceHostListing`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct PluginMarketplaceHostListingDto {
    pub marketplace_source_id: String,
    pub marketplace_name: String,
    pub marketplace_owner: String,
    pub marketplace_commit: String,
    pub plugin_key: String,
    pub source: PluginMarketplaceGitSourceDto,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub categories: Vec<String>,
    pub official: bool,
    pub bundled: bool,
}

impl From<MarketplaceListing> for PluginMarketplaceHostListingDto {
    fn from(listing: MarketplaceListing) -> Self {
        Self {
            marketplace_source_id: listing.marketplace_source_id,
            marketplace_name: listing.marketplace_name,
            marketplace_owner: listing.marketplace_owner,
            marketplace_commit: listing.marketplace_commit,
            plugin_key: listing.plugin_key,
            source: listing.source.into(),
            description: listing.description,
            categories: listing.categories,
            official: listing.official,
            bundled: listing.bundled,
        }
    }
}

/// TS `PluginMarketplaceHostInstallPreview`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct PluginMarketplaceInstallPreviewDto {
    pub marketplace_source_id: String,
    pub marketplace_name: String,
    pub marketplace_owner: String,
    pub marketplace_commit: String,
    pub plugin_key: String,
    pub source: PluginMarketplaceGitSourceDto,
    pub resolved_commit: String,
    pub content_hash: String,
    pub consent_fingerprint: String,
    pub manifest: Json,
    pub official: bool,
    pub bundled: bool,
}

impl From<MarketplacePreview> for PluginMarketplaceInstallPreviewDto {
    fn from(preview: MarketplacePreview) -> Self {
        Self {
            marketplace_source_id: preview.marketplace_source_id,
            marketplace_name: preview.marketplace_name,
            marketplace_owner: preview.marketplace_owner,
            marketplace_commit: preview.marketplace_commit,
            plugin_key: preview.plugin_key,
            source: preview.source.into(),
            resolved_commit: preview.resolved_commit,
            content_hash: preview.content_hash,
            consent_fingerprint: preview.consent_fingerprint,
            manifest: Json::new(serde_json::to_value(preview.manifest).unwrap_or(Value::Null)),
            official: preview.official,
            bundled: false,
        }
    }
}

/// TS `PluginProjectLockStatus`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "kebab-case")]
pub enum PluginProjectLockStatusDto {
    Installed,
    Missing,
    HashMismatch,
    SourceUnavailable,
}

impl From<LockEntryStatus> for PluginProjectLockStatusDto {
    fn from(status: LockEntryStatus) -> Self {
        match status {
            LockEntryStatus::Installed => Self::Installed,
            LockEntryStatus::Missing => Self::Missing,
            LockEntryStatus::HashMismatch => Self::HashMismatch,
            LockEntryStatus::SourceUnavailable => Self::SourceUnavailable,
        }
    }
}

/// TS `PluginProjectLockEntry`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct PluginProjectLockEntryDto {
    pub plugin_key: String,
    pub version: String,
    pub source: Json,
    pub content_hash: String,
    pub status: PluginProjectLockStatusDto,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

impl From<ade_plugins::service::LockStateEntry> for PluginProjectLockEntryDto {
    fn from(entry: ade_plugins::service::LockStateEntry) -> Self {
        Self {
            plugin_key: entry.plugin_key,
            version: entry.version,
            source: Json::new(serde_json::to_value(entry.source).unwrap_or(Value::Null)),
            content_hash: entry.content_hash,
            status: entry.status.into(),
            detail: entry.detail,
        }
    }
}

/// TS `PluginProjectLockState`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct PluginProjectLockStateDto {
    pub repo_id: String,
    pub lockfile_path: String,
    pub entries: Vec<PluginProjectLockEntryDto>,
}

impl From<LockState> for PluginProjectLockStateDto {
    fn from(state: LockState) -> Self {
        Self {
            repo_id: state.repo_id,
            lockfile_path: state.lockfile_path,
            entries: state.entries.into_iter().map(Into::into).collect(),
        }
    }
}

// ===== args =====

/// `PluginRef` arguments (`consent`/`setEnabled`/`remove`/`readPanelEntry`/
/// `invokeCommand` share the identity triple).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct PluginsPluginRefArgs {
    pub plugin_key: String,
    pub scope: PluginScopeDto,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repo_id: Option<String>,
}

impl PluginsPluginRefArgs {
    fn to_ref(&self) -> PluginRef {
        PluginRef {
            plugin_key: self.plugin_key.clone(),
            scope: self.scope.into(),
            repo_id: self.repo_id.clone(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "kebab-case")]
pub enum PluginConsentDecisionDto {
    Approve,
    #[serde(alias = "decline")]
    KeepDisabled,
}

/// TS `PluginConsentRequest`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct PluginsConsentArgs {
    pub plugin_key: String,
    pub scope: PluginScopeDto,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repo_id: Option<String>,
    pub reviewed_fingerprint: String,
    pub decision: PluginConsentDecisionDto,
}

impl PluginsConsentArgs {
    fn to_request(&self) -> ConsentRequest {
        ConsentRequest {
            plugin_ref: PluginRef {
                plugin_key: self.plugin_key.clone(),
                scope: self.scope.into(),
                repo_id: self.repo_id.clone(),
            },
            reviewed_fingerprint: self.reviewed_fingerprint.clone(),
            decision: match self.decision {
                PluginConsentDecisionDto::Approve => ConsentDecision::Approve,
                PluginConsentDecisionDto::KeepDisabled => ConsentDecision::Decline,
            },
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct PluginsSetEnabledArgs {
    pub plugin_key: String,
    pub scope: PluginScopeDto,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repo_id: Option<String>,
    pub enabled: bool,
}

impl PluginsSetEnabledArgs {
    fn to_ref(&self) -> PluginRef {
        PluginRef {
            plugin_key: self.plugin_key.clone(),
            scope: self.scope.into(),
            repo_id: self.repo_id.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct PluginsReadPanelEntryArgs {
    pub plugin_key: String,
    pub panel_id: String,
    pub scope: PluginScopeDto,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repo_id: Option<String>,
    /// Webview label owning the session; defaulted to `main` (the renderer).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner_window: Option<String>,
}

impl PluginsReadPanelEntryArgs {
    fn to_ref(&self) -> PluginRef {
        PluginRef {
            plugin_key: self.plugin_key.clone(),
            scope: self.scope.into(),
            repo_id: self.repo_id.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct PluginsInvokeCommandArgs {
    pub plugin_key: String,
    pub command_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub args: Option<Json>,
    pub scope: PluginScopeDto,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repo_id: Option<String>,
}

impl PluginsInvokeCommandArgs {
    fn to_ref(&self) -> PluginRef {
        PluginRef {
            plugin_key: self.plugin_key.clone(),
            scope: self.scope.into(),
            repo_id: self.repo_id.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct PluginsPanelActionArgs {
    pub session_token: String,
    pub action: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub params: Option<Json>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct PluginsRemoveMarketplaceArgs {
    pub source_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct PluginsRefreshMarketplacesArgs {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct PluginsPreviewMarketplacePluginArgs {
    pub marketplace_source_id: String,
    pub plugin_key: String,
}

/// TS `installMarketplacePlugin` argument: the pinned identity the user
/// reviewed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct PluginsInstallMarketplacePluginArgs {
    pub marketplace_source_id: String,
    pub marketplace_commit: String,
    pub plugin_key: String,
    pub resolved_commit: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct PluginsMarketplacePluginKeyArgs {
    pub plugin_key: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct PluginsGetLogsArgs {
    pub plugin_key: String,
}

/// `{repoId}` for the project lockfile surface.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct PluginsProjectLockArgs {
    pub repo_id: String,
}

// ===== state-free impls =====

pub fn plugins_list_impl(service: &ade_plugins::service::PluginService) -> Vec<PluginListEntryDto> {
    service.list().into_iter().map(Into::into).collect()
}

pub fn plugins_list_language_packs_impl(
    service: &ade_plugins::service::PluginService,
) -> Vec<PluginLanguagePackRegistrationDto> {
    service
        .language_packs()
        .into_iter()
        .map(Into::into)
        .collect()
}

pub fn plugins_consent_impl(
    service: &ade_plugins::service::PluginService,
    args: PluginsConsentArgs,
) -> Result<Vec<PluginListEntryDto>, PluginError> {
    service
        .consent(args.to_request())
        .map(|entries| entries.into_iter().map(Into::into).collect())
}

pub fn plugins_set_enabled_impl(
    service: &ade_plugins::service::PluginService,
    args: PluginsSetEnabledArgs,
) -> Result<Vec<PluginListEntryDto>, PluginError> {
    service
        .set_enabled(args.to_ref(), args.enabled)
        .map(|entries| entries.into_iter().map(Into::into).collect())
}

pub fn plugins_read_panel_entry_impl(
    service: &ade_plugins::service::PluginService,
    args: PluginsReadPanelEntryArgs,
) -> Result<Option<PluginPanelEntryDto>, PluginError> {
    let owner = args.owner_window.as_deref().unwrap_or("main");
    match service.panel_entry(&args.to_ref(), &args.panel_id, owner) {
        Ok(entry) => Ok(Some(entry.into())),
        Err(error) if error.code() == "panel_not_found" => Ok(None),
        Err(error) => Err(error),
    }
}

pub fn plugins_invoke_command_impl(
    service: &ade_plugins::service::PluginService,
    args: PluginsInvokeCommandArgs,
) -> Result<Json, PluginError> {
    let value = service.invoke_command(
        &args.to_ref(),
        &args.command_id,
        args.args.map(Json::into_inner),
    )?;
    Ok(Json::new(value))
}

pub fn plugins_panel_action_impl(
    service: &ade_plugins::service::PluginService,
    args: PluginsPanelActionArgs,
) -> PluginPanelActionOutcomeDto {
    let params = args.params.map(Json::into_inner).unwrap_or(Value::Null);
    service
        .panel_action(&args.session_token, &args.action, params)
        .into()
}

pub fn plugins_install_impl(
    service: &ade_plugins::service::PluginService,
    source: PluginHostInstallSourceDto,
) -> PluginInstallResultDto {
    let (source, target) = source.into_parts();
    match service.install(source, target) {
        Ok(result) => PluginInstallResultDto::from(result),
        Err(error) => PluginInstallResultDto::failed(error.to_string()),
    }
}

pub fn plugins_list_marketplaces_impl(
    service: &ade_plugins::service::PluginService,
) -> Result<Vec<PluginMarketplaceHostSourceStateDto>, PluginError> {
    service
        .marketplace()
        .service()
        .list_sources()
        .map(|sources| sources.into_iter().map(Into::into).collect())
}

pub fn plugins_add_marketplace_impl(
    service: &ade_plugins::service::PluginService,
    source: PluginMarketplaceGitSourceDto,
) -> Result<PluginMarketplaceHostSourceStateDto, PluginError> {
    let added_at = now_millis().max(0) as u64;
    service
        .marketplace()
        .service()
        .add_source(&source.into(), added_at)
        .map(Into::into)
}

pub fn plugins_remove_marketplace_impl(
    service: &ade_plugins::service::PluginService,
    args: PluginsRemoveMarketplaceArgs,
) -> Result<Vec<PluginMarketplaceHostSourceStateDto>, PluginError> {
    service
        .marketplace()
        .service()
        .remove_source(&args.source_id)?;
    plugins_list_marketplaces_impl(service)
}

pub fn plugins_refresh_marketplaces_impl(
    service: &ade_plugins::service::PluginService,
    args: PluginsRefreshMarketplacesArgs,
) -> Result<Vec<PluginMarketplaceHostSourceStateDto>, PluginError> {
    service
        .marketplace()
        .service()
        .refresh(args.source_id.as_deref())
        .map(|sources| sources.into_iter().map(Into::into).collect())
}

pub fn plugins_list_marketplace_plugins_impl(
    service: &ade_plugins::service::PluginService,
) -> Result<Vec<PluginMarketplaceHostListingDto>, PluginError> {
    service
        .marketplace()
        .service()
        .list_plugins()
        .map(|listings| listings.into_iter().map(Into::into).collect())
}

pub fn plugins_preview_marketplace_plugin_impl(
    service: &ade_plugins::service::PluginService,
    args: PluginsPreviewMarketplacePluginArgs,
) -> Result<PluginMarketplaceInstallPreviewDto, PluginError> {
    service
        .marketplace()
        .preview(&args.marketplace_source_id, &args.plugin_key)
        .map(Into::into)
}

pub fn plugins_install_marketplace_plugin_impl(
    service: &ade_plugins::service::PluginService,
    args: PluginsInstallMarketplacePluginArgs,
) -> PluginInstallResultDto {
    // Re-resolve the listing and refuse when the pinned identity the user
    // reviewed no longer matches (TOCTOU guard mirroring the installer).
    let preview = match service
        .marketplace()
        .preview(&args.marketplace_source_id, &args.plugin_key)
    {
        Ok(preview) => preview,
        Err(error) => return PluginInstallResultDto::failed(error.to_string()),
    };
    if preview.marketplace_commit != args.marketplace_commit {
        return PluginInstallResultDto::failed(
            "content_changed: marketplace changed after preview; review the plugin again",
        );
    }
    if preview.resolved_commit != args.resolved_commit {
        return PluginInstallResultDto::failed(
            "content_changed: plugin source changed after preview; review the update again",
        );
    }
    plugins_install_impl(
        service,
        PluginHostInstallSourceDto::Marketplace {
            source_id: args.marketplace_source_id,
            plugin_key: args.plugin_key,
            scope: PluginScopeDto::Global,
            repo_id: None,
        },
    )
}

pub fn plugins_preview_marketplace_update_impl(
    service: &ade_plugins::service::PluginService,
    args: PluginsMarketplacePluginKeyArgs,
) -> Result<PluginMarketplaceInstallPreviewDto, PluginError> {
    service
        .preview_marketplace_update(&args.plugin_key)
        .map(Into::into)
}

pub fn plugins_rollback_marketplace_plugin_impl(
    service: &ade_plugins::service::PluginService,
    args: PluginsMarketplacePluginKeyArgs,
) -> PluginInstallResultDto {
    match service.rollback_marketplace_plugin(&args.plugin_key) {
        Ok(result) => PluginInstallResultDto::from(result),
        Err(error) => PluginInstallResultDto::failed(error.to_string()),
    }
}

pub fn plugins_remove_impl(
    service: &ade_plugins::service::PluginService,
    args: PluginsPluginRefArgs,
) -> Result<Vec<PluginListEntryDto>, PluginError> {
    service
        .remove(args.to_ref())
        .map(|entries| entries.into_iter().map(Into::into).collect())
}

pub fn plugins_get_logs_impl(
    service: &ade_plugins::service::PluginService,
    args: PluginsGetLogsArgs,
) -> Vec<PluginHostLogLineDto> {
    service
        .get_logs(&args.plugin_key)
        .into_iter()
        .map(|line| PluginHostLogLineDto {
            ts: line.ts,
            level: match line.outcome {
                AuditOutcome::Error => "error".to_string(),
                AuditOutcome::Attempt | AuditOutcome::Ok => "info".to_string(),
            },
            line: format!("{}: {}", line.method, line.summary),
        })
        .collect()
}

pub fn plugins_refresh_impl(
    service: &ade_plugins::service::PluginService,
) -> Vec<PluginListEntryDto> {
    service.refresh().into_iter().map(Into::into).collect()
}

pub fn plugins_get_project_lock_state_impl(
    service: &ade_plugins::service::PluginService,
    args: PluginsProjectLockArgs,
) -> PluginProjectLockStateDto {
    service.lockfile_state(&args.repo_id).into()
}

pub fn plugins_install_missing_from_lockfile_impl(
    service: &ade_plugins::service::PluginService,
    args: PluginsProjectLockArgs,
) -> Vec<PluginInstallResultDto> {
    service
        .install_missing_from_lockfile(&args.repo_id)
        .into_iter()
        .map(|result| match result {
            Ok(result) => PluginInstallResultDto::from(result),
            Err(error) => PluginInstallResultDto::failed(error.to_string()),
        })
        .collect()
}

/// Probe payload for tests/troubleshooting: the stable gate error body.
pub fn disabled_system_outcome() -> PluginPanelActionOutcomeDto {
    PluginPanelActionOutcomeDto {
        ok: false,
        value: None,
        code: Some(DISABLED_SYSTEM.to_string()),
        error: Some("plugin system is disabled".to_string()),
    }
}

/// `plugins_list`: discovery + status projection for both scopes.
#[tauri::command]
#[specta::specta]
pub fn plugins_list(state: State<'_, AppState>) -> Vec<PluginListEntryDto> {
    plugins_list_impl(&state.plugins)
}

#[tauri::command]
#[specta::specta]
pub fn plugins_list_language_packs(
    state: State<'_, AppState>,
) -> Vec<PluginLanguagePackRegistrationDto> {
    plugins_list_language_packs_impl(&state.plugins)
}

#[tauri::command]
#[specta::specta]
pub fn plugins_consent(
    state: State<'_, AppState>,
    args: PluginsConsentArgs,
) -> Result<Vec<PluginListEntryDto>, BridgeError> {
    plugin_result(plugins_consent_impl(&state.plugins, args))
}

#[tauri::command]
#[specta::specta]
pub fn plugins_set_enabled(
    state: State<'_, AppState>,
    args: PluginsSetEnabledArgs,
) -> Result<Vec<PluginListEntryDto>, BridgeError> {
    plugin_result(plugins_set_enabled_impl(&state.plugins, args))
}

#[tauri::command]
#[specta::specta]
pub fn plugins_read_panel_entry(
    state: State<'_, AppState>,
    args: PluginsReadPanelEntryArgs,
) -> Result<Option<PluginPanelEntryDto>, BridgeError> {
    if !state.plugin_system_enabled() {
        return Err(disabled_system_error());
    }
    plugin_result(plugins_read_panel_entry_impl(&state.plugins, args))
}

#[tauri::command]
#[specta::specta]
pub fn plugins_invoke_command(
    state: State<'_, AppState>,
    args: PluginsInvokeCommandArgs,
) -> Result<Json, BridgeError> {
    if !state.plugin_system_enabled() {
        return Err(disabled_system_error());
    }
    plugin_result(plugins_invoke_command_impl(&state.plugins, args))
}

#[tauri::command]
#[specta::specta]
pub fn plugins_panel_action(
    state: State<'_, AppState>,
    args: PluginsPanelActionArgs,
) -> PluginPanelActionOutcomeDto {
    if !state.plugin_system_enabled() {
        return disabled_system_outcome();
    }
    plugins_panel_action_impl(&state.plugins, args)
}

#[tauri::command]
#[specta::specta]
pub async fn plugins_install(
    state: State<'_, AppState>,
    source: PluginHostInstallSourceDto,
) -> Result<PluginInstallResultDto, BridgeError> {
    let service = Arc::clone(&state.plugins);
    run_blocking(move || Ok(plugins_install_impl(&service, source))).await
}

#[tauri::command]
#[specta::specta]
pub fn plugins_list_marketplaces(
    state: State<'_, AppState>,
) -> Result<Vec<PluginMarketplaceHostSourceStateDto>, BridgeError> {
    plugin_result(plugins_list_marketplaces_impl(&state.plugins))
}

#[tauri::command]
#[specta::specta]
pub async fn plugins_add_marketplace(
    state: State<'_, AppState>,
    source: PluginMarketplaceGitSourceDto,
) -> Result<PluginMarketplaceHostSourceStateDto, BridgeError> {
    let service = Arc::clone(&state.plugins);
    run_blocking(move || plugin_result(plugins_add_marketplace_impl(&service, source))).await
}

#[tauri::command]
#[specta::specta]
pub fn plugins_remove_marketplace(
    state: State<'_, AppState>,
    args: PluginsRemoveMarketplaceArgs,
) -> Result<Vec<PluginMarketplaceHostSourceStateDto>, BridgeError> {
    plugin_result(plugins_remove_marketplace_impl(&state.plugins, args))
}

#[tauri::command]
#[specta::specta]
pub async fn plugins_refresh_marketplaces(
    state: State<'_, AppState>,
    args: PluginsRefreshMarketplacesArgs,
) -> Result<Vec<PluginMarketplaceHostSourceStateDto>, BridgeError> {
    let service = Arc::clone(&state.plugins);
    run_blocking(move || plugin_result(plugins_refresh_marketplaces_impl(&service, args))).await
}

#[tauri::command]
#[specta::specta]
pub fn plugins_list_marketplace_plugins(
    state: State<'_, AppState>,
) -> Result<Vec<PluginMarketplaceHostListingDto>, BridgeError> {
    plugin_result(plugins_list_marketplace_plugins_impl(&state.plugins))
}

#[tauri::command]
#[specta::specta]
pub async fn plugins_preview_marketplace_plugin(
    state: State<'_, AppState>,
    args: PluginsPreviewMarketplacePluginArgs,
) -> Result<PluginMarketplaceInstallPreviewDto, BridgeError> {
    let service = Arc::clone(&state.plugins);
    run_blocking(move || plugin_result(plugins_preview_marketplace_plugin_impl(&service, args)))
        .await
}

#[tauri::command]
#[specta::specta]
pub async fn plugins_install_marketplace_plugin(
    state: State<'_, AppState>,
    args: PluginsInstallMarketplacePluginArgs,
) -> Result<PluginInstallResultDto, BridgeError> {
    let service = Arc::clone(&state.plugins);
    run_blocking(move || Ok(plugins_install_marketplace_plugin_impl(&service, args))).await
}

#[tauri::command]
#[specta::specta]
pub async fn plugins_preview_marketplace_update(
    state: State<'_, AppState>,
    args: PluginsMarketplacePluginKeyArgs,
) -> Result<PluginMarketplaceInstallPreviewDto, BridgeError> {
    let service = Arc::clone(&state.plugins);
    run_blocking(move || plugin_result(plugins_preview_marketplace_update_impl(&service, args)))
        .await
}

#[tauri::command]
#[specta::specta]
pub async fn plugins_rollback_marketplace_plugin(
    state: State<'_, AppState>,
    args: PluginsMarketplacePluginKeyArgs,
) -> Result<PluginInstallResultDto, BridgeError> {
    let service = Arc::clone(&state.plugins);
    run_blocking(move || Ok(plugins_rollback_marketplace_plugin_impl(&service, args))).await
}

#[tauri::command]
#[specta::specta]
pub fn plugins_remove(
    state: State<'_, AppState>,
    args: PluginsPluginRefArgs,
) -> Result<Vec<PluginListEntryDto>, BridgeError> {
    plugin_result(plugins_remove_impl(&state.plugins, args))
}

#[tauri::command]
#[specta::specta]
pub fn plugins_get_logs(
    state: State<'_, AppState>,
    args: PluginsGetLogsArgs,
) -> Vec<PluginHostLogLineDto> {
    plugins_get_logs_impl(&state.plugins, args)
}

#[tauri::command]
#[specta::specta]
pub fn plugins_refresh(state: State<'_, AppState>) -> Vec<PluginListEntryDto> {
    plugins_refresh_impl(&state.plugins)
}

#[tauri::command]
#[specta::specta]
pub fn plugins_get_project_lock_state(
    state: State<'_, AppState>,
    args: PluginsProjectLockArgs,
) -> PluginProjectLockStateDto {
    plugins_get_project_lock_state_impl(&state.plugins, args)
}

#[tauri::command]
#[specta::specta]
pub async fn plugins_install_missing_from_lockfile(
    state: State<'_, AppState>,
    args: PluginsProjectLockArgs,
) -> Result<Vec<PluginInstallResultDto>, BridgeError> {
    let service = Arc::clone(&state.plugins);
    run_blocking(move || Ok(plugins_install_missing_from_lockfile_impl(&service, args))).await
}
