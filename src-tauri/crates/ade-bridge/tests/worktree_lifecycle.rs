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
    create_worktree_impl, force_delete_preserved_branch_impl, forget_local_impl,
    list_all_worktrees, list_worktrees, persist_sort_order_impl, remove_worktree_impl,
    update_meta_impl, WorktreesCreateArgs,
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
    repo_row_with_id("r1", repo)
}

fn repo_row_with_id(id: &str, repo: &Path) -> Value {
    json!({
        "id": id,
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

/// A symlinked workspace root (`/tmp` → `/private/tmp`, a symlinked HOME) makes
/// git report the real path while the create path is lexical: the meta key and
/// the post-create lookup must use one canonical spelling, or create fails
/// after the checkout was made and a retry keeps creating suffixed directories.
#[cfg(unix)]
#[test]
fn create_worktree_under_symlinked_workspace_root_round_trips() {
    let _env = hermetic_env();
    let dir = TestDir::new("create-symlink-root");
    let repo = init_git_repo(&dir, "my-repo");
    let real_ws = dir.file("real-ws");
    std::fs::create_dir_all(&real_ws).expect("create real workspace root");
    let link_ws = dir.file("link-ws");
    std::os::unix::fs::symlink(&real_ws, &link_ws).expect("create workspace symlink");
    let meta = WorktreeMetaStore::load(dir.file("worktrees.json"));
    let fs = FsService::new();
    let repo_value = repo_row(&repo);
    let settings = json!({
        "workspaceDir": link_ws.to_str().expect("utf-8 link path"),
        "nestWorkspaces": true,
        "branchPrefix": "none",
    });

    let worktree = create_worktree_impl(
        &repo_value,
        &settings,
        &meta,
        &fs,
        &create_args(&repo_value, "fix-auth"),
    )
    .unwrap();

    let expected = real_ws
        .join("my-repo/fix-auth")
        .canonicalize()
        .expect("canonicalize created worktree");
    assert_eq!(
        worktree.path,
        expected.to_str().expect("utf-8 real path"),
        "the projection carries git's real path, not the lexical symlink path"
    );
    assert_eq!(worktree.id, format!("r1::{}", worktree.path));
    assert!(
        meta.get(&worktree.id).is_some(),
        "metadata is keyed by the canonical path"
    );

    let listed = read_worktree(&repo_value, &meta, &fs, &worktree.id);
    assert_eq!(listed["displayName"], "fix-auth", "list merges the meta");

    // A renderer id built from the symlinked spelling resolves to the same row.
    let lexical_id = format!(
        "r1::{}",
        link_ws
            .join("my-repo/fix-auth")
            .to_str()
            .expect("utf-8 lexical path")
    );
    let updated = update_meta_impl(
        &repo_value,
        &[],
        &meta,
        &fs,
        &lexical_id,
        &json!({ "isPinned": true }),
    )
    .unwrap();
    assert!(updated.is_pinned);

    remove_worktree_impl(&repo_value, &meta, &fs, &lexical_id, false).unwrap();
    assert!(!expected.exists(), "checkout was deleted");
    assert!(meta.get(&worktree.id).is_none(), "metadata was cleaned up");
    assert!(meta.items().is_empty(), "no orphaned metadata entries");
}

/// Spec §4.4: an override that hits an existing local branch checks that branch
/// out (`git worktree add <path> <branch>`, no `-b`/`--no-track`) instead of
/// suffix-retrying, and the adopted branch is preserved on removal.
#[test]
fn create_worktree_branch_override_adopts_existing_local_branch() {
    let _env = hermetic_env();
    let dir = TestDir::new("create-adopt-branch");
    let repo = init_git_repo(&dir, "my-repo");
    let main_head = git(&repo, &["rev-parse", "HEAD"]);
    git(&repo, &["branch", "fix-auth"]);
    let meta = WorktreeMetaStore::load(dir.file("worktrees.json"));
    let fs = FsService::new();
    let repo_value = repo_row(&repo);
    let args: WorktreesCreateArgs = serde_json::from_value(json!({
        "repoId": repo_value["id"],
        "name": "fix-auth",
        "branchNameOverride": "fix-auth"
    }))
    .expect("deserialize create args");

    let worktree = create_worktree_impl(&repo_value, &settings(&dir), &meta, &fs, &args).unwrap();

    assert_eq!(worktree.branch, "refs/heads/fix-auth");
    assert_eq!(worktree.head, main_head);
    assert_eq!(
        worktree.path,
        format!("{}/ws/my-repo/fix-auth", dir.path.display()),
        "no -2 suffix directory"
    );
    assert_eq!(
        meta.get(&worktree.id).unwrap()["preserveBranchOnDelete"],
        true,
        "an adopted branch is preserved on removal"
    );
    // Why: checkout-existing skips the new-branch config side effects.
    assert!(!git_succeeds(
        &repo,
        &["config", "--local", "--get", "branch.fix-auth.base"]
    ));
    assert!(!git_succeeds(
        &repo,
        &["config", "--local", "--get", "push.autoSetupRemote"]
    ));

    let removal = remove_worktree_impl(&repo_value, &meta, &fs, &worktree.id, false).unwrap();
    assert!(removal.preserved_branch.is_none(), "no branch was deleted");
    assert!(
        git_succeeds(&repo, &["rev-parse", "--verify", "refs/heads/fix-auth"]),
        "the adopted branch survives removal"
    );
}

/// The override is used verbatim (`resolveCreateBranchName`): the configured
/// branch prefix applies only to generated names.
#[test]
fn create_worktree_branch_override_skips_the_configured_prefix() {
    let _env = hermetic_env();
    let dir = TestDir::new("create-override-prefix");
    let repo = init_git_repo(&dir, "my-repo");
    let meta = WorktreeMetaStore::load(dir.file("worktrees.json"));
    let fs = FsService::new();
    let repo_value = repo_row(&repo);
    let settings = json!({
        "workspaceDir": dir.file("ws").to_str().expect("utf-8 workspace dir"),
        "nestWorkspaces": true,
        "branchPrefix": "custom",
        "branchPrefixCustom": "team",
    });
    let args: WorktreesCreateArgs = serde_json::from_value(json!({
        "repoId": repo_value["id"],
        "name": "fix-auth",
        "branchNameOverride": "review/fix-auth"
    }))
    .expect("deserialize create args");

    let worktree = create_worktree_impl(&repo_value, &settings, &meta, &fs, &args).unwrap();

    assert_eq!(worktree.branch, "refs/heads/review/fix-auth");
}

/// A branch that is already checked out elsewhere cannot be adopted; the
/// suffix retry keeps the override candidate (`fix-auth-2`) for both branch and
/// path.
#[test]
fn create_worktree_branch_override_suffixes_when_branch_is_checked_out() {
    let _env = hermetic_env();
    let dir = TestDir::new("create-override-checked-out");
    let repo = init_git_repo(&dir, "my-repo");
    let first = dir.file("first-checkout");
    git(
        &repo,
        &["worktree", "add", "-b", "fix-auth", first.to_str().unwrap()],
    );
    let meta = WorktreeMetaStore::load(dir.file("worktrees.json"));
    let fs = FsService::new();
    let repo_value = repo_row(&repo);
    let args: WorktreesCreateArgs = serde_json::from_value(json!({
        "repoId": repo_value["id"],
        "name": "fix-auth",
        "branchNameOverride": "fix-auth"
    }))
    .expect("deserialize create args");

    let worktree = create_worktree_impl(&repo_value, &settings(&dir), &meta, &fs, &args).unwrap();

    assert_eq!(worktree.branch, "refs/heads/fix-auth-2");
    assert_eq!(
        worktree.path,
        format!("{}/ws/my-repo/fix-auth-2", dir.path.display())
    );
}

/// Folder workspace ids stay verbatim: canonicalizing them would miss the
/// stored `folderPath` spelling and make updateMeta fail under a symlink.
#[cfg(unix)]
#[test]
fn update_meta_keeps_folder_workspace_ids_verbatim_under_symlinks() {
    let _env = hermetic_env();
    let dir = TestDir::new("update-meta-folder-symlink");
    let real_folder = dir.file("real-folder");
    std::fs::create_dir_all(real_folder.join("child")).expect("create real folder");
    let link_folder = dir.file("link-folder");
    std::os::unix::fs::symlink(&real_folder, &link_folder).expect("create folder symlink");
    let fs = FsService::new();
    let meta = WorktreeMetaStore::load(dir.file("worktrees.json"));
    let repo_value = json!({
        "id": "f1",
        "path": real_folder.to_str().expect("utf-8 folder path"),
        "displayName": "Folder",
        "kind": "folder",
        "projectGroupId": "g1",
    });
    let workspace_path = link_folder.join("child");
    let workspace_path = workspace_path.to_str().expect("utf-8 workspace path");
    let workspace = json!({
        "id": "w1",
        "projectGroupId": "g1",
        "folderPath": workspace_path,
        "name": "Child",
        "lastActivityAt": 1,
    });
    let raw_id = format!("f1::{workspace_path}");

    let updated = update_meta_impl(
        &repo_value,
        std::slice::from_ref(&workspace),
        &meta,
        &fs,
        &raw_id,
        &json!({ "isPinned": true }),
    )
    .unwrap();

    // Why: the folder projection does not merge the meta store yet (record
    // §3.3), so the id resolution and the persisted key are what this guards.
    assert_eq!(updated.id, raw_id);
    assert_eq!(
        meta.get(&raw_id).unwrap()["isPinned"],
        true,
        "metadata keeps the verbatim folder-workspace id"
    );
}

/// `canCheckoutExistingLocalBranch` only adopts a branch that points at the
/// base ref; a diverged local branch is a conflict the suffix loop retries.
#[test]
fn create_worktree_branch_override_suffixes_when_local_branch_diverged_from_base() {
    let _env = hermetic_env();
    let dir = TestDir::new("create-override-diverged");
    let repo = init_git_repo(&dir, "my-repo");
    git(&repo, &["checkout", "-b", "fix-auth"]);
    std::fs::write(repo.join("wip.txt"), "wip\n").expect("write file");
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-m", "wip"]);
    git(&repo, &["checkout", "main"]);
    let meta = WorktreeMetaStore::load(dir.file("worktrees.json"));
    let fs = FsService::new();
    let repo_value = repo_row(&repo);
    let args: WorktreesCreateArgs = serde_json::from_value(json!({
        "repoId": repo_value["id"],
        "name": "fix-auth",
        "branchNameOverride": "fix-auth"
    }))
    .expect("deserialize create args");

    let worktree = create_worktree_impl(&repo_value, &settings(&dir), &meta, &fs, &args).unwrap();

    assert_eq!(worktree.branch, "refs/heads/fix-auth-2");
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
    assert_eq!(updated.display_name_mode, "fixed");
    assert!(updated.is_pinned);
    assert!(updated.is_unread);

    let listed = read_worktree(&repo_value, &meta, &fs, &worktree.id);
    assert_eq!(listed["displayName"], "renamed");
    assert_eq!(listed["displayNameMode"], "fixed");
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

#[test]
fn create_worktree_name_only_pins_the_user_label() {
    let _env = hermetic_env();
    let dir = TestDir::new("create-user-label");
    let repo = init_git_repo(&dir, "my-repo");
    let meta = WorktreeMetaStore::load(dir.file("worktrees.json"));
    let fs = FsService::new();
    let repo_value = repo_row(&repo);
    // Why: a name-only legacy request is a user label; the raw name survives as
    // the pinned display name while the branch/path use the sanitized form.
    let args = create_args(&repo_value, "Fix Auth");

    let worktree = create_worktree_impl(&repo_value, &settings(&dir), &meta, &fs, &args).unwrap();

    assert_eq!(worktree.branch, "refs/heads/Fix-Auth");
    assert_eq!(worktree.display_name, "Fix Auth");
    assert_eq!(worktree.display_name_mode, "fixed");
    let stored = meta.get(&worktree.id).unwrap();
    assert_eq!(stored["displayName"], "Fix Auth");
    assert_eq!(stored["displayNameIsPinned"], true);
}

#[test]
fn create_worktree_generated_display_name_equal_to_branch_stays_automatic() {
    let _env = hermetic_env();
    let dir = TestDir::new("create-generated-label");
    let repo = init_git_repo(&dir, "my-repo");
    let meta = WorktreeMetaStore::load(dir.file("worktrees.json"));
    let fs = FsService::new();
    let repo_value = repo_row(&repo);
    let args: WorktreesCreateArgs = serde_json::from_value(json!({
        "repoId": repo_value["id"],
        "name": "fix-auth",
        "displayName": "fix-auth"
    }))
    .expect("deserialize create args");

    let worktree = create_worktree_impl(&repo_value, &settings(&dir), &meta, &fs, &args).unwrap();

    assert_eq!(worktree.display_name, "fix-auth");
    assert_eq!(worktree.display_name_mode, "automatic");
    let stored = meta.get(&worktree.id).unwrap();
    assert!(stored.get("displayName").is_none());
    assert!(stored.get("displayNameIsPinned").is_none());
}

#[test]
fn update_meta_unpinned_display_name_falls_back_to_automatic() {
    let _env = hermetic_env();
    let dir = TestDir::new("update-meta-unpinned");
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
        &json!({ "displayName": "renamed", "displayNameIsPinned": false }),
    )
    .unwrap();

    assert_eq!(
        updated.display_name, "fix-auth",
        "an unpinned label falls back to the branch short name"
    );
    assert_eq!(updated.display_name_mode, "automatic");
}

#[test]
fn update_meta_legacy_cli_provenance_is_fixed() {
    let _env = hermetic_env();
    let dir = TestDir::new("update-meta-cli");
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

    // A CLI-created label predates `displayNameIsPinned` and is still explicit.
    let updated = update_meta_impl(
        &repo_value,
        &[],
        &meta,
        &fs,
        &worktree.id,
        &json!({ "displayName": "cli-label", "cliProvenance": { "kind": "created-by-cli" } }),
    )
    .unwrap();

    assert_eq!(updated.display_name, "cli-label");
    assert_eq!(updated.display_name_mode, "fixed");
}

#[test]
fn update_meta_missing_worktree_errors_without_persisting() {
    let _env = hermetic_env();
    let dir = TestDir::new("update-meta-missing");
    let repo = init_git_repo(&dir, "my-repo");
    let meta = WorktreeMetaStore::load(dir.file("worktrees.json"));
    let fs = FsService::new();
    let repo_value = repo_row(&repo);
    let stale_id = format!("r1::{}", dir.path.join("ws/my-repo/gone").display());

    let error = update_meta_impl(
        &repo_value,
        &[],
        &meta,
        &fs,
        &stale_id,
        &json!({ "isPinned": true }),
    )
    .unwrap_err();

    assert!(error.to_string().contains("Worktree not found"), "{error}");
    assert!(
        meta.get(&stale_id).is_none(),
        "a stale id must not persist a ghost meta entry"
    );
    assert!(
        !dir.file("worktrees.json").exists(),
        "no store write happened"
    );
}

#[test]
fn git_failure_degrades_to_empty_list_for_that_repo() {
    let _env = hermetic_env();
    let dir = TestDir::new("list-degrade");
    let broken = init_git_repo(&dir, "broken-repo");
    std::fs::remove_dir_all(broken.join(".git")).expect("break the repo");
    let healthy = init_git_repo(&dir, "healthy-repo");
    let fs = FsService::new();
    let meta = WorktreeMetaStore::load(dir.file("worktrees.json"));
    let broken_row = repo_row_with_id("r-broken", &broken);
    let healthy_row = repo_row_with_id("r-healthy", &healthy);

    let listed = list_worktrees(&broken_row, &[], &meta.items(), &fs).unwrap();
    assert!(
        listed.is_empty(),
        "a broken git repo degrades to an empty list"
    );

    let all = list_all_worktrees(&[broken_row, healthy_row], &[], &meta.items(), &fs).unwrap();
    assert_eq!(all.len(), 1, "the healthy repo is still listed");
    assert_eq!(all[0].repo_id, "r-healthy");
}
