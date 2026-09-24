//! Integration coverage for the projectGroups and folderWorkspaces registries:
//! real tempdirs, real `git init` repos for nested scans/imports, a real
//! `FsService` authorization registry, and the on-disk `ProjectsStore`.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};

use ade_bridge::commands::folder_workspaces::{
    create_folder_workspace, delete_folder_workspace, get_path_status, list_folder_workspaces,
    update_folder_workspace, FolderWorkspacesCreateArgs, FolderWorkspacesGetPathStatusArgs,
};
use ade_bridge::commands::project_groups::{
    create_group, delete_group, import_nested, list_groups, move_project_to_group,
    scan_nested_repos, update_group, ProjectGroupsCreateArgs, ProjectGroupsImportNestedArgs,
};
use ade_core::models::folder_workspace::FolderWorkspacePathStatusReason;
use ade_core::models::project_group::{
    new_project_group, normalize_nested_repo_scan_options, NestedRepoScanOptions,
    NestedRepoSelectedPathKind, ProjectGroupCreatedFrom,
};
use ade_fs::FsService;
use ade_store::projects_store::ProjectsStore;
use serde_json::{json, Value};

struct TestDir {
    path: PathBuf,
}

impl TestDir {
    fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "ade-bridge-catalogs-it-{name}-{}",
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

fn group_id(group: &Value) -> &str {
    group["id"].as_str().expect("group id")
}

fn workspace_id(workspace: &Value) -> &str {
    workspace["id"].as_str().expect("workspace id")
}

fn create_args(name: &str) -> ProjectGroupsCreateArgs {
    ProjectGroupsCreateArgs {
        name: name.to_string(),
        parent_path: None,
        connection_id: None,
        parent_group_id: None,
        created_from: None,
    }
}

fn options(max_depth: u64, max_repos: u64) -> NestedRepoScanOptions {
    NestedRepoScanOptions {
        max_depth,
        max_repos,
        timeout_ms: None,
    }
}

fn folder_args(group_id_value: &str, folder_path: Option<&str>) -> FolderWorkspacesCreateArgs {
    FolderWorkspacesCreateArgs {
        project_group_id: group_id_value.to_string(),
        name: None,
        folder_path: folder_path.map(str::to_string),
        connection_id: None,
        linked_task: None,
        linked_task_source_context: None,
        created_with_agent: None,
        pending_first_agent_message_rename: None,
    }
}

#[test]
fn group_create_defaults_persist_and_increment_tab_order() {
    let dir = TestDir::new("group-create");
    let mut projects = store(&dir);
    let first = create_group(&mut projects, &create_args("  First  "), 100).unwrap();
    assert_eq!(first["name"], "First");
    assert_eq!(first["parentPath"], Value::Null);
    assert_eq!(first["connectionId"], Value::Null);
    assert_eq!(first["parentGroupId"], Value::Null);
    assert_eq!(first["createdFrom"], "manual");
    assert_eq!(first["tabOrder"], 0);
    assert_eq!(first["isCollapsed"], false);
    assert_eq!(first["color"], Value::Null);
    assert_eq!(first["createdAt"], 100);
    assert_eq!(first["updatedAt"], 100);

    let child = create_group(
        &mut projects,
        &ProjectGroupsCreateArgs {
            parent_group_id: Some(group_id(&first).to_string()),
            parent_path: Some("/parent".to_string()),
            created_from: Some(ProjectGroupCreatedFrom::FolderScan),
            ..create_args("Child")
        },
        200,
    )
    .unwrap();
    assert_eq!(child["tabOrder"], 1);
    assert_eq!(child["createdFrom"], "folder-scan");
    assert_eq!(child["parentPath"], "/parent");

    let reloaded = store(&dir);
    assert_eq!(list_groups(&reloaded).len(), 2);
    let reloaded_first = reloaded
        .project_groups()
        .into_iter()
        .find(|group| group_id(group) == group_id(&first))
        .expect("first group persisted");
    assert_eq!(reloaded_first["name"], "First");
    assert_eq!(reloaded_first["isCollapsed"], false);
}

#[test]
fn group_update_delete_and_move_round_trip() {
    let dir = TestDir::new("group-mutations");
    let fs = FsService::new();
    let mut projects = store(&dir);
    let root = create_group(&mut projects, &create_args("Root"), 1).unwrap();
    let child = create_group(&mut projects, &create_args("Child"), 2).unwrap();
    let root_id = group_id(&root).to_string();
    let child_id = group_id(&child).to_string();
    projects
        .mutate_groups(|groups| {
            groups
                .iter_mut()
                .find(|group| group_id(group) == child_id)
                .expect("child group")["parentGroupId"] = json!(root_id);
        })
        .unwrap();

    let updated = update_group(
        &mut projects,
        &root_id,
        &json!({ "name": "  Renamed  ", "isCollapsed": true, "tabOrder": 9, "color": "#abc" }),
        10,
    )
    .unwrap()
    .expect("group exists");
    assert_eq!(updated["name"], "Renamed");
    assert_eq!(updated["isCollapsed"], true);
    assert_eq!(updated["tabOrder"], 9);
    assert_eq!(updated["updatedAt"], 10);
    assert!(update_group(&mut projects, "missing", &json!({}), 11)
        .unwrap()
        .is_none());

    let folder = dir.dir("workspace");
    projects
        .mutate_repos(|repos| {
            repos.push(json!({ "id": "r1", "path": "/a", "projectGroupId": root_id }));
            repos.push(json!({ "id": "r2", "path": "/b", "projectGroupId": child_id }));
            repos.push(json!({ "id": "r3", "path": "/c" }));
        })
        .unwrap();
    projects
        .mutate_folder_workspaces(|workspaces| {
            workspaces.push(json!({
                "id": "w1",
                "projectGroupId": child_id,
                "folderPath": folder.to_str().unwrap()
            }));
        })
        .unwrap();
    fs.authorize_root(folder.to_str().unwrap()).unwrap();

    let moved = move_project_to_group(&mut projects, "r3", Some(&root_id), None)
        .unwrap()
        .expect("repo exists");
    assert_eq!(moved["projectGroupId"], root_id);
    assert_eq!(moved["projectGroupOrder"], 0);
    assert!(move_project_to_group(&mut projects, "missing", None, None)
        .unwrap()
        .is_none());

    assert!(delete_group(&mut projects, &fs, &root_id).unwrap());
    assert!(list_groups(&projects).is_empty(), "subtree removed");
    let repos = projects.repos();
    assert_eq!(repos.len(), 3);
    assert!(repos
        .iter()
        .all(|repo| repo["projectGroupId"] == Value::Null));
    assert!(projects.folder_workspaces().is_empty());
    assert!(matches!(
        fs.resolve(folder.to_str().unwrap()),
        Err(ade_fs::FsError::PathAccessDenied)
    ));
    assert!(!delete_group(&mut projects, &fs, &root_id).unwrap());

    let reloaded = store(&dir);
    assert!(reloaded.project_groups().is_empty());
    assert!(reloaded
        .repos()
        .iter()
        .all(|repo| repo["projectGroupId"] == Value::Null));
}

#[test]
fn scan_finds_directories_files_and_bare_repos() {
    let dir = TestDir::new("scan-markers");
    let root = dir.dir("root");
    std::fs::create_dir_all(root.join("dir-repo").join(".git")).unwrap();
    let file_repo = dir.dir("root/file-repo");
    std::fs::write(file_repo.join(".git"), "gitdir: /elsewhere\n").unwrap();
    let bare = dir.dir("root/bare-repo");
    std::fs::create_dir_all(bare.join("objects")).unwrap();
    std::fs::create_dir_all(bare.join("refs")).unwrap();
    std::fs::write(bare.join("HEAD"), "ref: refs/heads/main\n").unwrap();
    std::fs::create_dir_all(root.join("node_modules").join("dep").join(".git")).unwrap();
    std::fs::create_dir_all(root.join("nested").join("deep").join(".git")).unwrap();
    std::fs::write(root.join(".gitignore"), "ignored/\n").unwrap();
    std::fs::create_dir_all(root.join("ignored").join("repo").join(".git")).unwrap();

    let result = scan_nested_repos(root.to_str().unwrap(), &options(3, 100), None, None);
    assert_eq!(result.selected_path, root.to_str().unwrap());
    assert_eq!(
        result.selected_path_kind,
        NestedRepoSelectedPathKind::NonGitFolder
    );
    assert!(!result.truncated);
    assert!(!result.timed_out);
    assert!(!result.stopped);
    assert_eq!(result.max_depth, 3);
    assert_eq!(result.max_repos, 100);
    assert_eq!(result.timeout_ms, None);
    assert_eq!(
        result
            .repos
            .iter()
            .map(|repo| repo.display_name.as_str())
            .collect::<Vec<_>>(),
        vec!["bare-repo", "dir-repo", "file-repo", "deep"]
    );
    assert_eq!(result.repos[3].depth, 2);

    // A selected path that is itself a repo stops the walk immediately.
    let selected = scan_nested_repos(
        root.join("dir-repo").to_str().unwrap(),
        &options(3, 100),
        None,
        None,
    );
    assert_eq!(
        selected.selected_path_kind,
        NestedRepoSelectedPathKind::GitRepo
    );
    assert!(selected.repos.is_empty());
}

#[test]
fn scan_caps_and_cancel_match_the_oracle_contract() {
    let dir = TestDir::new("scan-caps");
    let root = dir.dir("root");
    for name in ["a", "b", "c"] {
        std::fs::create_dir_all(root.join(name).join(".git")).unwrap();
    }

    let capped = scan_nested_repos(root.to_str().unwrap(), &options(3, 2), None, None);
    assert!(capped.truncated);
    assert_eq!(capped.repos.len(), 2);

    let cancel = AtomicBool::new(false);
    let cancelled = scan_nested_repos(
        root.to_str().unwrap(),
        &options(3, 100),
        Some(&cancel),
        Some(&mut |_: &_, _: u64| cancel.store(true, Ordering::SeqCst)),
    );
    assert!(cancelled.stopped);
    assert_eq!(cancelled.repos.len(), 1);

    let pre_cancelled = AtomicBool::new(true);
    let stopped = scan_nested_repos(
        root.to_str().unwrap(),
        &options(3, 100),
        Some(&pre_cancelled),
        None,
    );
    assert!(stopped.stopped);
    assert!(stopped.repos.is_empty());
}

#[test]
fn scan_options_normalize_to_the_oracle_clamps() {
    let defaults = normalize_nested_repo_scan_options(&json!({}));
    assert_eq!(defaults.max_depth, 3);
    assert_eq!(defaults.max_repos, 100);
    assert_eq!(defaults.timeout_ms, None);

    let clamped = normalize_nested_repo_scan_options(&json!({
        "maxDepth": 12,
        "maxRepos": 900,
        "timeoutMs": 100
    }));
    assert_eq!(clamped.max_depth, 8);
    assert_eq!(clamped.max_repos, 500);
    assert_eq!(clamped.timeout_ms, Some(500));
}

#[test]
fn import_is_idempotent_and_reuses_repos_add_semantics() {
    let dir = TestDir::new("import");
    let fs = FsService::new();
    let mut projects = store(&dir);
    let root = dir.dir("workspace");
    let first = init_git_repo(&root, "auth-service");
    let second = init_git_repo(&root, "billing-service");

    let scan = scan_nested_repos(root.to_str().unwrap(), &options(3, 100), None, None);
    let import_args = ProjectGroupsImportNestedArgs {
        parent_path: root.to_str().unwrap().to_string(),
        group_name: Some("  Services  ".to_string()),
        project_paths: vec![
            first.to_str().unwrap().to_string(),
            second.to_str().unwrap().to_string(),
            "/elsewhere".to_string(),
        ],
        connection_id: None,
        scan_id: None,
        mode: "group".to_string(),
    };

    let result = import_nested(&mut projects, &fs, &import_args, &scan, 500).unwrap();
    assert_eq!(result.imported_count, 2);
    assert_eq!(result.already_known_count, 0);
    assert_eq!(result.failed_count, 1);
    assert_eq!(
        result.projects[0].error.as_deref(),
        Some("Repository was not found in the nested repo scan result")
    );
    let group = result.group.as_ref().expect("root group").0.clone();
    assert_eq!(group["name"], "Services");
    assert_eq!(group["parentPath"], root.to_str().unwrap());
    assert_eq!(group["createdFrom"], "folder-scan");
    assert_eq!(projects.project_groups().len(), 1);
    let repos = projects.repos();
    assert_eq!(repos.len(), 2);
    assert!(repos
        .iter()
        .all(|repo| repo["projectGroupId"] == group_id(&group)));
    assert_eq!(repos[0]["projectGroupOrder"], 0);
    assert_eq!(repos[1]["projectGroupOrder"], 1);
    assert!(fs.resolve(first.to_str().unwrap()).is_ok());

    let again = import_nested(&mut projects, &fs, &import_args, &scan, 600).unwrap();
    assert_eq!(again.imported_count, 0);
    assert_eq!(again.already_known_count, 2);
    assert_eq!(again.failed_count, 1);
    assert_eq!(projects.repos().len(), 2, "no duplicate rows");
    // The oracle creates a fresh root group per import and moves the known
    // repos into it, so repeated imports never duplicate repos but do stack
    // groups.
    assert_eq!(projects.project_groups().len(), 2);
    let second_group = again.group.as_ref().expect("root group").0.clone();
    assert_ne!(group_id(&second_group), group_id(&group));
    assert!(projects
        .repos()
        .iter()
        .all(|repo| repo["projectGroupId"] == group_id(&second_group)));

    // `separate` mode imports without creating a group.
    let separate = ProjectGroupsImportNestedArgs {
        group_name: None,
        mode: "separate".to_string(),
        ..import_args.clone()
    };
    let separate_result = import_nested(&mut projects, &fs, &separate, &scan, 700).unwrap();
    assert_eq!(separate_result.already_known_count, 2);
    assert!(separate_result.group.is_none());
    assert_eq!(projects.project_groups().len(), 2);
}

#[test]
fn import_without_matches_rolls_the_group_back() {
    let dir = TestDir::new("import-rollback");
    let fs = FsService::new();
    let mut projects = store(&dir);
    let root = dir.dir("workspace");
    init_git_repo(&root, "service");
    let scan = scan_nested_repos(root.to_str().unwrap(), &options(3, 100), None, None);

    let result = import_nested(
        &mut projects,
        &fs,
        &ProjectGroupsImportNestedArgs {
            parent_path: root.to_str().unwrap().to_string(),
            group_name: None,
            project_paths: vec!["/elsewhere".to_string()],
            connection_id: None,
            scan_id: None,
            mode: "group".to_string(),
        },
        &scan,
        1,
    )
    .unwrap();
    assert_eq!(result.failed_count, 1);
    assert!(result.group.is_none());
    assert!(
        projects.project_groups().is_empty(),
        "empty group rolled back"
    );
    assert!(projects.repos().is_empty());
}

#[test]
fn import_falls_back_to_a_group_name_from_the_parent_basename() {
    let dir = TestDir::new("import-name");
    let fs = FsService::new();
    let mut projects = store(&dir);
    let root = dir.dir("my-workspace");
    init_git_repo(&root, "service");
    let scan = scan_nested_repos(root.to_str().unwrap(), &options(3, 100), None, None);

    let result = import_nested(
        &mut projects,
        &fs,
        &ProjectGroupsImportNestedArgs {
            parent_path: root.to_str().unwrap().to_string(),
            group_name: Some("   ".to_string()),
            project_paths: vec![root.join("service").to_str().unwrap().to_string()],
            connection_id: None,
            scan_id: None,
            mode: "group".to_string(),
        },
        &scan,
        1,
    )
    .unwrap();
    let group = result.group.as_ref().expect("root group").0.clone();
    assert_eq!(group["name"], "my-workspace");
}

#[test]
fn folder_workspace_create_validates_group_and_path() {
    let dir = TestDir::new("folder-create-validation");
    let fs = FsService::new();
    let mut projects = store(&dir);
    assert_eq!(
        create_folder_workspace(&mut projects, &fs, &folder_args("missing", None), 1)
            .unwrap_err()
            .to_string(),
        "folder_workspace_project_group_not_found"
    );

    let parent = dir.dir("parent");
    let group = new_project_group(
        &ade_core::ids::new_uuid(),
        "Folder Group",
        Some(parent.to_str().unwrap()),
        None,
        None,
        ProjectGroupCreatedFrom::Manual,
        0,
        1,
    );
    let group_id_value = group_id(&group).to_string();
    projects.mutate_groups(|groups| groups.push(group)).unwrap();

    let missing = dir.path.join("missing");
    assert_eq!(
        create_folder_workspace(
            &mut projects,
            &fs,
            &folder_args(&group_id_value, missing.to_str()),
            2
        )
        .unwrap_err()
        .to_string(),
        format!(
            "folder_workspace_path_missing:{}",
            missing.to_str().unwrap()
        )
    );

    let file = dir.file("plain.txt");
    std::fs::write(&file, "hi").unwrap();
    assert_eq!(
        create_folder_workspace(
            &mut projects,
            &fs,
            &folder_args(&group_id_value, file.to_str()),
            3
        )
        .unwrap_err()
        .to_string(),
        format!(
            "folder_workspace_path_not_directory:{}",
            file.to_str().unwrap()
        )
    );
    assert!(projects.folder_workspaces().is_empty());
}

#[test]
fn folder_workspace_defaults_delete_and_revoke() {
    let dir = TestDir::new("folder-defaults");
    let fs = FsService::new();
    let mut projects = store(&dir);
    let parent = dir.dir("parent");
    let group = new_project_group(
        &ade_core::ids::new_uuid(),
        "Folder Group",
        Some(parent.to_str().unwrap()),
        None,
        None,
        ProjectGroupCreatedFrom::Manual,
        0,
        1,
    );
    let group_id_value = group_id(&group).to_string();
    projects.mutate_groups(|groups| groups.push(group)).unwrap();

    let workspace =
        create_folder_workspace(&mut projects, &fs, &folder_args(&group_id_value, None), 42)
            .unwrap();
    assert_eq!(workspace["name"], "Folder Group workspace");
    assert_eq!(workspace["folderPath"], parent.to_str().unwrap());
    assert_eq!(workspace["connectionId"], Value::Null);
    assert_eq!(workspace["creatorProvenance"], json!({ "kind": "host" }));
    assert_eq!(workspace["comment"], "");
    assert_eq!(workspace["sortOrder"], 42);
    assert_eq!(workspace["isArchived"], false);
    assert_eq!(workspace["isUnread"], false);
    assert_eq!(workspace["isPinned"], false);
    assert_eq!(workspace["lastActivityAt"], 0);
    assert_eq!(workspace["createdAt"], 42);
    assert_eq!(workspace["updatedAt"], 42);
    assert_eq!(list_folder_workspaces(&projects).len(), 1);
    assert!(fs.resolve(parent.to_str().unwrap()).is_ok());

    let renamed = update_folder_workspace(
        &mut projects,
        &fs,
        workspace_id(&workspace),
        &json!({ "name": "  Notes  ", "isPinned": true, "comment": "hello" }),
        43,
    )
    .unwrap()
    .expect("workspace exists");
    assert_eq!(renamed["name"], "Notes");
    assert_eq!(renamed["isPinned"], true);
    assert_eq!(renamed["comment"], "hello");
    assert_eq!(renamed["updatedAt"], 43);
    assert!(
        update_folder_workspace(&mut projects, &fs, "missing", &json!({}), 44)
            .unwrap()
            .is_none()
    );

    let id = workspace_id(&workspace).to_string();
    assert!(delete_folder_workspace(&mut projects, &fs, &id).unwrap());
    assert!(projects.folder_workspaces().is_empty());
    assert!(matches!(
        fs.resolve(parent.to_str().unwrap()),
        Err(ade_fs::FsError::PathAccessDenied)
    ));
    assert!(!delete_folder_workspace(&mut projects, &fs, &id).unwrap());
}

#[test]
fn folder_path_status_covers_three_scopes_and_states() {
    let dir = TestDir::new("folder-status");
    let fs = FsService::new();
    let mut projects = store(&dir);
    let parent = dir.dir("parent");
    let group = new_project_group(
        &ade_core::ids::new_uuid(),
        "Folder Group",
        Some(parent.to_str().unwrap()),
        None,
        None,
        ProjectGroupCreatedFrom::Manual,
        0,
        1,
    );
    let group_id_value = group_id(&group).to_string();
    projects.mutate_groups(|groups| groups.push(group)).unwrap();
    let workspace =
        create_folder_workspace(&mut projects, &fs, &folder_args(&group_id_value, None), 1)
            .unwrap();

    let by_workspace = get_path_status(
        &projects,
        &FolderWorkspacesGetPathStatusArgs {
            scope: "folder-workspace".to_string(),
            folder_workspace_id: Some(workspace_id(&workspace).to_string()),
            project_group_id: None,
            path: None,
            connection_id: None,
        },
    )
    .unwrap();
    assert!(by_workspace.exists);
    assert_eq!(by_workspace.path, parent.to_str().unwrap());
    assert_eq!(by_workspace.reason, None);

    let by_group = get_path_status(
        &projects,
        &FolderWorkspacesGetPathStatusArgs {
            scope: "project-group".to_string(),
            folder_workspace_id: None,
            project_group_id: Some(group_id_value.clone()),
            path: None,
            connection_id: None,
        },
    )
    .unwrap();
    assert!(by_group.exists);

    let missing_path = dir.path.join("missing");
    let missing = get_path_status(
        &projects,
        &FolderWorkspacesGetPathStatusArgs {
            scope: "path".to_string(),
            folder_workspace_id: None,
            project_group_id: None,
            path: Some(missing_path.to_str().unwrap().to_string()),
            connection_id: None,
        },
    )
    .unwrap();
    assert!(!missing.exists);
    assert_eq!(
        missing.reason,
        Some(FolderWorkspacePathStatusReason::Missing)
    );

    let file = dir.file("plain.txt");
    std::fs::write(&file, "hi").unwrap();
    let not_directory = get_path_status(
        &projects,
        &FolderWorkspacesGetPathStatusArgs {
            scope: "path".to_string(),
            folder_workspace_id: None,
            project_group_id: None,
            path: Some(file.to_str().unwrap().to_string()),
            connection_id: None,
        },
    )
    .unwrap();
    assert!(!not_directory.exists);
    assert_eq!(
        not_directory.reason,
        Some(FolderWorkspacePathStatusReason::NotDirectory)
    );

    assert_eq!(
        get_path_status(
            &projects,
            &FolderWorkspacesGetPathStatusArgs {
                scope: "folder-workspace".to_string(),
                folder_workspace_id: Some("missing".to_string()),
                project_group_id: None,
                path: None,
                connection_id: None,
            }
        )
        .unwrap_err()
        .to_string(),
        "folder_workspace_path_scope_not_found"
    );
}

#[test]
fn folder_workspace_status_answers_unavailable_for_ssh_connections() {
    let dir = TestDir::new("folder-ssh");
    let mut projects = store(&dir);
    let group = new_project_group(
        &ade_core::ids::new_uuid(),
        "Remote Group",
        Some("/remote/path"),
        Some("box"),
        None,
        ProjectGroupCreatedFrom::Manual,
        0,
        1,
    );
    let group_id_value = group_id(&group).to_string();
    projects.mutate_groups(|groups| groups.push(group)).unwrap();

    let status = get_path_status(
        &projects,
        &FolderWorkspacesGetPathStatusArgs {
            scope: "project-group".to_string(),
            folder_workspace_id: None,
            project_group_id: Some(group_id_value),
            path: None,
            connection_id: None,
        },
    )
    .unwrap();
    assert!(!status.exists);
    assert_eq!(
        status.reason,
        Some(FolderWorkspacePathStatusReason::Unavailable),
        "A has no SSH provider"
    );
}
