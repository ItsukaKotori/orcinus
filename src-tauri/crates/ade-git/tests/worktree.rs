//! Worktree creation support that needs a real git repository: base-ref
//! probing and git username resolution.
//!
//! Mirrors `orca:src/main/git/repo-default-base-ref.ts`,
//! `orca:src/main/worktree-create-base.ts` and the explicit-config half of
//! `orca:src/main/git/git-username.ts` (the `gh` CLI probe is not ported).

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use ade_core::errors::CoreError;
use ade_git::branch::{resolve_create_base, resolve_default_base_ref, resolve_git_username};

static COUNTER: AtomicU64 = AtomicU64::new(0);

struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new(name: &str) -> Self {
        let unique = COUNTER.fetch_add(1, Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("ade-git-{name}-{}-{unique}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("create temp dir");
        Self { path }
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

/// git with the user's environment stripped, so host config cannot leak into fixtures.
fn git_command(dir: &Path) -> Command {
    let mut command = Command::new("git");
    command
        .arg("-C")
        .arg(dir)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_AUTHOR_NAME", "Ade Test")
        .env("GIT_AUTHOR_EMAIL", "ade-test@example.com")
        .env("GIT_COMMITTER_NAME", "Ade Test")
        .env("GIT_COMMITTER_EMAIL", "ade-test@example.com")
        .env("GIT_TERMINAL_PROMPT", "0");
    command
}

fn git(dir: &Path, args: &[&str]) -> std::process::Output {
    let output = git_command(dir).args(args).output().expect("run git");
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    output
}

fn commit_all(dir: &Path, message: &str) {
    git(dir, &["add", "-A"]);
    git(
        dir,
        &[
            "-c",
            "commit.gpgsign=false",
            "commit",
            "--quiet",
            "-m",
            message,
        ],
    );
}

/// `main` branch with one commit containing `README.md` (`line1\n`).
fn init_repo_with_commit(dir: &TempDir) -> PathBuf {
    let repo = dir.path().join("repo");
    std::fs::create_dir_all(&repo).expect("create repo dir");
    git(&repo, &["init", "--quiet"]);
    git(&repo, &["symbolic-ref", "HEAD", "refs/heads/main"]);
    std::fs::write(repo.join("README.md"), "line1\n").expect("write README");
    commit_all(&repo, "init");
    repo
}

fn repo_path(repo: &Path) -> &str {
    repo.to_str().expect("repo path is UTF-8")
}

fn invalid_input_message(error: CoreError) -> String {
    match error {
        CoreError::InvalidInput(message) => message,
        other => panic!("expected InvalidInput, got {other:?}"),
    }
}

#[test]
fn resolve_default_base_ref_prefers_origin_head_then_remotes_then_local() {
    let dir = TempDir::new("worktree-base");
    let repo = init_repo_with_commit(&dir);
    let repo = repo_path(&repo);

    assert_eq!(resolve_default_base_ref(repo).unwrap(), "refs/heads/main");

    git(
        Path::new(repo),
        &["update-ref", "refs/remotes/origin/master", "HEAD"],
    );
    assert_eq!(
        resolve_default_base_ref(repo).unwrap(),
        "refs/remotes/origin/master"
    );

    git(
        Path::new(repo),
        &["update-ref", "refs/remotes/origin/main", "HEAD"],
    );
    assert_eq!(
        resolve_default_base_ref(repo).unwrap(),
        "refs/remotes/origin/main"
    );

    git(
        Path::new(repo),
        &[
            "symbolic-ref",
            "refs/remotes/origin/HEAD",
            "refs/remotes/origin/main",
        ],
    );
    assert_eq!(
        resolve_default_base_ref(repo).unwrap(),
        "refs/remotes/origin/HEAD"
    );
}

#[test]
fn resolve_default_base_ref_errors_with_oracle_message_when_unresolvable() {
    let dir = TempDir::new("worktree-base-empty");
    let repo = dir.path().join("repo");
    std::fs::create_dir_all(&repo).expect("create repo dir");
    git(&repo, &["init", "--quiet"]);

    assert_eq!(
        invalid_input_message(resolve_default_base_ref(repo_path(&repo)).unwrap_err()),
        "Could not resolve a default base ref for this repo. Pick a base branch explicitly and try again."
    );
}

#[test]
fn resolve_create_base_prefers_explicit_then_usable_repo_ref_then_default() {
    let dir = TempDir::new("worktree-create-base");
    let repo = init_repo_with_commit(&dir);
    let repo = repo_path(&repo);

    // Explicit refs are authoritative without a probe (oracle worktree-create-base.ts:11-13).
    assert_eq!(
        resolve_create_base(repo, Some("develop"), None).unwrap(),
        "develop"
    );
    assert_eq!(
        resolve_create_base(repo, Some("develop"), Some("main")).unwrap(),
        "develop"
    );
    // Empty explicit values fall through, like the oracle's falsy checks.
    assert_eq!(
        resolve_create_base(repo, Some(""), Some("main")).unwrap(),
        "main"
    );
    assert_eq!(
        resolve_create_base(repo, None, Some("main")).unwrap(),
        "main"
    );
    assert_eq!(
        resolve_create_base(repo, None, Some("")).unwrap(),
        "refs/heads/main"
    );
    assert_eq!(
        resolve_create_base(repo, None, None).unwrap(),
        "refs/heads/main"
    );
    // Stale persisted refs fall back to the detected default.
    assert_eq!(
        resolve_create_base(repo, None, Some("does-not-exist")).unwrap(),
        "refs/heads/main"
    );
}

#[test]
fn resolve_create_base_errors_when_no_base_resolves() {
    let dir = TempDir::new("worktree-create-base-empty");
    let repo = dir.path().join("repo");
    std::fs::create_dir_all(&repo).expect("create repo dir");
    git(&repo, &["init", "--quiet"]);

    assert_eq!(
        invalid_input_message(
            resolve_create_base(repo_path(&repo), None, Some("also-missing")).unwrap_err()
        ),
        "Could not resolve a default base ref for this repo. Pick a base branch explicitly and try again."
    );
}

#[test]
fn resolve_git_username_prefers_github_user_over_user_username() {
    let dir = TempDir::new("git-username-priority");
    let repo = init_repo_with_commit(&dir);
    git(&repo, &["config", "--local", "github.user", "bob"]);
    git(&repo, &["config", "--local", "user.username", "alice"]);

    assert_eq!(
        resolve_git_username(repo_path(&repo)).as_deref(),
        Some("bob")
    );
}

#[test]
fn resolve_git_username_falls_back_when_github_user_is_unsafe() {
    let dir = TempDir::new("git-username-fallback");
    let repo = init_repo_with_commit(&dir);
    git(&repo, &["config", "--local", "github.user", "team/bob"]);
    git(&repo, &["config", "--local", "user.username", "alice"]);

    assert_eq!(
        resolve_git_username(repo_path(&repo)).as_deref(),
        Some("alice")
    );
}

#[test]
fn resolve_git_username_normalizes_configured_values() {
    let dir = TempDir::new("git-username-normalize");
    let repo = init_repo_with_commit(&dir);
    // Local values shadow any host-level config, so this stays deterministic.
    git(&repo, &["config", "--local", "github.user", "bad name"]);
    git(
        &repo,
        &[
            "config",
            "--local",
            "user.username",
            "123+alice@example.com",
        ],
    );

    assert_eq!(
        resolve_git_username(repo_path(&repo)).as_deref(),
        Some("alice")
    );
}

#[test]
fn resolve_git_username_is_none_without_usable_config() {
    let dir = TempDir::new("git-username-none");
    let repo = init_repo_with_commit(&dir);
    // Local values shadow any host-level config, so this stays deterministic.
    git(&repo, &["config", "--local", "github.user", "bad name"]);
    git(&repo, &["config", "--local", "user.username", ".hidden"]);

    assert_eq!(resolve_git_username(repo_path(&repo)), None);
}
