pub mod app;
pub mod folder_workspaces;
pub mod fs;
pub mod git;
pub mod onboarding;
pub mod platform;
pub mod preflight;
pub mod project_groups;
pub mod pty;
pub mod repos;
pub mod session;
pub mod settings;
pub mod ui;
pub mod worktrees;

use crate::errors::BridgeError;

/// Run blocking IO (fs, search, watch installs) off the async runtime threads,
/// surfacing a panicked task as a bridge error instead of a silent hang.
pub(crate) async fn run_blocking<T, F>(task: F) -> Result<T, BridgeError>
where
    F: FnOnce() -> Result<T, BridgeError> + Send + 'static,
    T: Send + 'static,
{
    tauri::async_runtime::spawn_blocking(task)
        .await
        .map_err(|error| BridgeError::message(format!("blocking task failed: {error}")))?
}
