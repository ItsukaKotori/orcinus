//! The aiVault command surface (Phase 2F): thin wrappers over `ade_ai_vault`
//! plus the bridge-side `AiVaultService` that bundles the roots configuration
//! with the 60s list cache and the scan cancel registry.
//!
//! Every command takes one camelCase `args` struct with the `{ args }`
//! envelope; unknown (fenced/legacy) fields are ignored by serde, never
//! rejected. Blocking scans and trash operations run through [`run_blocking`].

use std::sync::Arc;

use ade_ai_vault::{
    execute_delete, prepare_session_resume, resolve_titles, scan_sessions, CancelRegistry,
    ListCache, ScanRequest, TitleRequest, VaultConfig,
};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tauri::State;

use crate::commands::run_blocking;
use crate::errors::BridgeError;
use crate::json::Json;
use crate::state::AppState;

/// Roots configuration + list cache + in-flight scan cancellations.
pub struct AiVaultService {
    config: VaultConfig,
    cache: ListCache,
    cancels: CancelRegistry,
}

impl AiVaultService {
    pub fn new(config: VaultConfig) -> Self {
        Self {
            config,
            cache: ListCache::new(),
            cancels: CancelRegistry::new(),
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct AiVaultListArgs {
    #[serde(default)]
    pub limit: Option<u32>,
    #[serde(default)]
    pub force: Option<bool>,
    #[serde(default)]
    pub scope_paths: Option<Vec<String>>,
    /// Accepted and ignored: the scan stays capped at the default 1000.
    #[serde(default)]
    pub unlimited: Option<bool>,
    #[serde(default)]
    pub request_token: Option<String>,
}

impl AiVaultListArgs {
    pub fn to_scan_request(&self) -> ScanRequest {
        ScanRequest {
            scope_paths: self.scope_paths.clone().unwrap_or_default(),
            limit: self.limit.unwrap_or(0) as usize,
            force: self.force.unwrap_or(false),
            request_token: self.request_token.clone(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct AiVaultScanIssue {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub execution_host_id: Option<String>,
    pub agent: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    pub path: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct AiVaultListResult {
    pub sessions: Vec<Json>,
    pub issues: Vec<AiVaultScanIssue>,
    pub scanned_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cancelled: Option<bool>,
}

#[derive(Debug, Clone, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct AiVaultSessionTitleRequest {
    pub agent: String,
    pub session_id: String,
    #[serde(default)]
    pub transcript_path: Option<String>,
}

impl From<AiVaultSessionTitleRequest> for TitleRequest {
    fn from(request: AiVaultSessionTitleRequest) -> Self {
        Self {
            agent: request.agent,
            session_id: request.session_id,
            transcript_path: request.transcript_path,
        }
    }
}

#[derive(Debug, Clone, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct AiVaultSessionTitlesArgs {
    /// Accepted and ignored: this bridge is local-only.
    #[serde(default)]
    pub execution_host_scope: Option<String>,
    pub requests: Vec<AiVaultSessionTitleRequest>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct AiVaultSessionTitle {
    pub agent: String,
    pub session_id: String,
    pub title: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct AiVaultSessionTitlesResult {
    pub titles: Vec<AiVaultSessionTitle>,
}

#[derive(Debug, Clone, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct AiVaultCancelListSessionsArgs {
    pub request_token: String,
}

#[derive(Debug, Clone, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct AiVaultPrepareSessionResumeArgs {
    pub agent: String,
    pub file_path: String,
    #[serde(default)]
    pub codex_home: Option<String>,
    #[serde(default)]
    pub session_id: Option<String>,
    #[serde(default)]
    pub execution_host_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct AiVaultPrepareSessionResumeResult {
    pub use_real_codex_home: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub substitute_codex_home: Option<String>,
}

#[derive(Debug, Clone, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct AiVaultDeleteSessionArgs {
    pub agent: String,
    pub file_path: String,
    #[serde(default)]
    pub session_id: Option<String>,
    #[serde(default)]
    pub execution_host_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(tag = "outcome", rename_all = "lowercase")]
pub enum AiVaultDeleteSessionResult {
    Deleted,
    Rejected { agent: String, reason: String },
    Failed { agent: String, error: String },
}

fn from_value<T: DeserializeOwned>(value: Value) -> Result<T, BridgeError> {
    serde_json::from_value(value)
        .map_err(|error| BridgeError::message(format!("unexpected ai vault payload: {error}")))
}

// ─── pure impls (Tauri-free, so tests can exercise them without an app) ──────

/// Register the request token (when given), then serve the cached scan or run
/// one. The registration is released whether the scan succeeded or not; a
/// superseded scan returns `{cancelled: true}` instead of a stale list.
pub fn list_sessions_impl(
    service: &AiVaultService,
    args: &AiVaultListArgs,
) -> Result<AiVaultListResult, BridgeError> {
    let request = args.to_scan_request();
    let token = request.request_token.clone();
    let cancel = token
        .as_deref()
        .map(|token| service.cancels.register(token))
        .unwrap_or_default();
    let result = service.cache.get_or_scan(&request, || {
        scan_sessions(&service.config, &request, &cancel)
    });
    if let Some(token) = token.as_deref() {
        service.cancels.finish(token);
    }
    from_value(result)
}

pub fn resolve_titles_impl(
    service: &AiVaultService,
    args: &AiVaultSessionTitlesArgs,
) -> Result<AiVaultSessionTitlesResult, BridgeError> {
    let requests: Vec<TitleRequest> = args
        .requests
        .iter()
        .cloned()
        .map(TitleRequest::from)
        .collect();
    from_value(resolve_titles(&service.config.home, &requests))
}

/// `false` when the token is unknown (already finished or never registered):
/// a late cancel is a no-op, never an error.
pub fn cancel_list_sessions_impl(service: &AiVaultService, request_token: &str) -> bool {
    service.cancels.cancel(request_token)
}

pub fn prepare_session_resume_impl(
    args: &AiVaultPrepareSessionResumeArgs,
) -> Result<AiVaultPrepareSessionResumeResult, BridgeError> {
    let payload = json!({
        "agent": &args.agent,
        "filePath": &args.file_path,
        "codexHome": &args.codex_home,
        "sessionId": &args.session_id,
        "executionHostId": &args.execution_host_id,
    });
    from_value(prepare_session_resume(&payload))
}

/// Run the two-stage delete; a `deleted` outcome invalidates the list cache so
/// the next (non-forced) list cannot serve the removed row.
pub fn delete_session_impl(
    service: &AiVaultService,
    args: &AiVaultDeleteSessionArgs,
) -> Result<AiVaultDeleteSessionResult, BridgeError> {
    let result: AiVaultDeleteSessionResult = from_value(execute_delete(
        &service.config.home,
        &args.agent,
        &args.file_path,
    ))?;
    if matches!(result, AiVaultDeleteSessionResult::Deleted) {
        service.cache.invalidate();
    }
    Ok(result)
}

// ─── commands ────────────────────────────────────────────────────────────────

/// `aiVault.listSessions`: `unlimited` is accepted and ignored (spec §5).
#[tauri::command]
#[specta::specta]
pub async fn ai_vault_list_sessions(
    state: State<'_, AppState>,
    args: Option<AiVaultListArgs>,
) -> Result<AiVaultListResult, BridgeError> {
    let service = Arc::clone(&state.ai_vault);
    let args = args.unwrap_or_default();
    run_blocking(move || list_sessions_impl(&service, &args)).await
}

#[tauri::command]
#[specta::specta]
pub async fn ai_vault_resolve_session_titles(
    state: State<'_, AppState>,
    args: AiVaultSessionTitlesArgs,
) -> Result<AiVaultSessionTitlesResult, BridgeError> {
    let service = Arc::clone(&state.ai_vault);
    run_blocking(move || resolve_titles_impl(&service, &args)).await
}

/// `aiVault.cancelListSessions`: unknown token is a silent no-op.
#[tauri::command]
#[specta::specta]
pub async fn ai_vault_cancel_list_sessions(
    state: State<'_, AppState>,
    args: AiVaultCancelListSessionsArgs,
) -> Result<(), BridgeError> {
    cancel_list_sessions_impl(&state.ai_vault, &args.request_token);
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn ai_vault_prepare_session_resume(
    args: AiVaultPrepareSessionResumeArgs,
) -> Result<AiVaultPrepareSessionResumeResult, BridgeError> {
    run_blocking(move || prepare_session_resume_impl(&args)).await
}

#[tauri::command]
#[specta::specta]
pub async fn ai_vault_delete_session(
    state: State<'_, AppState>,
    args: AiVaultDeleteSessionArgs,
) -> Result<AiVaultDeleteSessionResult, BridgeError> {
    let service = Arc::clone(&state.ai_vault);
    run_blocking(move || delete_session_impl(&service, &args)).await
}
