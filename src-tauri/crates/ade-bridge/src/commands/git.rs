//! The git command surface: thin, argument-mapped wrappers over `ade_git`.
//!
//! Every command takes its renderer payload as `{ args }` with camelCase
//! fields matching `src/shared/preload-api/api/git-inspection-api.ts` and
//! `git-operation-api.ts`; unknown TS fields are ignored by serde. Blocking
//! git subprocesses run through [`run_blocking`] so they never sit on the
//! async runtime threads.

use std::path::{Component, Path};
use std::time::Duration;

use ade_fs::FsService;
use ade_git::compare::{GitBranchCompareResult, GitCommitCompareResult};
use ade_git::diff::GitDiffResult;
use ade_git::history::GitHistoryResult;
use ade_git::runner::CancelToken;
use ade_git::status::{GitConflictOperation, GitStatusResult, GitUpstreamStatus};
use ade_git::status_read::StatusOptions;
use serde::{Deserialize, Serialize};
use tauri::State;

use crate::commands::run_blocking;
use crate::errors::BridgeError;
use crate::state::AppState;

/// `git show`'s budget mirrors `ade_git::diff`'s own 120s command timeout.
pub const GIT_DIFF_TIMEOUT: Duration = Duration::from_secs(120);

#[derive(Debug, Clone, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct GitStatusArgs {
    pub worktree_path: String,
    #[serde(default)]
    pub connection_id: Option<String>,
    #[serde(default)]
    pub admission_tier: Option<String>,
    #[serde(default)]
    pub include_ignored: Option<bool>,
    #[serde(default)]
    pub include_line_stats: Option<bool>,
    #[serde(default)]
    pub bypass_effective_upstream_negative_cache: Option<bool>,
    #[serde(default)]
    pub reuse_line_stats: Option<bool>,
    #[serde(default)]
    pub branch_line_total_merge_base: Option<String>,
    #[serde(default)]
    pub request_token: Option<String>,
}

impl GitStatusArgs {
    /// The renderer only sends `includeLineStats` when it is explicitly
    /// `false` (`runtime-git-status-client.ts:26-27`), so an absent field means
    /// line stats are wanted. `limit` has no renderer counterpart; `None`
    /// resolves to the 1000-row cap.
    pub fn to_status_options(&self) -> StatusOptions {
        StatusOptions {
            limit: None,
            include_ignored: self.include_ignored.unwrap_or(false),
            include_line_stats: self.include_line_stats.unwrap_or(true),
            branch_line_total_merge_base: self.branch_line_total_merge_base.clone(),
        }
    }
}

#[derive(Debug, Clone, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct GitCancelStatusArgs {
    pub request_token: String,
}

#[derive(Debug, Clone, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct GitDiffArgs {
    pub worktree_path: String,
    pub file_path: String,
    pub staged: bool,
    #[serde(default)]
    pub compare_against_head: Option<bool>,
    #[serde(default)]
    pub connection_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct GitFileArgs {
    pub worktree_path: String,
    pub file_path: String,
    #[serde(default)]
    pub connection_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct GitFilesArgs {
    pub worktree_path: String,
    pub file_paths: Vec<String>,
    #[serde(default)]
    pub connection_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct GitCommitArgs {
    pub worktree_path: String,
    pub message: String,
    #[serde(default)]
    pub connection_id: Option<String>,
}

/// `git.commit` resolves a rejected commit as `{success:false,error?}` rather
/// than rejecting; only a missing message still rejects.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct GitCommitOutcome {
    pub success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl From<ade_git::staging::CommitOutcome> for GitCommitOutcome {
    fn from(outcome: ade_git::staging::CommitOutcome) -> Self {
        Self {
            success: outcome.success,
            error: outcome.error,
        }
    }
}

#[derive(Debug, Clone, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct GitWorktreeArgs {
    pub worktree_path: String,
    #[serde(default)]
    pub connection_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct GitBranchCompareArgs {
    pub worktree_path: String,
    pub base_ref: String,
    #[serde(default)]
    pub connection_id: Option<String>,
    #[serde(default)]
    pub admission_tier: Option<String>,
}

#[derive(Debug, Clone, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct GitCommitCompareArgs {
    pub worktree_path: String,
    pub commit_id: String,
    #[serde(default)]
    pub connection_id: Option<String>,
}

/// `branchDiff.compare`: the caller passes both the base tip and the resolved
/// merge base, and the diff runs from the merge base (not `baseOid`), so a
/// diverged fork compares only the branch's own changes.
#[derive(Debug, Clone, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct GitCompareRefsArgs {
    pub base_ref: String,
    pub base_oid: String,
    pub head_oid: String,
    pub merge_base: String,
}

#[derive(Debug, Clone, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct GitBranchDiffArgs {
    pub worktree_path: String,
    pub compare: GitCompareRefsArgs,
    pub file_path: String,
    #[serde(default)]
    pub old_path: Option<String>,
    #[serde(default)]
    pub connection_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct GitCommitDiffArgs {
    pub worktree_path: String,
    pub commit_oid: String,
    #[serde(default)]
    pub parent_oid: Option<String>,
    pub file_path: String,
    #[serde(default)]
    pub old_path: Option<String>,
    #[serde(default)]
    pub connection_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct GitHistoryArgs {
    pub worktree_path: String,
    #[serde(default)]
    pub connection_id: Option<String>,
    #[serde(default)]
    pub limit: Option<u32>,
    #[serde(default)]
    pub base_ref: Option<String>,
}

// ─── pure impls (state-free, so tests can exercise them without a Tauri app) ──

pub fn status_impl(
    worktree_path: &str,
    options: &StatusOptions,
    cancel: Option<&CancelToken>,
) -> Result<GitStatusResult, BridgeError> {
    Ok(ade_git::status_read::status(
        worktree_path,
        options,
        cancel,
    )?)
}

pub fn diff_impl(
    worktree_path: &str,
    file_path: &str,
    staged: bool,
    compare_against_head: bool,
) -> Result<GitDiffResult, BridgeError> {
    Ok(ade_git::diff::diff(
        worktree_path,
        file_path,
        staged,
        compare_against_head,
        GIT_DIFF_TIMEOUT,
    )?)
}

pub fn stage_impl(worktree_path: &str, file_path: &str) -> Result<(), BridgeError> {
    Ok(ade_git::staging::stage(worktree_path, file_path)?)
}

pub fn bulk_stage_impl(worktree_path: &str, file_paths: &[String]) -> Result<(), BridgeError> {
    Ok(ade_git::staging::bulk_stage(worktree_path, file_paths)?)
}

pub fn unstage_impl(worktree_path: &str, file_path: &str) -> Result<(), BridgeError> {
    Ok(ade_git::staging::unstage(worktree_path, file_path)?)
}

pub fn bulk_unstage_impl(worktree_path: &str, file_paths: &[String]) -> Result<(), BridgeError> {
    Ok(ade_git::staging::bulk_unstage(worktree_path, file_paths)?)
}

pub fn discard_impl(worktree_path: &str, file_path: &str) -> Result<(), BridgeError> {
    Ok(ade_git::staging::discard(worktree_path, file_path)?)
}

pub fn bulk_discard_impl(worktree_path: &str, file_paths: &[String]) -> Result<(), BridgeError> {
    Ok(ade_git::staging::bulk_discard(worktree_path, file_paths)?)
}

pub fn commit_impl(worktree_path: &str, message: &str) -> Result<GitCommitOutcome, BridgeError> {
    Ok(ade_git::staging::commit(worktree_path, message)?.into())
}

pub fn upstream_status_impl(worktree_path: &str) -> Result<GitUpstreamStatus, BridgeError> {
    Ok(ade_git::staging::upstream_status(worktree_path)?)
}

pub fn conflict_operation_impl(worktree_path: &str) -> Result<GitConflictOperation, BridgeError> {
    Ok(ade_git::status_read::conflict_operation(worktree_path)?)
}

pub fn branch_compare_impl(
    worktree_path: &str,
    base_ref: &str,
) -> Result<GitBranchCompareResult, BridgeError> {
    Ok(ade_git::compare::branch_compare(worktree_path, base_ref)?)
}

pub fn commit_compare_impl(
    worktree_path: &str,
    commit_id: &str,
) -> Result<GitCommitCompareResult, BridgeError> {
    Ok(ade_git::compare::commit_compare(worktree_path, commit_id)?)
}

/// The left rev is the caller-provided merge base, never `baseOid`: the
/// renderer passes the fork point so a diverged base compares only the
/// branch's own changes (Task 8 report ruling).
pub fn branch_diff_impl(
    worktree_path: &str,
    merge_base: &str,
    head_oid: &str,
    file_path: &str,
    old_path: Option<&str>,
) -> Result<GitDiffResult, BridgeError> {
    Ok(ade_git::compare::branch_diff(
        worktree_path,
        merge_base,
        head_oid,
        file_path,
        old_path,
    )?)
}

pub fn commit_diff_impl(
    worktree_path: &str,
    commit_oid: &str,
    parent_oid: Option<&str>,
    file_path: &str,
    old_path: Option<&str>,
) -> Result<GitDiffResult, BridgeError> {
    Ok(ade_git::compare::commit_diff(
        worktree_path,
        commit_oid,
        parent_oid,
        file_path,
        old_path,
    )?)
}

pub fn history_impl(
    worktree_path: &str,
    limit: Option<u32>,
    base_ref: Option<&str>,
) -> Result<GitHistoryResult, BridgeError> {
    Ok(ade_git::history::history(worktree_path, limit, base_ref)?)
}

// ─── authorization guards ────────────────────────────────────────────────────

/// Reject a worktree path outside every authorized fs root, so the renderer
/// cannot run git against an arbitrary repository (the `fs.*` authorization
/// model).
pub fn require_authorized_worktree(fs: &FsService, worktree_path: &str) -> Result<(), BridgeError> {
    fs.resolve(worktree_path)?;
    Ok(())
}

/// Renderer-supplied repo-relative paths must not escape the authorized
/// worktree: absolute paths and `..` segments are rejected before git runs.
pub fn validate_relative_file_path(file_path: &str) -> Result<(), BridgeError> {
    let path = Path::new(file_path);
    if path.is_absolute()
        || path
            .components()
            .any(|part| matches!(part, Component::ParentDir))
    {
        return Err(BridgeError::message(format!(
            "Invalid file path: {file_path}"
        )));
    }
    Ok(())
}

fn validate_relative_file_paths(file_paths: &[String]) -> Result<(), BridgeError> {
    for file_path in file_paths {
        validate_relative_file_path(file_path)?;
    }
    Ok(())
}

// ─── commands ────────────────────────────────────────────────────────────────

/// `git.status`: registers the renderer's `requestToken` (or a generated one)
/// so `git_cancel_status` can kill the running subprocess; the registration is
/// released whether the run succeeds or fails.
#[tauri::command]
#[specta::specta]
pub async fn git_status(
    state: State<'_, AppState>,
    args: GitStatusArgs,
) -> Result<GitStatusResult, BridgeError> {
    require_authorized_worktree(&state.fs, &args.worktree_path)?;
    let request_token = args
        .request_token
        .clone()
        .unwrap_or_else(ade_core::ids::new_uuid);
    let worktree_path = args.worktree_path.clone();
    let options = args.to_status_options();
    let cancel = state.git_cancels.register(&request_token);
    let result = run_blocking(move || status_impl(&worktree_path, &options, Some(&cancel))).await;
    state.git_cancels.finish(&request_token);
    result
}

/// `git.cancelStatus`: sets the registered token; `false` (silently) when the
/// status already finished, matching the renderer's fire-and-forget call.
#[tauri::command]
#[specta::specta]
pub async fn git_cancel_status(
    state: State<'_, AppState>,
    args: GitCancelStatusArgs,
) -> Result<(), BridgeError> {
    state.git_cancels.cancel(&args.request_token);
    Ok(())
}

/// `git.diff`: HEAD/index vs worktree blob contents for one path.
#[tauri::command]
#[specta::specta]
pub async fn git_diff(
    state: State<'_, AppState>,
    args: GitDiffArgs,
) -> Result<GitDiffResult, BridgeError> {
    require_authorized_worktree(&state.fs, &args.worktree_path)?;
    validate_relative_file_path(&args.file_path)?;
    run_blocking(move || {
        diff_impl(
            &args.worktree_path,
            &args.file_path,
            args.staged,
            args.compare_against_head.unwrap_or(false),
        )
    })
    .await
}

#[tauri::command]
#[specta::specta]
pub async fn git_stage(state: State<'_, AppState>, args: GitFileArgs) -> Result<(), BridgeError> {
    require_authorized_worktree(&state.fs, &args.worktree_path)?;
    validate_relative_file_path(&args.file_path)?;
    run_blocking(move || stage_impl(&args.worktree_path, &args.file_path)).await
}

#[tauri::command]
#[specta::specta]
pub async fn git_bulk_stage(
    state: State<'_, AppState>,
    args: GitFilesArgs,
) -> Result<(), BridgeError> {
    require_authorized_worktree(&state.fs, &args.worktree_path)?;
    validate_relative_file_paths(&args.file_paths)?;
    run_blocking(move || bulk_stage_impl(&args.worktree_path, &args.file_paths)).await
}

#[tauri::command]
#[specta::specta]
pub async fn git_unstage(state: State<'_, AppState>, args: GitFileArgs) -> Result<(), BridgeError> {
    require_authorized_worktree(&state.fs, &args.worktree_path)?;
    validate_relative_file_path(&args.file_path)?;
    run_blocking(move || unstage_impl(&args.worktree_path, &args.file_path)).await
}

#[tauri::command]
#[specta::specta]
pub async fn git_bulk_unstage(
    state: State<'_, AppState>,
    args: GitFilesArgs,
) -> Result<(), BridgeError> {
    require_authorized_worktree(&state.fs, &args.worktree_path)?;
    validate_relative_file_paths(&args.file_paths)?;
    run_blocking(move || bulk_unstage_impl(&args.worktree_path, &args.file_paths)).await
}

#[tauri::command]
#[specta::specta]
pub async fn git_discard(state: State<'_, AppState>, args: GitFileArgs) -> Result<(), BridgeError> {
    require_authorized_worktree(&state.fs, &args.worktree_path)?;
    validate_relative_file_path(&args.file_path)?;
    run_blocking(move || discard_impl(&args.worktree_path, &args.file_path)).await
}

#[tauri::command]
#[specta::specta]
pub async fn git_bulk_discard(
    state: State<'_, AppState>,
    args: GitFilesArgs,
) -> Result<(), BridgeError> {
    require_authorized_worktree(&state.fs, &args.worktree_path)?;
    validate_relative_file_paths(&args.file_paths)?;
    run_blocking(move || bulk_discard_impl(&args.worktree_path, &args.file_paths)).await
}

#[tauri::command]
#[specta::specta]
pub async fn git_commit(
    state: State<'_, AppState>,
    args: GitCommitArgs,
) -> Result<GitCommitOutcome, BridgeError> {
    require_authorized_worktree(&state.fs, &args.worktree_path)?;
    run_blocking(move || commit_impl(&args.worktree_path, &args.message)).await
}

#[tauri::command]
#[specta::specta]
pub async fn git_upstream_status(
    state: State<'_, AppState>,
    args: GitWorktreeArgs,
) -> Result<GitUpstreamStatus, BridgeError> {
    require_authorized_worktree(&state.fs, &args.worktree_path)?;
    run_blocking(move || upstream_status_impl(&args.worktree_path)).await
}

#[tauri::command]
#[specta::specta]
pub async fn git_conflict_operation(
    state: State<'_, AppState>,
    args: GitWorktreeArgs,
) -> Result<GitConflictOperation, BridgeError> {
    require_authorized_worktree(&state.fs, &args.worktree_path)?;
    run_blocking(move || conflict_operation_impl(&args.worktree_path)).await
}

#[tauri::command]
#[specta::specta]
pub async fn git_branch_compare(
    state: State<'_, AppState>,
    args: GitBranchCompareArgs,
) -> Result<GitBranchCompareResult, BridgeError> {
    require_authorized_worktree(&state.fs, &args.worktree_path)?;
    run_blocking(move || branch_compare_impl(&args.worktree_path, &args.base_ref)).await
}

#[tauri::command]
#[specta::specta]
pub async fn git_commit_compare(
    state: State<'_, AppState>,
    args: GitCommitCompareArgs,
) -> Result<GitCommitCompareResult, BridgeError> {
    require_authorized_worktree(&state.fs, &args.worktree_path)?;
    run_blocking(move || commit_compare_impl(&args.worktree_path, &args.commit_id)).await
}

#[tauri::command]
#[specta::specta]
pub async fn git_branch_diff(
    state: State<'_, AppState>,
    args: GitBranchDiffArgs,
) -> Result<GitDiffResult, BridgeError> {
    require_authorized_worktree(&state.fs, &args.worktree_path)?;
    validate_relative_file_path(&args.file_path)?;
    if let Some(old_path) = &args.old_path {
        validate_relative_file_path(old_path)?;
    }
    run_blocking(move || {
        branch_diff_impl(
            &args.worktree_path,
            &args.compare.merge_base,
            &args.compare.head_oid,
            &args.file_path,
            args.old_path.as_deref(),
        )
    })
    .await
}

#[tauri::command]
#[specta::specta]
pub async fn git_commit_diff(
    state: State<'_, AppState>,
    args: GitCommitDiffArgs,
) -> Result<GitDiffResult, BridgeError> {
    require_authorized_worktree(&state.fs, &args.worktree_path)?;
    validate_relative_file_path(&args.file_path)?;
    if let Some(old_path) = &args.old_path {
        validate_relative_file_path(old_path)?;
    }
    run_blocking(move || {
        commit_diff_impl(
            &args.worktree_path,
            &args.commit_oid,
            args.parent_oid.as_deref(),
            &args.file_path,
            args.old_path.as_deref(),
        )
    })
    .await
}

#[tauri::command]
#[specta::specta]
pub async fn git_history(
    state: State<'_, AppState>,
    args: GitHistoryArgs,
) -> Result<GitHistoryResult, BridgeError> {
    require_authorized_worktree(&state.fs, &args.worktree_path)?;
    run_blocking(move || history_impl(&args.worktree_path, args.limit, args.base_ref.as_deref()))
        .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn status_options_default_to_line_stats_on_and_limit_uncapped() {
        let args: GitStatusArgs =
            serde_json::from_value(json!({ "worktreePath": "/tmp/x" })).unwrap();
        let options = args.to_status_options();
        assert_eq!(options.limit, None);
        assert!(!options.include_ignored);
        assert!(options.include_line_stats);
        assert_eq!(options.branch_line_total_merge_base, None);
    }

    #[test]
    fn status_options_honor_explicit_false_and_passthroughs() {
        let args: GitStatusArgs = serde_json::from_value(json!({
            "worktreePath": "/tmp/x",
            "includeIgnored": true,
            "includeLineStats": false,
            "branchLineTotalMergeBase": "0123456789abcdef0123456789abcdef01234567"
        }))
        .unwrap();
        let options = args.to_status_options();
        assert!(options.include_ignored);
        assert!(!options.include_line_stats);
        assert_eq!(
            options.branch_line_total_merge_base.as_deref(),
            Some("0123456789abcdef0123456789abcdef01234567")
        );
    }

    #[test]
    fn status_args_ignore_unknown_remote_only_fields() {
        let args: GitStatusArgs = serde_json::from_value(json!({
            "worktreePath": "/tmp/x",
            "bypassEffectiveUpstreamNegativeCache": true,
            "reuseLineStats": true,
            "admissionTier": "status",
            "connectionId": "conn-1"
        }))
        .unwrap();
        assert_eq!(args.connection_id.as_deref(), Some("conn-1"));
        assert_eq!(args.admission_tier.as_deref(), Some("status"));
    }

    #[test]
    fn cancel_args_deserialize_the_request_token() {
        let args: GitCancelStatusArgs =
            serde_json::from_value(json!({ "requestToken": "t1" })).unwrap();
        assert_eq!(args.request_token, "t1");
    }

    #[test]
    fn branch_diff_args_keep_merge_base_separate_from_base_oid() {
        let args: GitBranchDiffArgs = serde_json::from_value(json!({
            "worktreePath": "/tmp/x",
            "compare": {
                "baseRef": "main",
                "baseOid": "1111111111111111111111111111111111111111",
                "headOid": "2222222222222222222222222222222222222222",
                "mergeBase": "3333333333333333333333333333333333333333"
            },
            "filePath": "src/app.ts"
        }))
        .unwrap();
        assert_eq!(
            args.compare.merge_base,
            "3333333333333333333333333333333333333333"
        );
        assert_eq!(
            args.compare.base_oid,
            "1111111111111111111111111111111111111111"
        );
        assert_eq!(args.old_path, None);
    }

    #[test]
    fn commit_diff_args_accept_absent_and_null_parents() {
        let absent: GitCommitDiffArgs = serde_json::from_value(json!({
            "worktreePath": "/tmp/x",
            "commitOid": "abc",
            "filePath": "a.txt"
        }))
        .unwrap();
        assert_eq!(absent.parent_oid, None);

        let null: GitCommitDiffArgs = serde_json::from_value(json!({
            "worktreePath": "/tmp/x",
            "commitOid": "abc",
            "parentOid": null,
            "filePath": "a.txt"
        }))
        .unwrap();
        assert_eq!(null.parent_oid, None);
    }

    #[test]
    fn relative_file_path_guard_rejects_absolute_and_parent_traversal() {
        assert!(validate_relative_file_path("src/app.ts").is_ok());
        assert!(validate_relative_file_path("../outside.txt").is_err());
        assert!(validate_relative_file_path("a/b/../c").is_err());
        assert!(validate_relative_file_path("/etc/passwd").is_err());
    }

    #[test]
    fn commit_outcome_converts_from_the_domain_outcome() {
        let outcome: GitCommitOutcome = ade_git::staging::CommitOutcome {
            success: false,
            error: Some("boom".to_string()),
        }
        .into();
        assert_eq!(
            serde_json::to_value(&outcome).unwrap(),
            json!({ "success": false, "error": "boom" })
        );
    }
}
