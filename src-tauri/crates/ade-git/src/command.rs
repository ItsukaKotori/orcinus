//! Shared git process helpers for read-only query modules.
//!
//! `status_read` keeps its own bounded/streaming runner; this module serves the
//! simpler query paths (`compare`, `history`) that buffer one `git` invocation
//! and fold non-zero exits into [`CoreError::GitCommandFailed`].

use ade_core::errors::CoreError;
use std::time::Duration;

use crate::runner::run_git_in;

pub(crate) const GIT_READ_TIMEOUT: Duration = Duration::from_secs(120);

pub(crate) fn run(worktree_path: &str, args: &[&str]) -> Result<Vec<u8>, CoreError> {
    let output = run_git_in(worktree_path, args, GIT_READ_TIMEOUT, None)?;
    if !output.status.success() {
        return Err(CoreError::GitCommandFailed {
            command: format!("git {}", args.join(" ")),
            stderr: String::from_utf8_lossy(&output.stderr).trim().to_string(),
            exit_code: output.status.code(),
        });
    }
    Ok(output.stdout)
}

pub(crate) fn run_text(worktree_path: &str, args: &[&str]) -> Result<String, CoreError> {
    let stdout = run(worktree_path, args)?;
    Ok(String::from_utf8_lossy(&stdout).trim().to_string())
}
