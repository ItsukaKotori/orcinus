use std::sync::Arc;

use ade_fs::{
    DirEntry, FileContent, FileStat, MarkdownDocument, PathExistence, SearchOptions, SearchResult,
};
use serde::Deserialize;
use serde_json::Value;
use tauri::{State, Window};

use crate::commands::run_blocking;
use crate::errors::BridgeError;
use crate::state::AppState;

#[derive(Debug, Clone, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct FsReadDirArgs {
    pub dir_path: String,
}

#[derive(Debug, Clone, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct FsReadFileArgs {
    pub file_path: String,
}

#[derive(Debug, Clone, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct FsWriteFileArgs {
    pub file_path: String,
    pub content: String,
}

#[derive(Debug, Clone, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct FsCreateFileArgs {
    pub file_path: String,
}

#[derive(Debug, Clone, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct FsCreateDirArgs {
    pub dir_path: String,
}

#[derive(Debug, Clone, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct FsRenameArgs {
    pub old_path: String,
    pub new_path: String,
}

#[derive(Debug, Clone, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct FsCopyArgs {
    pub source_path: String,
    pub destination_path: String,
}

#[derive(Debug, Clone, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct FsDeletePathArgs {
    pub target_path: String,
    /// Accepted for contract parity; `deletePath` always moves to the system
    /// trash, so the flag is not needed to succeed.
    #[serde(default)]
    pub recursive: Option<bool>,
}

#[derive(Debug, Clone, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct FsStatArgs {
    pub file_path: String,
}

#[derive(Debug, Clone, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct FsPathExistsArgs {
    pub file_path: String,
}

#[derive(Debug, Clone, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct FsPathsExistArgs {
    pub file_paths: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct FsListFilesArgs {
    pub root_path: String,
    #[serde(default)]
    pub exclude_paths: Vec<String>,
    #[serde(default)]
    pub request_token: Option<String>,
    #[serde(default)]
    pub max_results: Option<usize>,
}

#[derive(Debug, Clone, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct FsCancelListFilesArgs {
    pub request_token: String,
}

#[derive(Debug, Clone, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct FsWatchWorktreeArgs {
    pub worktree_path: String,
}

#[derive(Debug, Clone, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct FsListMarkdownDocumentsArgs {
    pub root_path: String,
}

#[derive(Debug, Clone, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct FsAuthorizeExternalPathArgs {
    pub target_path: String,
}

/// Token used when the renderer omits `requestToken`: scoped to the root so two
/// concurrent scans of one root still supersede each other.
fn list_files_token(root_path: &str, request_token: Option<&str>) -> String {
    match request_token {
        Some(token) if !token.is_empty() => token.to_string(),
        _ => format!("listFiles:{root_path}"),
    }
}

#[tauri::command]
#[specta::specta]
pub async fn fs_read_dir(
    state: State<'_, AppState>,
    args: FsReadDirArgs,
) -> Result<Vec<DirEntry>, BridgeError> {
    let fs = Arc::clone(&state.fs);
    run_blocking(move || Ok(fs.read_dir(&args.dir_path)?)).await
}

#[tauri::command]
#[specta::specta]
pub async fn fs_read_file(
    state: State<'_, AppState>,
    args: FsReadFileArgs,
) -> Result<FileContent, BridgeError> {
    let fs = Arc::clone(&state.fs);
    run_blocking(move || Ok(fs.read_file(&args.file_path)?)).await
}

#[tauri::command]
#[specta::specta]
pub async fn fs_write_file(
    state: State<'_, AppState>,
    args: FsWriteFileArgs,
) -> Result<(), BridgeError> {
    let fs = Arc::clone(&state.fs);
    run_blocking(move || Ok(fs.write_file(&args.file_path, &args.content)?)).await
}

#[tauri::command]
#[specta::specta]
pub async fn fs_create_file(
    state: State<'_, AppState>,
    args: FsCreateFileArgs,
) -> Result<(), BridgeError> {
    let fs = Arc::clone(&state.fs);
    run_blocking(move || Ok(fs.create_file(&args.file_path)?)).await
}

#[tauri::command]
#[specta::specta]
pub async fn fs_create_dir(
    state: State<'_, AppState>,
    args: FsCreateDirArgs,
) -> Result<(), BridgeError> {
    let fs = Arc::clone(&state.fs);
    run_blocking(move || Ok(fs.create_dir(&args.dir_path)?)).await
}

#[tauri::command]
#[specta::specta]
pub async fn fs_rename(state: State<'_, AppState>, args: FsRenameArgs) -> Result<(), BridgeError> {
    let fs = Arc::clone(&state.fs);
    run_blocking(move || Ok(fs.rename(&args.old_path, &args.new_path)?)).await
}

#[tauri::command]
#[specta::specta]
pub async fn fs_copy(state: State<'_, AppState>, args: FsCopyArgs) -> Result<(), BridgeError> {
    let fs = Arc::clone(&state.fs);
    run_blocking(move || Ok(fs.copy(&args.source_path, &args.destination_path)?)).await
}

#[tauri::command]
#[specta::specta]
pub async fn fs_delete_path(
    state: State<'_, AppState>,
    args: FsDeletePathArgs,
) -> Result<(), BridgeError> {
    let fs = Arc::clone(&state.fs);
    run_blocking(move || Ok(fs.delete_path(&args.target_path)?)).await
}

#[tauri::command]
#[specta::specta]
pub async fn fs_stat(
    state: State<'_, AppState>,
    args: FsStatArgs,
) -> Result<FileStat, BridgeError> {
    let fs = Arc::clone(&state.fs);
    run_blocking(move || Ok(fs.stat(&args.file_path)?)).await
}

#[tauri::command]
#[specta::specta]
pub async fn fs_path_exists(
    state: State<'_, AppState>,
    args: FsPathExistsArgs,
) -> Result<bool, BridgeError> {
    let fs = Arc::clone(&state.fs);
    run_blocking(move || Ok(fs.path_exists(&args.file_path)?)).await
}

#[tauri::command]
#[specta::specta]
pub async fn fs_paths_exist(
    state: State<'_, AppState>,
    args: FsPathsExistArgs,
) -> Result<Vec<PathExistence>, BridgeError> {
    let fs = Arc::clone(&state.fs);
    run_blocking(move || Ok(fs.paths_exist(&args.file_paths)?)).await
}

#[tauri::command]
#[specta::specta]
pub async fn fs_list_files(
    state: State<'_, AppState>,
    args: FsListFilesArgs,
) -> Result<Vec<String>, BridgeError> {
    let fs = Arc::clone(&state.fs);
    run_blocking(move || {
        let token = list_files_token(&args.root_path, args.request_token.as_deref());
        // Why: the scan must share the service's own cancel registry so
        // `fs_cancel_list_files` reaches the running traversal (Task 5 contract).
        let cancel = fs.cancel_registry();
        Ok(fs.list_files(
            &args.root_path,
            &args.exclude_paths,
            args.max_results.unwrap_or(usize::MAX),
            &token,
            cancel,
        )?)
    })
    .await
}

#[tauri::command]
#[specta::specta]
pub async fn fs_cancel_list_files(
    state: State<'_, AppState>,
    args: FsCancelListFilesArgs,
) -> Result<(), BridgeError> {
    state.fs.cancel_list_files(&args.request_token);
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn fs_search(
    state: State<'_, AppState>,
    args: SearchOptions,
) -> Result<SearchResult, BridgeError> {
    let fs = Arc::clone(&state.fs);
    run_blocking(move || {
        let cancel = fs.cancel_registry();
        Ok(fs.search(args, cancel)?)
    })
    .await
}

#[tauri::command]
#[specta::specta]
pub async fn fs_watch_worktree(
    state: State<'_, AppState>,
    window: Window,
    args: FsWatchWorktreeArgs,
) -> Result<(), BridgeError> {
    state.watchers.watch(&args.worktree_path, window.label());
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn fs_unwatch_worktree(
    state: State<'_, AppState>,
    window: Window,
    args: FsWatchWorktreeArgs,
) -> Result<(), BridgeError> {
    state.watchers.unwatch(&args.worktree_path, window.label());
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn fs_list_markdown_documents(
    state: State<'_, AppState>,
    args: FsListMarkdownDocumentsArgs,
) -> Result<Vec<MarkdownDocument>, BridgeError> {
    let fs = Arc::clone(&state.fs);
    run_blocking(move || Ok(fs.list_markdown_documents(&args.root_path)?)).await
}

#[tauri::command]
#[specta::specta]
pub async fn fs_authorize_external_path(
    state: State<'_, AppState>,
    args: FsAuthorizeExternalPathArgs,
) -> Result<(), BridgeError> {
    let fs = Arc::clone(&state.fs);
    run_blocking(move || Ok(fs.authorize_external(&args.target_path)?)).await
}

/// Serialize the watcher payload exactly as the renderer contract expects; kept
/// as a value-level guard for the event payload shape.
#[allow(dead_code)]
pub(crate) fn fs_changed_payload_json(payload: &ade_fs::FsChangedPayload) -> Value {
    serde_json::to_value(payload).expect("FsChangedPayload serializes")
}

#[cfg(test)]
mod tests {
    use super::*;
    use ade_fs::{FsChangeEvent, FsChangeKind, FsChangedPayload};

    #[test]
    fn list_files_token_prefers_renderer_token() {
        assert_eq!(list_files_token("/repo", Some("req-1")), "req-1");
        assert_eq!(list_files_token("/repo", None), "listFiles:/repo");
        assert_eq!(list_files_token("/repo", Some("")), "listFiles:/repo");
    }

    #[test]
    fn fs_changed_payload_serializes_camel_case() {
        let payload = FsChangedPayload {
            worktree_path: "/repo".to_string(),
            events: vec![FsChangeEvent {
                kind: FsChangeKind::Create,
                absolute_path: "/repo/a.ts".to_string(),
                old_absolute_path: None,
                is_directory: Some(false),
            }],
        };
        assert_eq!(
            fs_changed_payload_json(&payload),
            serde_json::json!({
                "worktreePath": "/repo",
                "events": [{ "kind": "create", "absolutePath": "/repo/a.ts", "isDirectory": false }]
            })
        );
    }
}
