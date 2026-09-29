//! Integration coverage for the repos registry and the minimal worktree
//! projection: real tempdirs, a real `git init` + commit, a real `FsService`
//! authorization registry, and the on-disk `ProjectsStore`.

use std::path::{Path, PathBuf};
use std::process::Command;

use ade_bridge::commands::repos::{
    add_repo, base_ref_default, create_repo, default_create_project_parent, remove_repo,
    reorder_repos_for_host, resolve_add_path, search_base_ref_details, search_base_refs,
    update_repo, ReposCreateArgs,
};
use ade_bridge::commands::worktrees::{list_all_worktrees, list_worktrees};
use ade_core::models::repo::RepoKind;
use ade_fs::FsService;
use ade_store::projects_store::ProjectsStore;
use serde_json::{json, Map, Value};

struct TestDir {
    path: PathBuf,
}

impl TestDir {
    fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "ade-bridge-repos-it-{name}-{}",
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

    fn dir(&self, name: &str) -> PathBuf {
        let path = self.path.join(name);
        std::fs::create_dir_all(&path).expect("create dir");
        path
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

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
        .output()
        .expect("run git");
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

/// Create a git repo named `name` under `dir` with one commit on `main`.
fn init_git_repo(dir: &Path, name: &str) -> PathBuf {
    let repo = dir.join(name);
    std::fs::create_dir_all(&repo).expect("create repo dir");
    git(&repo, &["-c", "init.defaultBranch=main", "init"]);
    std::fs::write(repo.join("README.md"), "hello\n").expect("write file");
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-m", "init"]);
    repo.canonicalize().expect("canonicalize repo")
}

fn store(dir: &TestDir) -> ProjectsStore {
    ProjectsStore::load(dir.file("projects.json"))
}

fn repo_id(repo: &Value) -> &str {
    repo["id"].as_str().expect("repo id")
}

#[test]
fn git_repo_add_projects_main_worktree() {
    let dir = TestDir::new("git-main");
    let repo_path = init_git_repo(&dir.path, "demo");
    let head = git(&repo_path, &["rev-parse", "HEAD"]);
    let fs = FsService::new();
    let mut projects = store(&dir);

    let resolved = resolve_add_path(repo_path.to_str().unwrap(), RepoKind::Git).unwrap();
    assert_eq!(resolved, repo_path.to_str().unwrap());

    let outcome = add_repo(&mut projects, &fs, &resolved, RepoKind::Git, None, 1_000).unwrap();
    assert!(!outcome.already_existed);
    let repo = outcome.repo;
    assert_eq!(repo["path"], resolved);
    assert_eq!(repo["displayName"], "demo");
    assert_eq!(repo["badgeColor"], "#737373");
    assert_eq!(repo["kind"], "git");
    assert_eq!(repo["addedAt"], 1_000);
    assert_eq!(repo["externalWorktreeVisibilityLegacy"], false);
    assert_eq!(projects.repos().len(), 1);
    assert!(
        fs.resolve(&resolved).is_ok(),
        "a successful add authorizes its root"
    );

    let worktrees = list_worktrees(&repo, &[], &Map::new(), &fs).expect("git worktree list");
    assert_eq!(worktrees.len(), 1, "fresh repo has one worktree");
    let main = &worktrees[0];
    assert_eq!(main.id, format!("{}::{}", repo_id(&repo), resolved));
    assert_eq!(main.repo_id, repo_id(&repo));
    assert_eq!(main.path, resolved);
    assert_eq!(main.display_name, "main");
    assert_eq!(main.display_name_mode, "automatic");
    assert_eq!(main.head, head);
    assert_eq!(main.branch, "refs/heads/main");
    assert!(main.is_main_worktree);
    assert!(!main.is_bare);
    assert_eq!(main.comment, "");
    assert!(main.linked_issue.is_none());
    assert!(main.linked_pr.is_none());
    assert!(main.linked_linear_issue.is_none());
    assert!(!main.is_archived);
    assert!(!main.is_unread);
    assert!(!main.is_pinned);
    assert_eq!(main.sort_order, 0);
    assert_eq!(main.last_activity_at, 0);
    assert_eq!(main.workspace_status, "in-progress");
}

#[test]
fn linked_worktrees_follow_the_main_entry() {
    let dir = TestDir::new("git-linked");
    let repo_path = init_git_repo(&dir.path, "demo");
    let linked = dir.path.join("demo-feature");
    git(
        &repo_path,
        &[
            "worktree",
            "add",
            "-b",
            "feature/login",
            linked.to_str().unwrap(),
        ],
    );
    let fs = FsService::new();
    let mut projects = store(&dir);
    let outcome = add_repo(
        &mut projects,
        &fs,
        repo_path.to_str().unwrap(),
        RepoKind::Git,
        None,
        1,
    )
    .unwrap();

    let worktrees = list_worktrees(&outcome.repo, &[], &Map::new(), &fs).unwrap();
    assert_eq!(worktrees.len(), 2);
    assert!(worktrees[0].is_main_worktree);
    assert_eq!(worktrees[0].display_name, "main");
    assert!(!worktrees[1].is_main_worktree);
    assert_eq!(worktrees[1].branch, "refs/heads/feature/login");
    assert_eq!(worktrees[1].display_name, "feature/login");
}

#[test]
fn list_worktrees_authorizes_linked_worktree_paths() {
    let dir = TestDir::new("git-authorize-linked");
    let repo_path = init_git_repo(&dir.path, "demo");
    let linked = dir.path.join("demo-feature");
    git(
        &repo_path,
        &[
            "worktree",
            "add",
            "-b",
            "feature/login",
            linked.to_str().unwrap(),
        ],
    );
    let fs = FsService::new();
    let mut projects = store(&dir);
    let outcome = add_repo(
        &mut projects,
        &fs,
        repo_path.to_str().unwrap(),
        RepoKind::Git,
        None,
        1,
    )
    .unwrap();

    // Adding the repo grants only the repo root; the linked worktree is a
    // sibling directory and stays unreachable until a listing authorizes it.
    assert!(matches!(
        fs.resolve(linked.to_str().unwrap()),
        Err(ade_fs::FsError::PathAccessDenied)
    ));

    let worktrees = list_worktrees(&outcome.repo, &[], &Map::new(), &fs).unwrap();
    assert!(
        worktrees
            .iter()
            .any(|worktree| worktree.path == linked.to_str().unwrap()),
        "the linked worktree is listed"
    );

    assert!(
        fs.resolve(linked.to_str().unwrap()).is_ok(),
        "listing authorizes every returned worktree path"
    );
    let unrelated = dir.dir("unrelated");
    assert!(matches!(
        fs.resolve(unrelated.to_str().unwrap()),
        Err(ade_fs::FsError::PathAccessDenied)
    ));
}

#[test]
fn folder_kind_add_projects_its_main_workspace() {
    let dir = TestDir::new("folder-main");
    let folder = dir.dir("notes");
    let fs = FsService::new();
    let mut projects = store(&dir);

    let resolved = resolve_add_path(folder.to_str().unwrap(), RepoKind::Folder).unwrap();
    assert_eq!(resolved, folder.to_str().unwrap());

    let outcome = add_repo(
        &mut projects,
        &fs,
        &resolved,
        RepoKind::Folder,
        Some("  My Folder  "),
        2,
    )
    .unwrap();
    let repo = outcome.repo;
    assert_eq!(repo["displayName"], "My Folder");
    assert_eq!(repo["kind"], "folder");
    assert!(repo.get("externalWorktreeVisibilityLegacy").is_none());
    assert!(fs.resolve(&resolved).is_ok());

    let worktrees = list_worktrees(&repo, &[], &Map::new(), &fs).unwrap();
    assert_eq!(worktrees.len(), 1);
    let main = &worktrees[0];
    assert_eq!(main.id, format!("{}::{}", repo_id(&repo), resolved));
    assert_eq!(main.path, resolved);
    assert_eq!(main.display_name, "My Folder");
    assert_eq!(main.head, "");
    assert_eq!(main.branch, "");
    assert!(main.is_main_worktree);
    assert!(!main.is_bare);
    assert_eq!(main.workspace_status, "in-progress");
}

#[test]
fn folder_workspaces_append_after_main_by_last_activity() {
    let dir = TestDir::new("folder-workspaces");
    let folder = dir.dir("notes");
    let fs = FsService::new();
    let mut projects = store(&dir);
    let outcome = add_repo(
        &mut projects,
        &fs,
        folder.to_str().unwrap(),
        RepoKind::Folder,
        None,
        2,
    )
    .unwrap();
    let mut repo = outcome.repo;
    repo["projectGroupId"] = json!("g1");
    let workspaces = vec![
        json!({
            "id": "w1",
            "projectGroupId": "g1",
            "name": "Older",
            "folderPath": dir.dir("notes/older").to_str().unwrap(),
            "lastActivityAt": 5
        }),
        json!({
            "id": "w2",
            "projectGroupId": "g1",
            "name": "Newer",
            "folderPath": dir.dir("notes/newer").to_str().unwrap(),
            "lastActivityAt": 10,
            "isPinned": true
        }),
    ];

    let worktrees = list_worktrees(&repo, &workspaces, &Map::new(), &fs).unwrap();
    assert_eq!(
        worktrees
            .iter()
            .map(|worktree| worktree.display_name.as_str())
            .collect::<Vec<_>>(),
        vec!["notes", "Newer", "Older"]
    );
    assert!(worktrees[0].is_main_worktree);
    assert!(!worktrees[1].is_main_worktree);
    assert!(worktrees[1].is_pinned);
    assert_eq!(worktrees[1].head, "");
    assert_eq!(worktrees[1].branch, "");
}

#[test]
fn folder_workspaces_are_scoped_to_their_repo_group() {
    let dir = TestDir::new("folder-scope");
    let first_folder = dir.dir("first");
    let second_folder = dir.dir("second");
    let fs = FsService::new();
    let mut projects = store(&dir);
    let mut first = add_repo(
        &mut projects,
        &fs,
        first_folder.to_str().unwrap(),
        RepoKind::Folder,
        None,
        1,
    )
    .unwrap()
    .repo;
    let mut second = add_repo(
        &mut projects,
        &fs,
        second_folder.to_str().unwrap(),
        RepoKind::Folder,
        None,
        2,
    )
    .unwrap()
    .repo;
    first["projectGroupId"] = json!("g1");
    second["projectGroupId"] = json!("g2");
    let first_id = repo_id(&first).to_string();
    let second_id = repo_id(&second).to_string();
    projects
        .mutate_repos(|repos| {
            for repo in repos.iter_mut() {
                if repo["id"] == first_id.as_str() {
                    repo["projectGroupId"] = json!("g1");
                } else if repo["id"] == second_id.as_str() {
                    repo["projectGroupId"] = json!("g2");
                }
            }
        })
        .unwrap();

    let workspaces = vec![
        json!({
            "id": "w1",
            "projectGroupId": "g1",
            "name": "First Workspace",
            "folderPath": dir.dir("first/one").to_str().unwrap(),
            "lastActivityAt": 1
        }),
        json!({
            "id": "w2",
            "projectGroupId": "g2",
            "name": "Second Workspace",
            "folderPath": dir.dir("second/two").to_str().unwrap(),
            "lastActivityAt": 2
        }),
        json!({
            "id": "w3",
            "projectGroupId": null,
            "name": "Orphan",
            "folderPath": dir.dir("orphan").to_str().unwrap(),
            "lastActivityAt": 3
        }),
    ];

    let first_worktrees = list_worktrees(&first, &workspaces, &Map::new(), &fs).unwrap();
    assert_eq!(
        first_worktrees
            .iter()
            .map(|worktree| worktree.path.as_str())
            .collect::<Vec<_>>(),
        vec![first_folder.to_str().unwrap(), dir.dir("first/one").to_str().unwrap()]
    );

    let second_worktrees = list_worktrees(&second, &workspaces, &Map::new(), &fs).unwrap();
    assert_eq!(
        second_worktrees
            .iter()
            .map(|worktree| worktree.path.as_str())
            .collect::<Vec<_>>(),
        vec![
            second_folder.to_str().unwrap(),
            dir.dir("second/two").to_str().unwrap()
        ]
    );

    let all = list_all_worktrees(&projects.repos(), &workspaces, &Map::new(), &fs).unwrap();
    assert_eq!(
        all.iter().map(|worktree| worktree.id.as_str()).collect::<Vec<_>>(),
        vec![
            format!("{}::{}", repo_id(&first), first_folder.to_str().unwrap()),
            format!("{}::{}", repo_id(&first), dir.dir("first/one").to_str().unwrap()),
            format!("{}::{}", repo_id(&second), second_folder.to_str().unwrap()),
            format!("{}::{}", repo_id(&second), dir.dir("second/two").to_str().unwrap()),
        ]
    );
}

#[test]
fn ungrouped_folder_repo_projects_only_its_root() {
    let dir = TestDir::new("folder-ungrouped");
    let folder = dir.dir("notes");
    let fs = FsService::new();
    let mut projects = store(&dir);
    let repo = add_repo(
        &mut projects,
        &fs,
        folder.to_str().unwrap(),
        RepoKind::Folder,
        None,
        1,
    )
    .unwrap()
    .repo;
    let workspaces = vec![
        json!({
            "id": "w1",
            "projectGroupId": null,
            "name": "Null Group",
            "folderPath": dir.dir("notes/one").to_str().unwrap(),
            "lastActivityAt": 1
        }),
        json!({
            "id": "w2",
            "projectGroupId": "g1",
            "name": "Other Group",
            "folderPath": dir.dir("notes/two").to_str().unwrap(),
            "lastActivityAt": 2
        }),
    ];

    let worktrees = list_worktrees(&repo, &workspaces, &Map::new(), &fs).unwrap();
    assert_eq!(worktrees.len(), 1);
    assert_eq!(worktrees[0].id, format!("{}::{}", repo_id(&repo), folder.to_str().unwrap()));
    assert!(worktrees[0].is_main_worktree);
}

#[test]
fn folder_root_spelling_does_not_duplicate_the_root_row() {
    let dir = TestDir::new("folder-root-spelling");
    let folder = dir.dir("notes");
    let fs = FsService::new();
    let mut projects = store(&dir);
    let with_slash = format!("{}/", folder.to_str().unwrap());
    let mut repo = add_repo(
        &mut projects,
        &fs,
        &with_slash,
        RepoKind::Folder,
        None,
        1,
    )
    .unwrap()
    .repo;
    repo["projectGroupId"] = json!("g1");
    let workspaces = vec![json!({
        "id": "w1",
        "projectGroupId": "g1",
        "name": "Root Again",
        "folderPath": folder.to_str().unwrap(),
        "lastActivityAt": 1
    })];

    let worktrees = list_worktrees(&repo, &workspaces, &Map::new(), &fs).unwrap();
    assert_eq!(worktrees.len(), 1);
    assert_eq!(worktrees[0].display_name, "notes");
    assert!(worktrees[0].is_main_worktree);
}

#[test]
fn duplicate_adds_are_idempotent_across_path_spellings() {
    let dir = TestDir::new("dedup");
    let repo_path = init_git_repo(&dir.path, "demo");
    let subdir = dir.dir("demo/src");
    let folder = dir.dir("plain-folder");
    let fs = FsService::new();
    let mut projects = store(&dir);

    let outcome = add_repo(
        &mut projects,
        &fs,
        repo_path.to_str().unwrap(),
        RepoKind::Git,
        None,
        1,
    )
    .unwrap();
    assert!(!outcome.already_existed);
    let first_id = repo_id(&outcome.repo).to_string();

    // Trailing separator: `rev-parse --show-toplevel` is canonical either way.
    let with_slash = format!("{}/", repo_path.to_str().unwrap());
    let resolved_slash = resolve_add_path(&with_slash, RepoKind::Git).unwrap();
    let duplicate = add_repo(&mut projects, &fs, &resolved_slash, RepoKind::Git, None, 2).unwrap();
    assert!(duplicate.already_existed);
    assert_eq!(repo_id(&duplicate.repo), first_id);

    // A subdirectory resolves to the same toplevel root.
    let resolved_subdir = resolve_add_path(subdir.to_str().unwrap(), RepoKind::Git).unwrap();
    assert_eq!(resolved_subdir, repo_path.to_str().unwrap());
    let duplicate = add_repo(&mut projects, &fs, &resolved_subdir, RepoKind::Git, None, 3).unwrap();
    assert!(duplicate.already_existed);
    assert_eq!(repo_id(&duplicate.repo), first_id);

    // Folder kinds dedup on the normalized spelling (`/folder/` vs `/folder`).
    let folder_with_slash = format!("{}/", folder.to_str().unwrap());
    let added = add_repo(
        &mut projects,
        &fs,
        &folder_with_slash,
        RepoKind::Folder,
        None,
        4,
    )
    .unwrap();
    assert!(!added.already_existed);
    let duplicate = add_repo(
        &mut projects,
        &fs,
        folder.to_str().unwrap(),
        RepoKind::Folder,
        None,
        5,
    )
    .unwrap();
    assert!(duplicate.already_existed);
    assert_eq!(repo_id(&duplicate.repo), repo_id(&added.repo));

    assert_eq!(projects.repos().len(), 2, "no duplicate rows persisted");
}

#[test]
fn non_git_paths_answer_the_contract_error() {
    let dir = TestDir::new("not-git");
    let plain = dir.dir("plain");
    let error = resolve_add_path(plain.to_str().unwrap(), RepoKind::Git).unwrap_err();
    assert_eq!(
        error.to_string(),
        format!("Not a valid git repository: {}", plain.to_str().unwrap())
    );
}

#[test]
fn remove_revokes_the_root_without_cascading() {
    let dir = TestDir::new("remove");
    let folder = dir.dir("plain-folder");
    let fs = FsService::new();
    let mut projects = store(&dir);
    let outcome = add_repo(
        &mut projects,
        &fs,
        folder.to_str().unwrap(),
        RepoKind::Folder,
        None,
        1,
    )
    .unwrap();
    let id = repo_id(&outcome.repo).to_string();
    projects
        .mutate_groups(|groups| groups.push(json!({ "id": "g1", "name": "Group" })))
        .unwrap();
    projects
        .mutate_folder_workspaces(|workspaces| {
            workspaces.push(json!({ "id": "w1", "projectGroupId": "g1" }))
        })
        .unwrap();

    let removed = remove_repo(&mut projects, &fs, &id).unwrap().expect("repo");
    assert_eq!(repo_id(&removed), id);
    assert!(projects.repos().is_empty());
    assert!(matches!(
        fs.resolve(folder.to_str().unwrap()),
        Err(ade_fs::FsError::PathAccessDenied)
    ));
    // No cascade: the group and folder workspace survive (spec §5.2).
    assert_eq!(projects.project_groups().len(), 1);
    assert_eq!(projects.folder_workspaces().len(), 1);

    assert!(remove_repo(&mut projects, &fs, &id).unwrap().is_none());
}

#[test]
fn update_and_reorder_persist_across_reload() {
    let dir = TestDir::new("persist");
    let first = dir.dir("first");
    let second = dir.dir("second");
    let fs = FsService::new();
    let mut projects = store(&dir);
    let first = add_repo(
        &mut projects,
        &fs,
        first.to_str().unwrap(),
        RepoKind::Folder,
        None,
        1,
    )
    .unwrap()
    .repo;
    let second = add_repo(
        &mut projects,
        &fs,
        second.to_str().unwrap(),
        RepoKind::Folder,
        None,
        2,
    )
    .unwrap()
    .repo;

    let updated = update_repo(
        &mut projects,
        repo_id(&first),
        &json!({
            "displayName": "Renamed",
            "badgeColor": "#fff",
            "connectionId": "ssh:ignored",
            "projectGroupId": null
        }),
    )
    .unwrap()
    .expect("repo exists");
    assert_eq!(updated["displayName"], "Renamed");
    assert_eq!(updated["badgeColor"], "#ffffff");
    assert!(updated.get("connectionId").is_none());
    assert_eq!(updated["projectGroupId"], Value::Null);

    assert!(reorder_repos_for_host(
        &mut projects,
        &[repo_id(&second).to_string(), repo_id(&first).to_string()],
        "local"
    )
    .unwrap());
    assert!(!reorder_repos_for_host(
        &mut projects,
        &[repo_id(&second).to_string(), repo_id(&first).to_string()],
        "ssh:box"
    )
    .unwrap());

    let reloaded = store(&dir);
    let reloaded_first = reloaded
        .repos()
        .into_iter()
        .find(|repo| repo_id(repo) == repo_id(&first))
        .expect("first repo persisted");
    assert_eq!(reloaded_first["displayName"], "Renamed");
    assert_eq!(reloaded_first["badgeColor"], "#ffffff");
    assert_eq!(reloaded_first["projectGroupOrder"], 1);
    let reloaded_second = reloaded
        .repos()
        .into_iter()
        .find(|repo| repo_id(repo) == repo_id(&second))
        .expect("second repo persisted");
    assert_eq!(reloaded_second["projectGroupOrder"], 0);
}

#[test]
fn worktrees_list_all_merges_every_repo() {
    let dir = TestDir::new("list-all");
    let repo_path = init_git_repo(&dir.path, "demo");
    let folder = dir.dir("notes");
    let fs = FsService::new();
    let mut projects = store(&dir);
    let git_repo = add_repo(
        &mut projects,
        &fs,
        repo_path.to_str().unwrap(),
        RepoKind::Git,
        None,
        1,
    )
    .unwrap()
    .repo;
    let folder_repo = add_repo(
        &mut projects,
        &fs,
        folder.to_str().unwrap(),
        RepoKind::Folder,
        None,
        2,
    )
    .unwrap()
    .repo;

    let worktrees = list_all_worktrees(
        &projects.repos(),
        &projects.folder_workspaces(),
        &Map::new(),
        &fs,
    )
    .unwrap();
    assert_eq!(worktrees.len(), 2);
    assert_eq!(
        worktrees
            .iter()
            .map(|worktree| worktree.id.as_str())
            .collect::<Vec<_>>(),
        vec![
            format!("{}::{}", repo_id(&git_repo), repo_path.to_str().unwrap()),
            format!("{}::{}", repo_id(&folder_repo), folder.to_str().unwrap()),
        ]
    );
}

#[test]
fn default_parent_survives_the_generated_settings_default() {
    let home = "/Users/tester";
    let settings = ade_core::defaults::settings_defaults(home);
    assert_eq!(
        default_create_project_parent(&settings, home),
        "/Users/tester/orcinus/projects"
    );
}

fn create_args(parent: &Path, name: &str, kind: Option<RepoKind>) -> ReposCreateArgs {
    ReposCreateArgs {
        parent_path: parent.to_str().expect("utf8 parent").to_string(),
        name: name.to_string(),
        kind,
    }
}

/// `create_repo` shells out to git in-process, so the tests that control the
/// identity environment process-wide must not overlap each other.
static GIT_IDENTITY_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn git_identity_env_lock() -> std::sync::MutexGuard<'static, ()> {
    GIT_IDENTITY_ENV_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Applies environment changes and restores the previous values on drop, so a
/// test can never leak a fake HOME or identity into the host config.
struct EnvGuard {
    saved: Vec<(&'static str, Option<std::ffi::OsString>)>,
}

impl EnvGuard {
    fn apply(changes: &[(&'static str, Option<&str>)]) -> Self {
        let saved = changes
            .iter()
            .map(|(key, _)| (*key, std::env::var_os(key)))
            .collect();
        for (key, value) in changes {
            match value {
                Some(value) => std::env::set_var(key, value),
                None => std::env::remove_var(key),
            }
        }
        Self { saved }
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        for (key, value) in &self.saved {
            match value {
                Some(value) => std::env::set_var(key, value),
                None => std::env::remove_var(key),
            }
        }
    }
}

/// A deterministic `git commit` identity, independent of the host config.
fn with_test_git_identity() -> EnvGuard {
    EnvGuard::apply(&[
        ("GIT_AUTHOR_NAME", Some("Ade Test")),
        ("GIT_AUTHOR_EMAIL", Some("ade-test@example.com")),
        ("GIT_COMMITTER_NAME", Some("Ade Test")),
        ("GIT_COMMITTER_EMAIL", Some("ade-test@example.com")),
    ])
}

/// No identity anywhere: global/system config is bypassed and
/// `user.useConfigOnly` stops git from inventing one from the OS account, so
/// `git commit` fails with the oracle's "Please tell me who you are." error.
fn without_git_identity(home: &Path) -> EnvGuard {
    let xdg = home.join(".config");
    EnvGuard::apply(&[
        ("GIT_CONFIG_GLOBAL", Some("/dev/null")),
        ("GIT_CONFIG_NOSYSTEM", Some("1")),
        ("GIT_CONFIG_COUNT", Some("1")),
        ("GIT_CONFIG_KEY_0", Some("user.useConfigOnly")),
        ("GIT_CONFIG_VALUE_0", Some("true")),
        ("HOME", Some(home.to_str().expect("utf8 home"))),
        ("XDG_CONFIG_HOME", Some(xdg.to_str().expect("utf8 xdg"))),
        ("GIT_AUTHOR_NAME", None),
        ("GIT_AUTHOR_EMAIL", None),
        ("GIT_COMMITTER_NAME", None),
        ("GIT_COMMITTER_EMAIL", None),
        ("EMAIL", None),
    ])
}

const IDENTITY_SETUP_HINT: &str = "Git author identity is not configured. Run `git config --global user.name \"Your Name\"` and `git config --global user.email \"you@example.com\"`, then try again.";

#[test]
fn create_git_repo_makes_directory_initial_commit_and_registers() {
    let _lock = git_identity_env_lock();
    let _env = with_test_git_identity();
    let dir = TestDir::new("create-git");
    let parents = dir.dir("parents");
    let fs = FsService::new();
    let mut projects = store(&dir);

    let value = create_repo(
        &mut projects,
        &fs,
        &create_args(&parents, "demo", Some(RepoKind::Git)),
        5_000,
    );

    let repo = value.get("repo").expect("created repo");
    let expected = parents.join("demo");
    assert_eq!(repo["path"], expected.to_str().unwrap());
    assert_eq!(repo["displayName"], "demo");
    assert_eq!(repo["kind"], "git");
    assert_eq!(repo["addedAt"], 5_000);
    assert_eq!(projects.repos().len(), 1);
    assert_eq!(
        git(&expected, &["log", "-1", "--format=%s"]),
        "Initial commit"
    );
    assert!(
        fs.resolve(expected.to_str().unwrap()).is_ok(),
        "a created repo authorizes its root"
    );
}

#[test]
fn create_repo_rejects_name_with_slash_and_empty_name() {
    let _lock = git_identity_env_lock();
    let dir = TestDir::new("create-name-validation");
    let fs = FsService::new();
    let mut projects = store(&dir);

    assert_eq!(
        create_repo(
            &mut projects,
            &fs,
            &create_args(&dir.path, "a/b", Some(RepoKind::Git)),
            1
        ),
        json!({ "error": "Name cannot contain slashes or be \".\" / \"..\"" })
    );
    assert_eq!(
        create_repo(&mut projects, &fs, &create_args(&dir.path, "a\\b", None), 1),
        json!({ "error": "Name cannot contain slashes or be \".\" / \"..\"" })
    );
    assert_eq!(
        create_repo(&mut projects, &fs, &create_args(&dir.path, "..", None), 1),
        json!({ "error": "Name cannot contain slashes or be \".\" / \"..\"" })
    );
    assert_eq!(
        create_repo(&mut projects, &fs, &create_args(&dir.path, "   ", None), 1),
        json!({ "error": "Name cannot be empty" })
    );
    assert_eq!(
        create_repo(
            &mut projects,
            &fs,
            &create_args(Path::new("relative"), "demo", None),
            1
        ),
        json!({ "error": "Parent directory must be an absolute path" })
    );
    assert!(projects.repos().is_empty());
    assert!(!dir.path.join("demo").exists());
}

#[test]
fn create_repo_rejects_non_empty_existing_directory() {
    let _lock = git_identity_env_lock();
    let _env = with_test_git_identity();
    let dir = TestDir::new("create-non-empty");
    let parents = dir.dir("parents");
    let target = parents.join("demo");
    std::fs::create_dir_all(&target).unwrap();
    std::fs::write(target.join("keep.txt"), "precious").unwrap();
    let fs = FsService::new();
    let mut projects = store(&dir);

    let value = create_repo(
        &mut projects,
        &fs,
        &create_args(&parents, "demo", Some(RepoKind::Git)),
        1,
    );

    assert_eq!(
        value,
        json!({ "error": "\"demo\" already exists at this location and is not empty." })
    );
    assert_eq!(
        std::fs::read_to_string(target.join("keep.txt")).unwrap(),
        "precious"
    );
    assert!(!target.join(".git").exists());
    assert!(projects.repos().is_empty());
}

#[test]
fn create_repo_reuses_existing_empty_directory() {
    let _lock = git_identity_env_lock();
    let _env = with_test_git_identity();
    let dir = TestDir::new("create-empty-dir");
    let parents = dir.dir("parents");
    let target = parents.join("demo");
    std::fs::create_dir_all(&target).unwrap();
    let fs = FsService::new();
    let mut projects = store(&dir);

    let value = create_repo(
        &mut projects,
        &fs,
        &create_args(&parents, "demo", Some(RepoKind::Git)),
        2,
    );

    let repo = value.get("repo").expect("reused empty directory");
    assert_eq!(repo["path"], target.to_str().unwrap());
    assert_eq!(projects.repos().len(), 1);
    assert_eq!(
        git(&target, &["log", "-1", "--format=%s"]),
        "Initial commit"
    );
}

#[test]
fn create_repo_identity_failure_reports_setup_hint() {
    let _lock = git_identity_env_lock();
    let dir = TestDir::new("create-identity");
    let parents = dir.dir("parents");
    let home = dir.dir("identity-home");
    let _env = without_git_identity(&home);
    let fs = FsService::new();
    let mut projects = store(&dir);

    let value = create_repo(
        &mut projects,
        &fs,
        &create_args(&parents, "demo", Some(RepoKind::Git)),
        1,
    );
    assert_eq!(value, json!({ "error": IDENTITY_SETUP_HINT }));
    assert!(
        !parents.join("demo").exists(),
        "a directory this call created is removed on failure"
    );
    assert!(projects.repos().is_empty());

    let target = parents.join("demo2");
    std::fs::create_dir_all(&target).unwrap();
    let value = create_repo(
        &mut projects,
        &fs,
        &create_args(&parents, "demo2", Some(RepoKind::Git)),
        1,
    );
    assert_eq!(value, json!({ "error": IDENTITY_SETUP_HINT }));
    assert!(
        target.exists(),
        "a pre-existing empty directory survives the failure"
    );
    assert!(
        !target.join(".git").exists(),
        "the partial .git from git init is removed"
    );
    assert!(projects.repos().is_empty());
}

#[test]
fn base_ref_default_and_remote_count() {
    let dir = TestDir::new("base-ref-default");
    let repo_path = init_git_repo(&dir.path, "demo");
    let folder = dir.dir("notes");
    let mut projects = store(&dir);
    projects
        .mutate_repos(|repos| {
            repos.push(json!({
                "id": "r1",
                "path": repo_path.to_str().unwrap(),
                "kind": "git"
            }));
            repos.push(json!({
                "id": "r2",
                "path": folder.to_str().unwrap(),
                "kind": "folder"
            }));
        })
        .unwrap();

    assert_eq!(
        base_ref_default(&projects, "r1", None),
        json!({ "defaultBaseRef": "main", "remoteCount": 0 })
    );
    assert_eq!(
        base_ref_default(&projects, "r1", Some("local")),
        json!({ "defaultBaseRef": "main", "remoteCount": 0 })
    );
    assert_eq!(
        base_ref_default(&projects, "r1", Some("ssh:box")),
        json!({ "defaultBaseRef": null, "remoteCount": 0 })
    );
    assert_eq!(
        base_ref_default(&projects, "r2", None),
        json!({ "defaultBaseRef": null, "remoteCount": 0 })
    );
    assert_eq!(
        base_ref_default(&projects, "missing", None),
        json!({ "defaultBaseRef": null, "remoteCount": 0 })
    );

    git(
        &repo_path,
        &["remote", "add", "origin", "/nonexistent/origin.git"],
    );
    assert_eq!(
        base_ref_default(&projects, "r1", None),
        json!({ "defaultBaseRef": "main", "remoteCount": 1 })
    );
}

#[test]
fn search_base_refs_filters_and_limits() {
    let dir = TestDir::new("search-base-refs");
    let repo_path = init_git_repo(&dir.path, "demo");
    git(&repo_path, &["branch", "feature-x"]);
    let head = git(&repo_path, &["rev-parse", "HEAD"]);
    git(
        &repo_path,
        &["update-ref", "refs/remotes/origin/main", &head],
    );
    let folder = dir.dir("notes");
    let mut projects = store(&dir);
    projects
        .mutate_repos(|repos| {
            repos.push(json!({
                "id": "r1",
                "path": repo_path.to_str().unwrap(),
                "kind": "git"
            }));
            repos.push(json!({
                "id": "r2",
                "path": folder.to_str().unwrap(),
                "kind": "folder"
            }));
        })
        .unwrap();

    assert_eq!(
        search_base_ref_details(&projects, "r1", "origin", None, None),
        vec![json!({ "refName": "origin/main", "localBranchName": "main" })]
    );

    let names = search_base_refs(&projects, "r1", "", None, None);
    assert!(names.contains(&"main".to_string()));
    assert!(names.contains(&"feature-x".to_string()));
    assert!(names.contains(&"origin/main".to_string()));

    assert_eq!(
        search_base_refs(&projects, "r1", "", Some(1), None).len(),
        1
    );
    assert!(search_base_refs(&projects, "r1", "", Some(0), None).is_empty());
    assert!(search_base_refs(&projects, "r1", "origin", None, None)
        .iter()
        .all(|name| name.contains("origin")));
    assert!(search_base_refs(&projects, "r2", "", None, None).is_empty());
    assert!(search_base_ref_details(&projects, "missing", "", None, None).is_empty());
}
