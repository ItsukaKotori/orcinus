//! Worktree creation support that needs a real git repository: base-ref
//! probing, git username resolution, `git worktree add`/`remove` and branch
//! cleanup.
//!
//! Mirrors `orca:src/main/git/repo-default-base-ref.ts`,
//! `orca:src/main/worktree-create-base.ts`, the explicit-config half of
//! `orca:src/main/git/git-username.ts` (the `gh` CLI probe is not ported),
//! `orca:src/main/git/worktree-add.ts`, `worktree-removal*.ts` and
//! `worktree-branch-removal.ts`.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, MutexGuard, Once};

use ade_core::errors::CoreError;
use ade_git::branch::{resolve_create_base, resolve_default_base_ref, resolve_git_username};
use ade_git::worktree_create::{
    configure_branch_base, ensure_push_auto_setup_remote, worktree_add, AddWorktreeRequest,
};
use ade_git::worktree_remove::{
    assert_worktree_removable, delete_branch, force_delete_branch, worktree_remove,
    BranchDeleteOutcome,
};

static COUNTER: AtomicU64 = AtomicU64::new(0);
static ENV_LOCK: Mutex<()> = Mutex::new(());
static ENV_INIT: Once = Once::new();

/// Serializes the tests that drive library-spawned git and strips the host's
/// git config once, so `push.autoSetupRemote` and identity cannot leak into
/// the fixtures (the direct [`git_command`] calls are isolated regardless).
fn hermetic_env() -> MutexGuard<'static, ()> {
    let guard = ENV_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    ENV_INIT.call_once(|| {
        std::env::set_var("GIT_CONFIG_NOSYSTEM", "1");
        std::env::set_var("GIT_CONFIG_GLOBAL", "/dev/null");
        std::env::set_var("GIT_AUTHOR_NAME", "Ade Test");
        std::env::set_var("GIT_AUTHOR_EMAIL", "ade-test@example.com");
        std::env::set_var("GIT_COMMITTER_NAME", "Ade Test");
        std::env::set_var("GIT_COMMITTER_EMAIL", "ade-test@example.com");
        std::env::set_var("GIT_TERMINAL_PROMPT", "0");
    });
    guard
}

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

/// git invocation that tolerates a non-zero exit, for asserting failures.
fn try_git(dir: &Path, args: &[&str]) -> std::process::Output {
    git_command(dir).args(args).output().expect("run git")
}

fn output_text(output: &std::process::Output) -> String {
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

/// The linked worktree path used by the add/remove tests, a sibling of `repo`.
fn linked_path(dir: &TempDir) -> PathBuf {
    dir.path().join("wt-feature")
}

fn str_path(path: &Path) -> &str {
    path.to_str().expect("path is UTF-8")
}

/// `feature/x` created from `main` through the library under test.
fn add_feature_worktree(repo: &Path, linked: &Path) {
    worktree_add(&AddWorktreeRequest {
        repo_path: repo_path(repo).to_string(),
        worktree_path: str_path(linked).to_string(),
        branch: "feature/x".to_string(),
        base_ref: "main".to_string(),
    })
    .expect("worktree_add");
}

/// Point the linked worktree's branch at one extra commit.
fn commit_on_linked(linked: &Path) -> String {
    std::fs::write(linked.join("feature.txt"), "feature\n").expect("write feature file");
    commit_all(linked, "feature commit");
    output_text(&git(linked, &["rev-parse", "HEAD"]))
}

fn same_path(listed: &str, expected: &Path) -> bool {
    std::fs::canonicalize(listed).expect("canonicalize listed path")
        == std::fs::canonicalize(expected).expect("canonicalize expected path")
}

#[test]
fn worktree_add_creates_linked_worktree_on_new_branch() {
    let _env = hermetic_env();
    let dir = TempDir::new("worktree-add");
    let repo = init_repo_with_commit(&dir);
    let linked = linked_path(&dir);

    add_feature_worktree(&repo, &linked);

    let entries = ade_git::worktree_list(repo_path(&repo)).expect("worktree_list");
    assert_eq!(entries.len(), 2);
    assert!(
        linked.join(".git").is_file(),
        "linked worktree has a .git file"
    );
    let linked_entry = entries
        .iter()
        .find(|entry| same_path(&entry.path, &linked))
        .expect("linked worktree entry");
    assert_eq!(linked_entry.branch.as_deref(), Some("refs/heads/feature/x"));
    assert!(!linked_entry.is_main_worktree);

    configure_branch_base(str_path(&linked), "feature/x", "main").expect("configure_branch_base");
    let value = output_text(&git(
        &linked,
        &["config", "--local", "--get", "branch.feature/x.base"],
    ));
    assert_eq!(value, "main");
}

#[test]
fn worktree_add_rejects_registered_path_with_conflict_message() {
    let _env = hermetic_env();
    let dir = TempDir::new("worktree-add-conflict");
    let repo = init_repo_with_commit(&dir);
    let linked = linked_path(&dir);
    add_feature_worktree(&repo, &linked);

    let error = worktree_add(&AddWorktreeRequest {
        repo_path: repo_path(&repo).to_string(),
        worktree_path: str_path(&linked).to_string(),
        branch: "feature/other".to_string(),
        base_ref: "main".to_string(),
    })
    .unwrap_err();

    assert_eq!(
        invalid_input_message(error),
        format!("Worktree path already exists locally: {}", linked.display())
    );
}

#[test]
fn ensure_push_auto_setup_remote_sets_once() {
    let _env = hermetic_env();
    let dir = TempDir::new("push-auto-setup");
    let repo = init_repo_with_commit(&dir);
    let linked = linked_path(&dir);
    add_feature_worktree(&repo, &linked);

    ensure_push_auto_setup_remote(str_path(&linked)).expect("ensure_push_auto_setup_remote");

    let value = output_text(&git(
        &linked,
        &["config", "--local", "--get", "push.autoSetupRemote"],
    ));
    assert_eq!(value, "true");

    // The second call observes the value and leaves it alone.
    ensure_push_auto_setup_remote(str_path(&linked)).expect("ensure is idempotent");
}

#[test]
fn assert_worktree_removable_rejects_dirty_without_force() {
    let _env = hermetic_env();
    let dir = TempDir::new("worktree-dirty");
    let repo = init_repo_with_commit(&dir);
    let linked = linked_path(&dir);
    add_feature_worktree(&repo, &linked);
    std::fs::write(linked.join("dirty.txt"), "dirty\n").expect("write dirty file");

    let error = assert_worktree_removable(repo_path(&repo), str_path(&linked), false).unwrap_err();
    assert_eq!(
        invalid_input_message(error),
        "Worktree has uncommitted or untracked changes."
    );

    assert_worktree_removable(repo_path(&repo), str_path(&linked), true)
        .expect("force skips the dirty check");
}

#[test]
fn assert_worktree_removable_rejects_missing_registration() {
    let _env = hermetic_env();
    let dir = TempDir::new("worktree-missing");
    let repo = init_repo_with_commit(&dir);
    let missing = dir.path().join("wt-never-added");

    let error = assert_worktree_removable(repo_path(&repo), str_path(&missing), false).unwrap_err();

    assert_eq!(
        invalid_input_message(error),
        format!(
            "Worktree registration changed during deletion: {}",
            missing.display()
        )
    );
}

#[test]
fn worktree_remove_deletes_directory_and_registration() {
    let _env = hermetic_env();
    let dir = TempDir::new("worktree-remove");
    let repo = init_repo_with_commit(&dir);
    let linked = linked_path(&dir);
    add_feature_worktree(&repo, &linked);

    worktree_remove(repo_path(&repo), str_path(&linked), false).expect("worktree_remove");

    assert!(!linked.exists());
    let entries = ade_git::worktree_list(repo_path(&repo)).expect("worktree_list");
    assert_eq!(entries.len(), 1);
    assert!(entries[0].is_main_worktree);
}

#[test]
fn worktree_remove_succeeds_when_directory_and_registration_are_gone() {
    let _env = hermetic_env();
    let dir = TempDir::new("worktree-remove-missing");
    let repo = init_repo_with_commit(&dir);
    let linked = linked_path(&dir);
    add_feature_worktree(&repo, &linked);
    std::fs::remove_dir_all(&linked).expect("delete linked directory by hand");
    // Prune first so `git worktree remove` itself fails, forcing the fallback.
    git(&repo, &["worktree", "prune"]);

    worktree_remove(repo_path(&repo), str_path(&linked), false).expect("idempotent removal");

    assert!(!linked.exists());
    let entries = ade_git::worktree_list(repo_path(&repo)).expect("worktree_list");
    assert_eq!(entries.len(), 1);
}

#[test]
fn delete_branch_preserves_unmerged_branch_and_deletes_merged_one() {
    let _env = hermetic_env();
    let dir = TempDir::new("branch-delete");
    let repo = init_repo_with_commit(&dir);
    let linked = linked_path(&dir);
    add_feature_worktree(&repo, &linked);
    let feature_head = commit_on_linked(&linked);
    worktree_remove(repo_path(&repo), str_path(&linked), false).expect("worktree_remove");

    let outcome =
        delete_branch(repo_path(&repo), "refs/heads/feature/x", false).expect("delete_branch");
    match outcome {
        BranchDeleteOutcome::Preserved { branch_name, head } => {
            assert_eq!(branch_name, "feature/x");
            assert_eq!(head.as_deref(), Some(feature_head.as_str()));
        }
        other => panic!("expected Preserved, got {other:?}"),
    }

    // Fast-forward `main` so the branch is fully merged, then `-d` deletes it.
    git(&repo, &["merge", "--ff-only", "feature/x"]);
    let outcome =
        delete_branch(repo_path(&repo), "refs/heads/feature/x", false).expect("delete_branch");
    assert_eq!(outcome, BranchDeleteOutcome::Deleted);
    assert!(
        !try_git(&repo, &["rev-parse", "--verify", "refs/heads/feature/x"])
            .status
            .success()
    );
}

#[test]
fn delete_branch_skips_non_local_branch_refs() {
    let _env = hermetic_env();
    let dir = TempDir::new("branch-skip");
    let repo = init_repo_with_commit(&dir);

    let outcome = delete_branch(repo_path(&repo), "refs/remotes/origin/main", false)
        .expect("non-local refs are skipped");
    assert_eq!(outcome, BranchDeleteOutcome::Skipped);
    let outcome = delete_branch(repo_path(&repo), "", false).expect("empty refs are skipped");
    assert_eq!(outcome, BranchDeleteOutcome::Skipped);
}

#[test]
fn delete_branch_skips_branch_checked_out_in_worktree() {
    let _env = hermetic_env();
    let dir = TempDir::new("branch-checked-out");
    let repo = init_repo_with_commit(&dir);
    let linked = linked_path(&dir);
    add_feature_worktree(&repo, &linked);

    let outcome = delete_branch(repo_path(&repo), "refs/heads/feature/x", false)
        .expect("a checked-out branch is skipped, not an error");
    assert_eq!(outcome, BranchDeleteOutcome::Skipped);
    assert!(
        try_git(&repo, &["rev-parse", "--verify", "refs/heads/feature/x"])
            .status
            .success()
    );
}

#[test]
fn delete_branch_prunes_stale_registration_and_retries() {
    let _env = hermetic_env();
    let dir = TempDir::new("branch-stale");
    let repo = init_repo_with_commit(&dir);
    let linked = linked_path(&dir);
    add_feature_worktree(&repo, &linked);
    // A manually deleted worktree keeps a stale registration that still pins
    // the branch; the checked-out refusal must trigger prune + one retry.
    std::fs::remove_dir_all(&linked).expect("delete linked directory by hand");

    let outcome = delete_branch(repo_path(&repo), "refs/heads/feature/x", false)
        .expect("a stale registration is reclaimed");

    assert_eq!(outcome, BranchDeleteOutcome::Deleted);
    assert!(
        !try_git(&repo, &["rev-parse", "--verify", "refs/heads/feature/x"])
            .status
            .success()
    );
}

#[test]
fn force_delete_branch_enforces_cas() {
    let _env = hermetic_env();
    let dir = TempDir::new("branch-force-delete");
    let repo = init_repo_with_commit(&dir);
    let linked = linked_path(&dir);
    add_feature_worktree(&repo, &linked);
    configure_branch_base(str_path(&linked), "feature/x", "main").expect("configure_branch_base");
    let feature_head = commit_on_linked(&linked);
    worktree_remove(repo_path(&repo), str_path(&linked), false).expect("worktree_remove");

    // Any head other than the preserved one must fail the CAS; `main` still
    // points at the base commit here, so it is a valid but different OID.
    let wrong_head = output_text(&git(&repo, &["rev-parse", "refs/heads/main"]));
    assert_ne!(wrong_head, feature_head);
    let error = force_delete_branch(repo_path(&repo), "feature/x", &wrong_head).unwrap_err();
    assert_eq!(
        invalid_input_message(error),
        "Local branch \"feature/x\" changed after the workspace was deleted. Review it before deleting it."
    );
    // The mismatch must not touch the branch or its config.
    assert!(
        try_git(&repo, &["rev-parse", "--verify", "refs/heads/feature/x"])
            .status
            .success()
    );
    assert!(try_git(
        &repo,
        &["config", "--local", "--get", "branch.feature/x.base"]
    )
    .status
    .success());

    force_delete_branch(repo_path(&repo), "feature/x", &feature_head).expect("force_delete_branch");
    assert!(
        !try_git(&repo, &["rev-parse", "--verify", "refs/heads/feature/x"])
            .status
            .success()
    );
    // `config --remove-section` runs best-effort after the ref deletion.
    assert!(!try_git(
        &repo,
        &["config", "--local", "--get", "branch.feature/x.base"]
    )
    .status
    .success());
}

#[test]
fn force_delete_branch_rejects_checked_out_branch() {
    let _env = hermetic_env();
    let dir = TempDir::new("branch-force-checked-out");
    let repo = init_repo_with_commit(&dir);
    let linked = linked_path(&dir);
    add_feature_worktree(&repo, &linked);
    let feature_head = output_text(&git(&repo, &["rev-parse", "refs/heads/feature/x"]));

    let error = force_delete_branch(repo_path(&repo), "feature/x", &feature_head).unwrap_err();
    assert_eq!(
        invalid_input_message(error),
        "Local branch \"feature/x\" is checked out in another worktree."
    );
    assert!(
        try_git(&repo, &["rev-parse", "--verify", "refs/heads/feature/x"])
            .status
            .success()
    );
}

#[test]
fn force_delete_branch_rejects_invalid_arguments() {
    let _env = hermetic_env();
    let dir = TempDir::new("branch-force-invalid");
    let repo = init_repo_with_commit(&dir);
    let linked = linked_path(&dir);
    add_feature_worktree(&repo, &linked);
    let feature_head = output_text(&git(&repo, &["rev-parse", "refs/heads/feature/x"]));

    assert_eq!(
        invalid_input_message(
            force_delete_branch(repo_path(&repo), "", &feature_head).unwrap_err()
        ),
        "Invalid branch name"
    );
    assert_eq!(
        invalid_input_message(
            force_delete_branch(repo_path(&repo), "feature\0x", &feature_head).unwrap_err()
        ),
        "Invalid branch name"
    );
    assert_eq!(
        invalid_input_message(force_delete_branch(repo_path(&repo), "feature/x", "").unwrap_err()),
        "Cannot force-delete local branch \"feature/x\" without the commit Git preserved."
    );
}

#[test]
fn remove_locked_worktree_is_rejected() {
    let _env = hermetic_env();
    let dir = TempDir::new("worktree-locked");
    let repo = init_repo_with_commit(&dir);
    let linked = linked_path(&dir);
    add_feature_worktree(&repo, &linked);
    git(
        &repo,
        &["worktree", "lock", "--reason", "in use", str_path(&linked)],
    );

    let error = assert_worktree_removable(repo_path(&repo), str_path(&linked), false).unwrap_err();
    let message = invalid_input_message(error);
    assert!(message.contains("git worktree unlock"), "{message}");
    assert!(message.contains("in use"), "{message}");

    // A Git lock is an external safety contract: force must not bypass it.
    let error = assert_worktree_removable(repo_path(&repo), str_path(&linked), true).unwrap_err();
    assert!(invalid_input_message(error).contains("git worktree unlock"));

    // And the removal itself refuses even with --force.
    assert!(worktree_remove(repo_path(&repo), str_path(&linked), true).is_err());
}
