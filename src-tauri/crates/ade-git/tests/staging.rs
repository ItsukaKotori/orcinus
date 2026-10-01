use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use ade_core::errors::CoreError;
use ade_git::staging::{
    bulk_discard, bulk_stage, bulk_unstage, commit, discard, stage, unstage, upstream_status,
    CommitOutcome,
};
use ade_git::status::{GitStagingArea, GitStatusEntry};
use ade_git::status_read::{status, StatusOptions};

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

fn commit_subject(dir: &Path) -> String {
    let output = git(dir, &["log", "-1", "--format=%s"]);
    String::from_utf8(output.stdout)
        .expect("subject is UTF-8")
        .trim()
        .to_string()
}

/// Identity in the repo's own config, so `ade_git::staging::commit` succeeds
/// even when the test process has no host git identity.
fn configure_identity(repo: &Path) {
    git(repo, &["config", "user.name", "Ade Test"]);
    git(repo, &["config", "user.email", "ade-test@example.com"]);
    git(repo, &["config", "commit.gpgsign", "false"]);
}

/// `main` branch with one commit containing `README.md` (`a\n`).
fn init_repo_with_commit(dir: &TempDir) -> PathBuf {
    let repo = dir.path().join("repo");
    std::fs::create_dir_all(&repo).expect("create repo dir");
    git(&repo, &["init", "--quiet"]);
    git(&repo, &["symbolic-ref", "HEAD", "refs/heads/main"]);
    configure_identity(&repo);
    std::fs::write(repo.join("README.md"), "a\n").expect("write README");
    commit_all(&repo, "init");
    repo
}

fn status_entries(repo: &Path) -> Vec<GitStatusEntry> {
    status(repo.to_str().unwrap(), &StatusOptions::default(), None)
        .expect("status")
        .entries
}

fn area_of(repo: &Path, path: &str) -> Option<GitStagingArea> {
    status_entries(repo)
        .into_iter()
        .find(|entry| entry.path == path)
        .map(|entry| entry.area)
}

fn repo_str(repo: &Path) -> &str {
    repo.to_str().expect("repo path is UTF-8")
}

#[test]
fn stage_and_unstage_round_trip() {
    let dir = TempDir::new("stage-round-trip");
    let repo = init_repo_with_commit(&dir);
    std::fs::write(repo.join("README.md"), "b\n").unwrap();

    stage(repo_str(&repo), "README.md").unwrap();
    assert_eq!(area_of(&repo, "README.md"), Some(GitStagingArea::Staged));

    unstage(repo_str(&repo), "README.md").unwrap();
    assert_eq!(area_of(&repo, "README.md"), Some(GitStagingArea::Unstaged));
    assert!(!status_entries(&repo)
        .iter()
        .any(|entry| entry.path == "README.md" && entry.area == GitStagingArea::Staged));
}

#[test]
fn stage_failure_reports_git_command_failed() {
    let dir = TempDir::new("stage-missing");
    let repo = init_repo_with_commit(&dir);

    let error = stage(repo_str(&repo), "missing.txt").unwrap_err();

    assert!(matches!(error, CoreError::GitCommandFailed { .. }));
}

#[test]
fn bulk_stage_empty_list_is_noop_and_handles_many_paths() {
    let dir = TempDir::new("bulk-stage");
    let repo = init_repo_with_commit(&dir);

    bulk_stage(repo_str(&repo), &[]).unwrap();
    assert!(status_entries(&repo).is_empty());

    let paths: Vec<String> = (0..250).map(|index| format!("f{index:03}.txt")).collect();
    for path in &paths {
        std::fs::write(repo.join(path), "x\n").unwrap();
    }

    bulk_stage(repo_str(&repo), &paths).unwrap();

    let entries = status_entries(&repo);
    assert_eq!(entries.len(), 250);
    assert!(entries
        .iter()
        .all(|entry| entry.area == GitStagingArea::Staged));
}

#[test]
fn bulk_unstage_returns_tracked_files_to_unstaged_and_new_files_to_untracked() {
    let dir = TempDir::new("bulk-unstage");
    let repo = init_repo_with_commit(&dir);
    std::fs::write(repo.join("README.md"), "b\n").unwrap();
    let new_paths: Vec<String> = (0..120).map(|index| format!("f{index:03}.txt")).collect();
    for path in &new_paths {
        std::fs::write(repo.join(path), "x\n").unwrap();
    }
    let mut paths = vec!["README.md".to_string()];
    paths.extend(new_paths.iter().cloned());
    bulk_stage(repo_str(&repo), &paths).unwrap();

    bulk_unstage(repo_str(&repo), &paths).unwrap();

    let entries = status_entries(&repo);
    assert_eq!(entries.len(), 121);
    assert_eq!(
        area_of(&repo, "README.md"),
        Some(GitStagingArea::Unstaged),
        "a tracked modification returns to the unstaged area"
    );
    assert!(
        new_paths
            .iter()
            .all(|path| area_of(&repo, path) == Some(GitStagingArea::Untracked)),
        "newly-added files return to the untracked area after unstage"
    );
    bulk_unstage(repo_str(&repo), &[]).unwrap();
}

#[test]
fn stage_treats_pathspec_magic_characters_literally() {
    let dir = TempDir::new("stage-literal");
    let repo = init_repo_with_commit(&dir);
    let names = ["weird [1].txt", "star*.txt", ":(glob)x.txt", "sp ace.txt"];
    for name in names {
        std::fs::write(repo.join(name), "x\n").unwrap();
        stage(repo_str(&repo), name).unwrap();
    }

    let entries = status_entries(&repo);
    for name in names {
        assert_eq!(
            area_of(&repo, name),
            Some(GitStagingArea::Staged),
            "{name} should be staged"
        );
    }
    assert_eq!(entries.len(), names.len());
}

#[test]
fn discard_restores_tracked_file_and_removes_untracked_file() {
    let dir = TempDir::new("discard-basic");
    let repo = init_repo_with_commit(&dir);
    std::fs::write(repo.join("README.md"), "b\n").unwrap();
    std::fs::write(repo.join("untracked.txt"), "new\n").unwrap();

    discard(repo_str(&repo), "README.md").unwrap();
    assert_eq!(
        std::fs::read_to_string(repo.join("README.md")).unwrap(),
        "a\n"
    );

    discard(repo_str(&repo), "untracked.txt").unwrap();
    assert!(!repo.join("untracked.txt").exists());
}

#[test]
fn discard_restores_a_deleted_tracked_file() {
    let dir = TempDir::new("discard-deleted");
    let repo = init_repo_with_commit(&dir);
    std::fs::remove_file(repo.join("README.md")).unwrap();

    discard(repo_str(&repo), "README.md").unwrap();

    assert_eq!(
        std::fs::read_to_string(repo.join("README.md")).unwrap(),
        "a\n"
    );
}

#[test]
fn discard_removes_an_untracked_directory_tree() {
    let dir = TempDir::new("discard-dir");
    let repo = init_repo_with_commit(&dir);
    std::fs::create_dir_all(repo.join("newdir/nested")).unwrap();
    std::fs::write(repo.join("newdir/nested/file.txt"), "x\n").unwrap();

    discard(repo_str(&repo), "newdir").unwrap();

    assert!(!repo.join("newdir").exists());
}

#[test]
fn discard_rejects_path_outside_worktree() {
    let dir = TempDir::new("discard-escape");
    let repo = init_repo_with_commit(&dir);
    std::fs::write(dir.path().join("escape.txt"), "keep\n").unwrap();

    let error = discard(repo_str(&repo), "../escape.txt").unwrap_err();

    assert!(matches!(error, CoreError::PathNotAllowed(_)));
    assert_eq!(
        std::fs::read_to_string(dir.path().join("escape.txt")).unwrap(),
        "keep\n"
    );
}

#[test]
fn discard_rejects_the_worktree_root_itself() {
    let dir = TempDir::new("discard-root");
    let repo = init_repo_with_commit(&dir);

    for path in ["", ".", "sub/.."] {
        let error = discard(repo_str(&repo), path).unwrap_err();
        assert!(
            matches!(error, CoreError::PathNotAllowed(_)),
            "{path:?} should be rejected"
        );
    }
}

#[cfg(unix)]
#[test]
fn discard_removes_a_symlink_leaf_without_following_it() {
    let dir = TempDir::new("discard-symlink-leaf");
    let repo = init_repo_with_commit(&dir);
    let outside = dir.path().join("outside-secret.txt");
    std::fs::write(&outside, "secret\n").unwrap();
    std::os::unix::fs::symlink(&outside, repo.join("link.txt")).unwrap();

    discard(repo_str(&repo), "link.txt").unwrap();

    assert!(std::fs::symlink_metadata(repo.join("link.txt")).is_err());
    assert_eq!(std::fs::read_to_string(&outside).unwrap(), "secret\n");
}

#[cfg(unix)]
#[test]
fn discard_rejects_path_through_symlinked_parent_outside_worktree() {
    let dir = TempDir::new("discard-symlink-parent");
    let repo = init_repo_with_commit(&dir);
    let outside_dir = dir.path().join("outside-dir");
    std::fs::create_dir_all(&outside_dir).unwrap();
    std::fs::write(outside_dir.join("secret.txt"), "secret\n").unwrap();
    std::os::unix::fs::symlink(&outside_dir, repo.join("link-dir")).unwrap();

    let error = discard(repo_str(&repo), "link-dir/secret.txt").unwrap_err();

    assert!(matches!(error, CoreError::PathNotAllowed(_)));
    assert_eq!(
        std::fs::read_to_string(outside_dir.join("secret.txt")).unwrap(),
        "secret\n"
    );
}

#[test]
fn bulk_discard_restores_tracked_and_removes_untracked() {
    let dir = TempDir::new("bulk-discard");
    let repo = init_repo_with_commit(&dir);
    std::fs::write(repo.join("README.md"), "b\n").unwrap();
    std::fs::write(repo.join("untracked.txt"), "new\n").unwrap();
    let paths = vec!["README.md".to_string(), "untracked.txt".to_string()];

    bulk_discard(repo_str(&repo), &paths).unwrap();

    assert_eq!(
        std::fs::read_to_string(repo.join("README.md")).unwrap(),
        "a\n"
    );
    assert!(!repo.join("untracked.txt").exists());
    bulk_discard(repo_str(&repo), &[]).unwrap();
}

#[test]
fn bulk_discard_rejects_outside_path_before_mutating() {
    let dir = TempDir::new("bulk-discard-escape");
    let repo = init_repo_with_commit(&dir);
    std::fs::write(repo.join("README.md"), "b\n").unwrap();
    std::fs::write(dir.path().join("escape.txt"), "keep\n").unwrap();
    let paths = vec!["README.md".to_string(), "../escape.txt".to_string()];

    let error = bulk_discard(repo_str(&repo), &paths).unwrap_err();

    assert!(matches!(error, CoreError::PathNotAllowed(_)));
    assert_eq!(
        std::fs::read_to_string(repo.join("README.md")).unwrap(),
        "b\n"
    );
    assert!(dir.path().join("escape.txt").exists());
}

#[test]
fn commit_returns_success_and_commit_visible_in_log() {
    let dir = TempDir::new("commit-success");
    let repo = init_repo_with_commit(&dir);
    std::fs::write(repo.join("README.md"), "b\n").unwrap();
    stage(repo_str(&repo), "README.md").unwrap();

    let outcome = commit(repo_str(&repo), "feat: x").unwrap();

    assert_eq!(
        outcome,
        CommitOutcome {
            success: true,
            error: None
        }
    );
    assert_eq!(commit_subject(&repo), "feat: x");
    assert!(status_entries(&repo).is_empty());
}

#[test]
fn commit_folds_spawn_failures_into_the_outcome() {
    let dir = TempDir::new("commit-spawn-failure");
    let repo = init_repo_with_commit(&dir);
    // A NUL byte cannot reach execve, so spawn fails before git runs: this is
    // the `run_git_in` Err path that must not escape as a rejected command.
    let invalid_path = format!("{}\0", repo_str(&repo));

    let outcome = commit(&invalid_path, "x").unwrap();

    assert!(!outcome.success);
    let error = outcome.error.expect("spawn failure carries an error");
    assert!(!error.trim().is_empty(), "error text should be surfaced");
}

#[cfg(unix)]
#[test]
fn commit_reports_hook_failure_via_success_false() {
    use std::os::unix::fs::PermissionsExt;

    let dir = TempDir::new("commit-hook");
    let repo = init_repo_with_commit(&dir);
    std::fs::write(repo.join("README.md"), "b\n").unwrap();
    stage(repo_str(&repo), "README.md").unwrap();
    let hook = repo.join(".git/hooks/pre-commit");
    std::fs::write(
        &hook,
        "#!/bin/sh\necho \"pre-commit denied: policy\" >&2\nexit 1\n",
    )
    .unwrap();
    std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755)).unwrap();

    let outcome = commit(repo_str(&repo), "should not land").unwrap();

    assert!(!outcome.success);
    let error = outcome.error.expect("hook failure carries an error");
    assert!(
        error.contains("pre-commit denied"),
        "error should include hook stderr, got: {error}"
    );
    assert_eq!(commit_subject(&repo), "init");
}

#[test]
fn commit_requires_message() {
    let dir = TempDir::new("commit-message");
    let repo = init_repo_with_commit(&dir);

    for message in ["", "   ", "\n\t"] {
        let error = commit(repo_str(&repo), message).unwrap_err();
        assert!(
            matches!(error, CoreError::InvalidInput(_)),
            "{message:?} should be rejected"
        );
    }
    assert_eq!(commit_subject(&repo), "init");
}

#[test]
fn upstream_status_without_upstream_reports_false() {
    let dir = TempDir::new("upstream-none");
    let repo = init_repo_with_commit(&dir);

    let upstream = upstream_status(repo_str(&repo)).unwrap();

    assert!(!upstream.has_upstream);
    assert_eq!(upstream.upstream_name, None);
    assert_eq!(upstream.ahead, 0);
    assert_eq!(upstream.behind, 0);
    assert_eq!(upstream.has_configured_push_target, None);
    assert_eq!(upstream.behind_commits_are_patch_equivalent, None);
}

#[test]
fn upstream_status_counts_ahead_behind_when_upstream_configured() {
    let dir = TempDir::new("upstream-configured");
    let origin = dir.path().join("origin.git");
    let seed = dir.path().join("seed");
    let work = dir.path().join("work");
    let other = dir.path().join("other");

    std::fs::create_dir_all(&origin).unwrap();
    git(
        &origin,
        &["init", "--quiet", "--bare", "--initial-branch=main"],
    );

    std::fs::create_dir_all(&seed).unwrap();
    git(&seed, &["init", "--quiet"]);
    git(&seed, &["symbolic-ref", "HEAD", "refs/heads/main"]);
    configure_identity(&seed);
    std::fs::write(seed.join("README.md"), "a\n").unwrap();
    commit_all(&seed, "init");
    git(
        &seed,
        &["remote", "add", "origin", origin.to_str().unwrap()],
    );
    git(&seed, &["push", "--quiet", "-u", "origin", "main"]);

    git(
        dir.path(),
        &[
            "clone",
            "--quiet",
            origin.to_str().unwrap(),
            work.to_str().unwrap(),
        ],
    );
    configure_identity(&work);

    std::fs::write(work.join("one.txt"), "1\n").unwrap();
    commit_all(&work, "one");
    std::fs::write(work.join("two.txt"), "2\n").unwrap();
    commit_all(&work, "two");

    let upstream = upstream_status(repo_str(&work)).unwrap();
    assert!(upstream.has_upstream);
    assert_eq!(upstream.upstream_name.as_deref(), Some("origin/main"));
    assert_eq!(upstream.ahead, 2);
    assert_eq!(upstream.behind, 0);

    git(
        &work,
        &["push", "--quiet", "origin", "HEAD~1:refs/heads/main"],
    );
    git(&work, &["fetch", "--quiet", "origin"]);

    let upstream = upstream_status(repo_str(&work)).unwrap();
    assert_eq!(upstream.ahead, 1);
    assert_eq!(upstream.behind, 0);

    git(
        dir.path(),
        &[
            "clone",
            "--quiet",
            origin.to_str().unwrap(),
            other.to_str().unwrap(),
        ],
    );
    configure_identity(&other);
    std::fs::write(other.join("three.txt"), "3\n").unwrap();
    commit_all(&other, "three");
    git(&other, &["push", "--quiet", "origin", "main"]);
    git(&work, &["fetch", "--quiet", "origin"]);

    let upstream = upstream_status(repo_str(&work)).unwrap();
    assert_eq!(upstream.ahead, 1, "one local commit is still unpushed");
    assert_eq!(upstream.behind, 1, "one remote commit was fetched");

    git(&work, &["reset", "--hard", "--quiet", "HEAD~1"]);

    let upstream = upstream_status(repo_str(&work)).unwrap();
    assert_eq!(upstream.ahead, 0);
    assert_eq!(upstream.behind, 1);
}
