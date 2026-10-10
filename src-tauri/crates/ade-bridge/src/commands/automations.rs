//! The automation command surface: thin wrappers over `ade_automations`.
//!
//! CRUD rides the store directly and broadcasts `automations:changed`
//! (`"definition"`) after every write; dispatch/precheck/observation ride
//! `AutomationService` so run writes also announce themselves and manual runs
//! publish `automations:dispatch-requested`. Blocking work (disk writes,
//! precheck subprocesses) runs through [`run_blocking`].
//!
//! Every command takes one camelCase `args` struct; unknown (fenced/legacy)
//! fields are ignored by serde, never rejected.

use std::sync::Arc;

use ade_automations::service::DEFAULT_MISSED_GRACE_MINUTES;
use ade_automations::{
    next_occurrence_after, validate_definition, AutomationService, AutomationStore,
    MAX_RUNS_PER_AUTOMATION,
};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use tauri::State;

use crate::commands::run_blocking;
use crate::errors::BridgeError;
use crate::events;
use crate::json::Json;
use crate::state::AppState;

/// Wall-clock milliseconds for command-side timestamps (`nextRunAt`,
/// `scheduledFor`, `updatedAt`); also the service's production clock.
pub fn now_millis() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as i64)
        .unwrap_or(0)
}

/// Run fields the dispatch loop may write back (spec §3.2, oracle
/// `updateAutomationRun`): everything else in the renderer's result — its
/// `runId` included — is dropped so runs stay clean.
const DISPATCH_RESULT_FIELDS: [&str; 11] = [
    "status",
    "workspaceId",
    "workspaceDisplayName",
    "terminalSessionId",
    "terminalPaneKey",
    "terminalPtyId",
    "outputSnapshot",
    "precheckResult",
    "error",
    "startedAt",
    "dispatchedAt",
];

#[derive(Debug, Clone, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct AutomationsCreateArgs {
    pub input: Json,
}

#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct AutomationsCreateResult {
    pub automation: Json,
}

#[derive(Debug, Clone, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct AutomationsUpdateArgs {
    pub id: String,
    pub updates: Json,
}

#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct AutomationsUpdateResult {
    pub automation: Option<Json>,
}

#[derive(Debug, Clone, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct AutomationsDeleteArgs {
    pub id: String,
}

#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct AutomationsDeleteResult {
    pub removed: bool,
}

#[derive(Debug, Clone, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct AutomationsRunNowArgs {
    pub id: String,
}

#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct AutomationsRunNowResult {
    pub run: Json,
}

/// `expectedOwner`/`selector`/`destination` (fenced multi-host fields) and any
/// other legacy extra deserialize into nothing; unknown keys are ignored.
#[derive(Debug, Clone, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct AutomationsRunsArgs {
    #[serde(default)]
    pub automation_id: Option<String>,
    #[serde(default)]
    pub limit: Option<usize>,
    #[serde(default)]
    pub cursor: Option<String>,
}

#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct AutomationsRunsResult {
    pub runs: Vec<Json>,
    pub next_cursor: Option<String>,
}

#[derive(Debug, Clone, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct AutomationsRunPrecheckArgs {
    pub automation_id: String,
    pub run_id: String,
}

#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct AutomationsRunPrecheckResult {
    pub result: Option<Json>,
}

#[derive(Debug, Clone, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct AutomationsMarkDispatchResultArgs {
    pub run_id: String,
    pub result: Json,
}

#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct AutomationsMarkDispatchResultResult {
    pub run: Option<Json>,
}

#[derive(Debug, Clone, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct AutomationsSnapshotWorkspaceNameArgs {
    pub workspace_id: String,
    pub display_name: String,
}

#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct AutomationsSnapshotWorkspaceNameResult {
    pub updated: usize,
}

/// Validate a new definition, compute its next occurrence, and persist it.
pub fn create_impl(
    store: &Arc<AutomationStore>,
    input: Value,
    now_ms: i64,
) -> Result<Value, BridgeError> {
    validate_definition(&input).map_err(BridgeError::message)?;
    let Some(mut record) = input.as_object().cloned() else {
        return Err(BridgeError::message(
            "Automation input must be a JSON object.",
        ));
    };
    let rrule = record
        .get("rrule")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let dtstart = record.get("dtstart").and_then(Value::as_i64).unwrap_or(0);
    let next_run_at = next_occurrence_after(rrule, dtstart, now_ms)
        .ok_or_else(|| BridgeError::message("Automation schedule has no upcoming occurrence."))?;
    record.insert("nextRunAt".to_string(), Value::from(next_run_at));
    record
        .entry("enabled".to_string())
        .or_insert(Value::Bool(true));
    record
        .entry("missedRunGraceMinutes".to_string())
        .or_insert(Value::from(DEFAULT_MISSED_GRACE_MINUTES));
    record
        .entry("executionTargetType".to_string())
        .or_insert_with(|| Value::from("local"));
    record
        .entry("executionTargetId".to_string())
        .or_insert_with(|| Value::from("local"));
    store
        .create(Value::Object(record), now_ms)
        .map(|(automation, _)| automation)
        .map_err(|error| BridgeError::message(error.to_string()))
}

/// Merge `updates` over the stored definition, validate the result, recompute
/// `nextRunAt` when the schedule changed, and persist. `nextRunAt` itself is
/// host-owned: a renderer-supplied value is ignored.
pub fn update_impl(
    store: &Arc<AutomationStore>,
    id: &str,
    updates: Value,
    now_ms: i64,
) -> Result<Option<Value>, BridgeError> {
    let Some(updates) = updates.as_object() else {
        return Err(BridgeError::message(
            "Automation updates must be a JSON object.",
        ));
    };
    let Some(existing) = store.get(id) else {
        return Ok(None);
    };
    let mut candidate = existing.as_object().cloned().unwrap_or_default();
    let mut effective = updates.clone();
    effective.remove("nextRunAt");
    for (key, value) in &effective {
        if value.is_null() {
            candidate.remove(key);
        } else {
            candidate.insert(key.clone(), value.clone());
        }
    }
    validate_definition(&Value::Object(candidate.clone())).map_err(BridgeError::message)?;
    if effective.contains_key("rrule") || effective.contains_key("dtstart") {
        let rrule = candidate
            .get("rrule")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let dtstart = candidate
            .get("dtstart")
            .and_then(Value::as_i64)
            .unwrap_or(0);
        let next_run_at = next_occurrence_after(rrule, dtstart, now_ms).ok_or_else(|| {
            BridgeError::message("Automation schedule has no upcoming occurrence.")
        })?;
        effective.insert("nextRunAt".to_string(), Value::from(next_run_at));
    }
    store
        .update(id, Value::Object(effective), now_ms)
        .map_err(|error| BridgeError::message(error.to_string()))
}

/// Delete an automation and every run it owns; `false` when it is already gone.
pub fn delete_impl(store: &Arc<AutomationStore>, id: &str) -> Result<bool, BridgeError> {
    store
        .delete(id)
        .map_err(|error| BridgeError::message(error.to_string()))
}

/// All automation definitions, in insertion order.
pub fn list_impl(store: &Arc<AutomationStore>) -> Vec<Value> {
    store.list()
}

/// One keyset page of run history (newest first); `None` limit pages at
/// [`MAX_RUNS_PER_AUTOMATION`].
pub fn runs_impl(
    store: &Arc<AutomationStore>,
    automation_id: Option<&str>,
    limit: Option<usize>,
    cursor: Option<&str>,
) -> (Vec<Value>, Option<String>) {
    let page = store.runs_page(
        automation_id,
        limit.unwrap_or(MAX_RUNS_PER_AUTOMATION),
        cursor,
    );
    (page.runs, page.next_cursor)
}

/// Manual dispatch (`runNow`); works for disabled automations.
pub fn run_now_impl(service: &Arc<AutomationService>, id: &str) -> Result<Value, BridgeError> {
    service
        .run_now(id)
        .ok_or_else(|| BridgeError::message("Automation not found."))
}

/// Execute and persist the scheduled run's precheck; `None` when the run is
/// manual, missing, or the automation has no precheck.
pub fn run_precheck_impl(
    service: &Arc<AutomationService>,
    automation_id: &str,
    run_id: &str,
) -> Option<Value> {
    service.run_precheck_for(automation_id, run_id)
}

/// Write the renderer's dispatch result back to the run (whitelisted fields;
/// final runs are never overwritten by the store guard).
pub fn mark_dispatch_result_impl(
    service: &Arc<AutomationService>,
    run_id: &str,
    result: &Value,
) -> Result<Option<Value>, BridgeError> {
    let result = result
        .as_object()
        .ok_or_else(|| BridgeError::message("Automation dispatch result must be a JSON object."))?;
    let has_status = result
        .get("status")
        .and_then(Value::as_str)
        .is_some_and(|status| !status.is_empty());
    if !has_status {
        return Err(BridgeError::message(
            "Automation dispatch result requires a status.",
        ));
    }
    let mut fields = Map::new();
    for key in DISPATCH_RESULT_FIELDS {
        if let Some(value) = result.get(key) {
            fields.insert(key.to_string(), value.clone());
        }
    }
    Ok(service.mark_dispatch_result(run_id, Value::Object(fields)))
}

/// Snapshot a workspace's display name onto every run pointing at it.
pub fn snapshot_workspace_name_impl(
    service: &Arc<AutomationService>,
    workspace_id: &str,
    display_name: &str,
) -> usize {
    service.snapshot_workspace_name(workspace_id, display_name)
}

/// All automation definitions (fenced selector/expectedOwner ignored).
#[tauri::command]
#[specta::specta]
pub async fn automations_list(state: State<'_, AppState>) -> Result<Vec<Json>, BridgeError> {
    Ok(list_impl(&state.automations)
        .into_iter()
        .map(Json::new)
        .collect())
}

/// Create one automation; an invalid schedule/definition is rejected before
/// anything touches the store. Broadcasts `automations:changed {reason:
/// "definition"}`.
#[tauri::command]
#[specta::specta]
pub async fn automations_create(
    state: State<'_, AppState>,
    args: AutomationsCreateArgs,
) -> Result<AutomationsCreateResult, BridgeError> {
    let store = Arc::clone(&state.automations);
    let input = args.input.into_inner();
    let automation = run_blocking(move || create_impl(&store, input, now_millis())).await?;
    events::emit_automations_changed(&state.app, "definition");
    Ok(AutomationsCreateResult {
        automation: Json::new(automation),
    })
}

/// Merge `updates` into one automation (`null` removes a key); recomputes
/// `nextRunAt` when `rrule`/`dtstart` change. `{automation: null}` when the id
/// is unknown.
#[tauri::command]
#[specta::specta]
pub async fn automations_update(
    state: State<'_, AppState>,
    args: AutomationsUpdateArgs,
) -> Result<AutomationsUpdateResult, BridgeError> {
    let store = Arc::clone(&state.automations);
    let id = args.id;
    let updates = args.updates.into_inner();
    let automation = run_blocking(move || update_impl(&store, &id, updates, now_millis())).await?;
    events::emit_automations_changed(&state.app, "definition");
    Ok(AutomationsUpdateResult {
        automation: automation.map(Json::new),
    })
}

/// Delete one automation and every run it owns.
#[tauri::command]
#[specta::specta]
pub async fn automations_delete(
    state: State<'_, AppState>,
    args: AutomationsDeleteArgs,
) -> Result<AutomationsDeleteResult, BridgeError> {
    let store = Arc::clone(&state.automations);
    let id = args.id;
    let removed = run_blocking(move || delete_impl(&store, &id)).await?;
    events::emit_automations_changed(&state.app, "definition");
    Ok(AutomationsDeleteResult { removed })
}

/// Manual dispatch: create the run and publish `automations:dispatch-requested`
/// (the service broadcasts `automations:changed {reason: "run"}` for the run
/// write). Not gated by `enabled`.
#[tauri::command]
#[specta::specta]
pub async fn automations_run_now(
    state: State<'_, AppState>,
    args: AutomationsRunNowArgs,
) -> Result<AutomationsRunNowResult, BridgeError> {
    let service = Arc::clone(&state.automation_service);
    let id = args.id;
    let run = run_blocking(move || run_now_impl(&service, &id)).await?;
    Ok(AutomationsRunNowResult {
        run: Json::new(run),
    })
}

/// One keyset page of run history (`createdAt desc, id asc`); the cursor names
/// the previous page's last run.
#[tauri::command]
#[specta::specta]
pub async fn automations_runs(
    state: State<'_, AppState>,
    args: AutomationsRunsArgs,
) -> Result<AutomationsRunsResult, BridgeError> {
    let (runs, next_cursor) = runs_impl(
        &state.automations,
        args.automation_id.as_deref(),
        args.limit,
        args.cursor.as_deref(),
    );
    Ok(AutomationsRunsResult {
        runs: runs.into_iter().map(Json::new).collect(),
        next_cursor,
    })
}

/// Execute the configured precheck for a scheduled run and persist its result
/// (`{result: null}` for manual runs or a missing run/precheck).
#[tauri::command]
#[specta::specta]
pub async fn automations_run_precheck(
    state: State<'_, AppState>,
    args: AutomationsRunPrecheckArgs,
) -> Result<AutomationsRunPrecheckResult, BridgeError> {
    let service = Arc::clone(&state.automation_service);
    let result = run_blocking(move || {
        Ok(run_precheck_impl(
            &service,
            &args.automation_id,
            &args.run_id,
        ))
    })
    .await?;
    Ok(AutomationsRunPrecheckResult {
        result: result.map(Json::new),
    })
}

/// Write the renderer's dispatch result back (`{run: null}` when the run is
/// gone; final runs are returned untouched).
#[tauri::command]
#[specta::specta]
pub async fn automations_mark_dispatch_result(
    state: State<'_, AppState>,
    args: AutomationsMarkDispatchResultArgs,
) -> Result<AutomationsMarkDispatchResultResult, BridgeError> {
    let service = Arc::clone(&state.automation_service);
    let run_id = args.run_id;
    let result = args.result.into_inner();
    let run = run_blocking(move || mark_dispatch_result_impl(&service, &run_id, &result)).await?;
    Ok(AutomationsMarkDispatchResultResult {
        run: run.map(Json::new),
    })
}

/// Snapshot a workspace's display name onto every run pointing at it (called
/// when a workspace is removed/renamed).
#[tauri::command]
#[specta::specta]
pub async fn automations_snapshot_workspace_name(
    state: State<'_, AppState>,
    args: AutomationsSnapshotWorkspaceNameArgs,
) -> Result<AutomationsSnapshotWorkspaceNameResult, BridgeError> {
    let service = Arc::clone(&state.automation_service);
    let updated = run_blocking(move || {
        Ok(snapshot_workspace_name_impl(
            &service,
            &args.workspace_id,
            &args.display_name,
        ))
    })
    .await?;
    Ok(AutomationsSnapshotWorkspaceNameResult { updated })
}

/// Arm the service and run the catch-up tick (renderer boot).
#[tauri::command]
#[specta::specta]
pub async fn automations_renderer_ready(state: State<'_, AppState>) -> Result<(), BridgeError> {
    let service = Arc::clone(&state.automation_service);
    run_blocking(move || {
        service.renderer_ready();
        Ok(())
    })
    .await
}
