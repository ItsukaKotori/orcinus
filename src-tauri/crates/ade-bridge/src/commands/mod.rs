pub mod agent_sessions;
pub mod agent_status;
pub mod ai_vault;
pub mod app;
pub mod cli_install;
pub mod folder_workspaces;
pub mod fs;
pub mod git;
pub mod notifications;
pub mod notifications_native;
pub mod onboarding;
pub mod platform;
pub mod plugins;
pub mod preflight;
pub mod project_groups;
pub mod pty;
pub mod repos;
pub mod session;
pub mod settings;
pub mod skills;
pub mod ui;
pub mod worktrees;

use crate::errors::BridgeError;

/// Wall-clock milliseconds for command-side timestamps (plugin marketplace
/// entries).
pub fn now_millis() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as i64)
        .unwrap_or(0)
}

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
