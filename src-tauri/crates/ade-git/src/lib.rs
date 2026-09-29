pub mod base_ref;
pub mod branch;
pub(crate) mod command;
pub mod compare;
pub mod diff;
pub mod history;
pub mod porcelain;
pub mod runner;
pub mod staging;
pub mod status;
pub mod status_read;
pub mod worktree_create;
pub mod worktree_remove;

pub use porcelain::{parse_worktree_list, GitWorktreeEntry};
pub use runner::{run_git_in, CancelToken, GitOutput};

use ade_core::errors::CoreError;
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Output};
use std::time::Duration;

const VERSION_TIMEOUT: Duration = Duration::from_millis(1500);
const COMMAND_TIMEOUT: Duration = Duration::from_secs(10);

pub fn is_available() -> bool {
    let mut command = Command::new("git");
    command.arg("--version");
    matches!(
        output_with_timeout(&mut command, VERSION_TIMEOUT),
        Ok(output) if output.status.success()
    )
}

pub fn rev_parse_toplevel(path: &str) -> Result<String, CoreError> {
    let output = run_git(path, &["rev-parse", "--show-toplevel"])?;
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

pub fn is_inside_work_tree(path: &str) -> bool {
    match run_git(path, &["rev-parse", "--is-inside-work-tree"]) {
        Ok(output) => String::from_utf8_lossy(&output.stdout).trim() == "true",
        Err(_) => false,
    }
}

pub fn worktree_list(path: &str) -> Result<Vec<GitWorktreeEntry>, CoreError> {
    let output = run_git(path, &["worktree", "list", "--porcelain", "-z"])?;
    Ok(parse_worktree_list(&output.stdout))
}

/// Normalize a worktree path for cross-platform comparison.
///
/// Mirrors `canonicalWorktreePath`
/// (`orca:src/main/git/worktree-path-comparison.ts:5-10`) with one addition
/// the oracle gets for free because its callers already hold resolved paths:
/// the path is canonicalized on disk, because `git worktree list` reports the
/// real path it stored (macOS `/var` → `/private/var`). Missing paths resolve
/// through their nearest existing ancestor, so a just-deleted worktree still
/// compares equal to its registration. Dot segments are resolved lexically and
/// Windows-syntax paths are case-folded.
pub fn canonical_worktree_path(path_value: &str) -> String {
    let resolved = resolve_real_path(Path::new(path_value))
        .unwrap_or_else(|_| normalize_lexically(Path::new(path_value)));
    let text = resolved.to_string_lossy().into_owned();
    if looks_like_windows_path(path_value) {
        text.to_lowercase()
    } else {
        text
    }
}

/// Mirrors `areWorktreePathsEqual`
/// (`orca:src/main/git/worktree-path-comparison.ts:12-21`).
pub(crate) fn are_worktree_paths_equal(left: &str, right: &str) -> bool {
    canonical_worktree_path(left) == canonical_worktree_path(right)
}

/// Mirrors `looksLikeWindowsPath` (`orca:src/main/git/worktree-path-comparison.ts:23-25`).
fn looks_like_windows_path(path_value: &str) -> bool {
    let bytes = path_value.as_bytes();
    (bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':')
        || path_value.starts_with("\\\\")
}

/// Real path of a possibly-missing leaf: the nearest existing ancestor is
/// canonicalized (following symlinks) and the missing tail is appended.
fn resolve_real_path(path: &Path) -> std::io::Result<PathBuf> {
    if let Ok(resolved) = std::fs::canonicalize(path) {
        return Ok(resolved);
    }
    let mut ancestor = path.parent();
    while let Some(current) = ancestor {
        if let Ok(resolved) = std::fs::canonicalize(current) {
            let mut result = resolved;
            if let Ok(remainder) = path.strip_prefix(current) {
                result.push(remainder);
            }
            return Ok(result);
        }
        ancestor = current.parent();
    }
    Err(std::io::Error::new(
        std::io::ErrorKind::NotFound,
        "path has no existing ancestor",
    ))
}

/// Lexical path normalization equivalent to Node's `path.posix.normalize`:
/// drops `.` components and resolves `..` by popping.
fn normalize_lexically(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            other => normalized.push(other.as_os_str()),
        }
    }
    normalized
}

/// Shared mapping for a non-zero exit from a commanded git invocation.
pub(crate) fn git_command_failed(args: &[&str], output: &GitOutput) -> CoreError {
    CoreError::GitCommandFailed {
        command: args.join(" "),
        stderr: String::from_utf8_lossy(&output.stderr).trim().to_string(),
        exit_code: output.status.code(),
    }
}

fn run_git(path: &str, args: &[&str]) -> Result<Output, CoreError> {
    let mut command = Command::new("git");
    command.arg("-C").arg(path).args(args);
    let output = output_with_timeout(&mut command, COMMAND_TIMEOUT)?;
    if output.status.success() {
        Ok(output)
    } else {
        Err(CoreError::NotAGitRepository(path.to_string()))
    }
}

fn output_with_timeout(command: &mut Command, timeout: Duration) -> std::io::Result<Output> {
    runner::run_process(command, timeout, None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    #[cfg(unix)]
    #[test]
    fn output_with_timeout_kills_slow_command() {
        let mut command = Command::new("sleep");
        command.arg("30");

        let started = Instant::now();
        let error = output_with_timeout(&mut command, Duration::from_millis(200))
            .expect_err("slow command should time out");

        assert_eq!(error.kind(), std::io::ErrorKind::TimedOut);
        assert!(started.elapsed() < Duration::from_secs(5));
    }

    #[cfg(unix)]
    #[test]
    fn output_with_timeout_captures_stdout_and_exit_status() {
        let mut command = Command::new("echo");
        command.arg("hello");

        let output =
            output_with_timeout(&mut command, Duration::from_secs(5)).expect("echo should run");

        assert!(output.status.success());
        assert_eq!(String::from_utf8_lossy(&output.stdout), "hello\n");
    }
}
