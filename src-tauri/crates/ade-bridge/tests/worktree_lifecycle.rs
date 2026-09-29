//! Worktree lifecycle coverage against real temp repos: create (naming, base
//! refs, conflict suffix retry, metadata + authorization), remove (dirty
//! preflight, branch preservation), forget, force-delete and the metadata
//! projection.
//!
//! Helpers mirror `tests/repos.rs`/`tests/git_commands.rs`: a real `git init`
//! with the host config stripped, a real `FsService` root registry and an
//! on-disk `WorktreeMetaStore`. The library-spawned git calls inherit this
//! process's environment, so the hermetic env is installed once and the tests
//! serialize on it (like `ade-git/tests/worktree.rs`).

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, MutexGuard, Once};

use ade_bridge::commands::worktrees::{
    create_worktree_impl, force_delete_preserved_branch_impl, forget_local_impl, list_worktrees,
    persist_sort_order_impl, remove_worktree_impl, update_meta_impl, WorktreesCreateArgs,
};
use ade_fs::FsService;
use ade_store::projects_store::ProjectsStore;
use ade_store::WorktreeMetaStore;
use serde_json::{json, Value};

static COUNTER: AtomicU64 = AtomicU64::new(0);
static ENV_LOCK: Mutex<()> = Mutex::new(());
static ENV_INIT: Once = Once::new();

/// Serialize tests and strip the host git config once, so identity, hooks and
/// `push.autoSetupRemote` cannot leak into the fixtures through the
/// library-spawned git commands.
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

struct TestDir {
    path: PathBuf,
}

impl TestDir {
    fn new(name: &str) -> Self {
        let unique = COUNTER.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "ade-bridge-worktree-it-{name}-{}-{unique}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("create test dir");
        Self {
            path: path.canonicalize().expect("canonicalize test dir"),
        }
    }

    fn file(&self, name: &str) -> PathBuf {
        self.path.join(name)
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

/// git with the user's environment stripped, so host config cannot leak into
/// fixtures.
fn git(dir: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_AUTHOR_NAME", "Ade Test")
        .env("GIT_AUTHOR_EMAIL", "ade-test@example.com")
        .env("GIT_COMMITTER_NAME", "Ade Test")
        .env("GIT_COMMITTER_EMAIL", "ade-test@example.com")
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .expect("run git");
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

fn git_succeeds(dir: &Path, args: &[&str]) -> bool {
    Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

/// Create a git repo named `name` under `dir` with one commit on `main`.
fn init_git_repo(dir: &TestDir, name: &str) -> PathBuf {
    let repo = dir.path.join(name);
    std::fs::create_dir_all(&repo).expect("create repo dir");
    git(&repo, &["-c", "init.defaultBranch=main", "init"]);
    std::fs::write(repo.join("README.md"), "hello\n").expect("write file");
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-m", "init"]);
    repo.canonicalize().expect("canonicalize repo")
}

fn repo_row(repo: &Path) -> Value {
    json!({
        "id": "r1",
        "path": repo.to_str().expect("utf-8 repo path"),
        "displayName": "my-repo",
        "kind": "git",
    })
}

fn settings(dir: &TestDir) -> Value {
    let workspace_dir = dir.path.join("ws");
    json!({
        "workspaceDir": workspace_dir.to_str().expect("utf-8 workspace dir"),
        "nestWorkspaces": true,
        "branchPrefix": "none",
    })
}

fn create_args(repo: &Value, name: &str) -> WorktreesCreateArgs {
    serde_json::from_value(json!({ "repoId": repo["id"], "name": name }))
        .expect("deserialize create args")
}

fn read_worktree(repo: &Value, meta: &WorktreeMetaStore, fs: &FsService, id: &str) -> Value {
    let worktrees = list_worktrees(repo, &[], &meta.items(), fs).expect("list worktrees");
    serde_json::to_value(
        worktrees
            .into_iter()
            .find(|worktree| worktree.id == id)
            .unwrap_or_else(|| panic!("worktree {id} is listed")),
    )
    .expect("serialize worktree")
}

#[test]
fn create_worktree_adds_git_worktree_writes_meta_and_authorizes() {
    let _env = hermetic_env();
    let dir = TestDir::new("create");
    let repo = init_git_repo(&dir, "my-repo");
    let settings = settings(&dir);
    let meta = WorktreeMetaStore::load(dir.file("worktrees.json"));
    let fs = FsService::new();
    let repo_value = repo_row(&repo);
    let args = create_args(&repo_value, "fix-auth");

    let worktree = create_worktree_impl(&repo_value, &settings, &meta, &fs, &args).unwrap();

    assert_eq!(
        worktree.path,
        format!("{}/ws/my-repo/fix-auth", dir.path.display())
    );
    assert_eq!(worktree.branch, "refs/heads/fix-auth");
    assert_eq!(worktree.display_name, "fix-auth");
    assert_eq!(worktree.workspace_status, "in-progress");
    assert_eq!(worktree.id, format!("r1::{}", worktree.path));
    assert!(
        Path::new(&worktree.path).exists(),
        "checkout exists on disk"
    );
    assert!(fs.resolve(&worktree.path).is_ok(), "path was authorized");
    assert_eq!(meta.get(&worktree.id).unwrap()["displayName"], "fix-auth");
    assert_eq!(
        meta.get(&worktree.id).unwrap()["workspaceStatus"],
        "in-progress"
    );
}

#[test]
fn create_worktree_retries_name_suffix_on_conflict() {
    let _env = hermetic_env();
    let dir = TestDir::new("create-conflict");
    let repo = init_git_repo(&dir, "my-repo");
    let settings = settings(&dir);
    let meta = WorktreeMetaStore::load(dir.file("worktrees.json"));
    let fs = FsService::new();
    let repo_value = repo_row(&repo);
    let args = create_args(&repo_value, "fix-auth");

    let first = create_worktree_impl(&repo_value, &settings, &meta, &fs, &args).unwrap();
    let second = create_worktree_impl(&repo_value, &settings, &meta, &fs, &args).unwrap();

    assert_eq!(
        second.path,
        format!("{}/ws/my-repo/fix-auth-2", dir.path.display())
    );
    assert_eq!(second.branch, "refs/heads/fix-auth-2");
    assert_eq!(first.branch, "refs/heads/fix-auth");
    assert!(Path::new(&second.path).exists());
    assert!(
        meta.get(&second.id).is_some(),
        "the retried worktree persisted its metadata"
    );
}

#[test]
fn create_worktree_uses_default_base_ref_when_absent() {
    let _env = hermetic_env();
    let dir = TestDir::new("create-base");
    let repo = init_git_repo(&dir, "my-repo");
    let main_head = git(&repo, &["rev-parse", "HEAD"]);
    let settings = settings(&dir);
    let meta = WorktreeMetaStore::load(dir.file("worktrees.json"));
    let fs = FsService::new();
    let repo_value = repo_row(&repo);
    let args = create_args(&repo_value, "fix-auth");

    let worktree = create_worktree_impl(&repo_value, &settings, &meta, &fs, &args).unwrap();

    assert_eq!(
        worktree.head, main_head,
        "created from the detected default base"
    );
    assert_eq!(worktree.branch, "refs/heads/fix-auth");
}

#[test]
fn remove_worktree_deletes_and_preserves_unmerged_branch() {
    let _env = hermetic_env();
    let dir = TestDir::new("remove-unmerged");
    let repo = init_git_repo(&dir, "my-repo");
    let meta = WorktreeMetaStore::load(dir.file("worktrees.json"));
    let fs = FsService::new();
    let repo_value = repo_row(&repo);
    let worktree = create_worktree_impl(
        &repo_value,
        &settings(&dir),
        &meta,
        &fs,
        &create_args(&repo_value, "fix-auth"),
    )
    .unwrap();

    let worktree_path = PathBuf::from(&worktree.path);
    std::fs::write(worktree_path.join("work.txt"), "wip\n").unwrap();
    git(&worktree_path, &["add", "-A"]);
    git(&worktree_path, &["commit", "-m", "wip"]);

    let result = remove_worktree_impl(&repo_value, &meta, &fs, &worktree.id, false).unwrap();

    assert!(!worktree_path.exists(), "checkout was deleted");
    let registered = ade_git::worktree_list(repo.to_str().unwrap()).unwrap();
    assert!(
        registered.iter().all(|entry| entry.path != worktree.path),
        "registration was removed"
    );
    let preserved = result
        .preserved_branch
        .expect("unmerged branch is preserved");
    assert_eq!(preserved.branch_name, "fix-auth");
    assert!(preserved.head.is_some());
    assert!(meta.get(&worktree.id).is_none(), "metadata was cleaned up");
    assert!(
        git_succeeds(&repo, &["rev-parse", "--verify", "refs/heads/fix-auth"]),
        "the preserved branch still exists"
    );
}

#[test]
fn remove_worktree_dirty_requires_force() {
    let _env = hermetic_env();
    let dir = TestDir::new("remove-dirty");
    let repo = init_git_repo(&dir, "my-repo");
    let meta = WorktreeMetaStore::load(dir.file("worktrees.json"));
    let fs = FsService::new();
    let repo_value = repo_row(&repo);
    let worktree = create_worktree_impl(
        &repo_value,
        &settings(&dir),
        &meta,
        &fs,
        &create_args(&repo_value, "fix-auth"),
    )
    .unwrap();

    let worktree_path = PathBuf::from(&worktree.path);
    std::fs::write(worktree_path.join("dirty.txt"), "dirty\n").unwrap();

    let error = remove_worktree_impl(&repo_value, &meta, &fs, &worktree.id, false).unwrap_err();
    assert!(
        error
            .to_string()
            .contains("Worktree has uncommitted or untracked changes."),
        "unexpected error: {error}"
    );
    assert!(
        worktree_path.exists(),
        "a refused removal keeps the checkout"
    );

    remove_worktree_impl(&repo_value, &meta, &fs, &worktree.id, true).unwrap();
    assert!(
        !worktree_path.exists(),
        "forced removal deleted the checkout"
    );
    assert!(meta.get(&worktree.id).is_none());
}

#[test]
fn forget_local_keeps_directory_and_drops_meta() {
    let _env = hermetic_env();
    let dir = TestDir::new("forget");
    let repo = init_git_repo(&dir, "my-repo");
    let meta = WorktreeMetaStore::load(dir.file("worktrees.json"));
    let fs = FsService::new();
    let repo_value = repo_row(&repo);
    let worktree = create_worktree_impl(
        &repo_value,
        &settings(&dir),
        &meta,
        &fs,
        &create_args(&repo_value, "fix-auth"),
    )
    .unwrap();
    let store = ProjectsStore::load(dir.file("projects.json"));

    forget_local_impl(&store, &meta, &fs, &worktree.id).unwrap();

    assert!(
        Path::new(&worktree.path).exists(),
        "directory survives forget"
    );
    let registered = ade_git::worktree_list(repo.to_str().unwrap()).unwrap();
    assert_eq!(registered.len(), 2, "git registration survives forget");
    assert!(meta.get(&worktree.id).is_none(), "metadata was dropped");
    assert!(
        matches!(
            fs.resolve(&worktree.path),
            Err(ade_fs::FsError::PathAccessDenied)
        ),
        "the forgotten path is no longer authorized"
    );
}

#[test]
fn force_delete_preserved_branch_removes_branch() {
    let _env = hermetic_env();
    let dir = TestDir::new("force-delete-branch");
    let repo = init_git_repo(&dir, "my-repo");
    let meta = WorktreeMetaStore::load(dir.file("worktrees.json"));
    let fs = FsService::new();
    let repo_value = repo_row(&repo);
    let worktree = create_worktree_impl(
        &repo_value,
        &settings(&dir),
        &meta,
        &fs,
        &create_args(&repo_value, "fix-auth"),
    )
    .unwrap();

    let worktree_path = PathBuf::from(&worktree.path);
    std::fs::write(worktree_path.join("work.txt"), "wip\n").unwrap();
    git(&worktree_path, &["add", "-A"]);
    git(&worktree_path, &["commit", "-m", "wip"]);
    let removal = remove_worktree_impl(&repo_value, &meta, &fs, &worktree.id, false).unwrap();
    let preserved = removal.preserved_branch.expect("branch preserved");

    force_delete_preserved_branch_impl(
        &repo_value,
        &preserved.branch_name,
        preserved.head.as_deref().expect("preserved head"),
    )
    .unwrap();

    assert!(
        !git_succeeds(&repo, &["rev-parse", "--verify", "refs/heads/fix-auth"]),
        "the preserved branch was deleted"
    );
}

#[test]
fn update_meta_merges_whitelist_and_projection_reflects() {
    let _env = hermetic_env();
    let dir = TestDir::new("update-meta");
    let repo = init_git_repo(&dir, "my-repo");
    let meta = WorktreeMetaStore::load(dir.file("worktrees.json"));
    let fs = FsService::new();
    let repo_value = repo_row(&repo);
    let worktree = create_worktree_impl(
        &repo_value,
        &settings(&dir),
        &meta,
        &fs,
        &create_args(&repo_value, "fix-auth"),
    )
    .unwrap();

    let updated = update_meta_impl(
        &repo_value,
        &[],
        &meta,
        &fs,
        &worktree.id,
        &json!({ "displayName": "renamed", "isPinned": true, "isUnread": true, "bogus": 1 }),
    )
    .unwrap();

    assert_eq!(updated.display_name, "renamed");
    assert_eq!(updated.display_name_mode, "custom");
    assert!(updated.is_pinned);
    assert!(updated.is_unread);

    let listed = read_worktree(&repo_value, &meta, &fs, &worktree.id);
    assert_eq!(listed["displayName"], "renamed");
    assert_eq!(listed["displayNameMode"], "custom");
    assert_eq!(listed["isPinned"], true);
    assert_eq!(listed["isUnread"], true);
    assert!(listed.get("bogus").is_none(), "unknown keys are ignored");
}

#[test]
fn persist_sort_order_sets_indexes_and_projection_order() {
    let _env = hermetic_env();
    let dir = TestDir::new("sort-order");
    let repo = init_git_repo(&dir, "my-repo");
    let meta = WorktreeMetaStore::load(dir.file("worktrees.json"));
    let fs = FsService::new();
    let repo_value = repo_row(&repo);
    let first = create_worktree_impl(
        &repo_value,
        &settings(&dir),
        &meta,
        &fs,
        &create_args(&repo_value, "alpha"),
    )
    .unwrap();
    let second = create_worktree_impl(
        &repo_value,
        &settings(&dir),
        &meta,
        &fs,
        &create_args(&repo_value, "beta"),
    )
    .unwrap();

    persist_sort_order_impl(&meta, &[second.id.clone(), first.id.clone()]).unwrap();

    assert_eq!(meta.get(&second.id).unwrap()["sortOrder"], 0);
    assert_eq!(meta.get(&first.id).unwrap()["sortOrder"], 1);
    let second_row = read_worktree(&repo_value, &meta, &fs, &second.id);
    let first_row = read_worktree(&repo_value, &meta, &fs, &first.id);
    assert_eq!(second_row["sortOrder"], 0);
    assert_eq!(first_row["sortOrder"], 1);
}
