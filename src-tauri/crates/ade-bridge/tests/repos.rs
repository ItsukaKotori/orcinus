//! Integration coverage for the repos registry and the minimal worktree
//! projection: real tempdirs, a real `git init` + commit, a real `FsService`
//! authorization registry, and the on-disk `ProjectsStore`.

use std::path::{Path, PathBuf};
use std::process::Command;

use ade_bridge::commands::repos::{
    add_repo, default_create_project_parent, remove_repo, reorder_repos_for_host, resolve_add_path,
    update_repo,
};
use ade_bridge::commands::worktrees::{list_all_worktrees, list_worktrees};
use ade_core::models::repo::RepoKind;
use ade_fs::FsService;
use ade_store::projects_store::ProjectsStore;
use serde_json::{json, Value};

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

    let worktrees = list_worktrees(&repo, &[], &fs).expect("git worktree list");
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

    let worktrees = list_worktrees(&outcome.repo, &[], &fs).unwrap();
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

    let worktrees = list_worktrees(&outcome.repo, &[], &fs).unwrap();
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

    let worktrees = list_worktrees(&repo, &[], &fs).unwrap();
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

    let worktrees = list_worktrees(&repo, &workspaces, &fs).unwrap();
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

    let first_worktrees = list_worktrees(&first, &workspaces, &fs).unwrap();
    assert_eq!(
        first_worktrees
            .iter()
            .map(|worktree| worktree.path.as_str())
            .collect::<Vec<_>>(),
        vec![first_folder.to_str().unwrap(), dir.dir("first/one").to_str().unwrap()]
    );

    let second_worktrees = list_worktrees(&second, &workspaces, &fs).unwrap();
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

    let all = list_all_worktrees(&projects.repos(), &workspaces, &fs).unwrap();
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

    let worktrees = list_worktrees(&repo, &workspaces, &fs).unwrap();
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

    let worktrees = list_worktrees(&repo, &workspaces, &fs).unwrap();
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

    let worktrees =
        list_all_worktrees(&projects.repos(), &projects.folder_workspaces(), &fs).unwrap();
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
