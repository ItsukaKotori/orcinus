use ade_core::models::repo::RepoKind;
use ade_core::models::worktree::Worktree;
use ade_core::path_compare::normalize_for_comparison;
use serde::Deserialize;
use serde_json::Value;
use tauri::State;

use crate::commands::run_blocking;
use crate::errors::BridgeError;
use crate::state::{lock, AppState};

#[derive(Debug, Clone, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct WorktreesListArgs {
    pub repo_id: String,
}

pub fn repo_id(repo: &Value) -> &str {
    repo.get("id").and_then(Value::as_str).unwrap_or_default()
}

pub fn repo_path(repo: &Value) -> &str {
    repo.get("path").and_then(Value::as_str).unwrap_or_default()
}

pub fn repo_display_name(repo: &Value) -> &str {
    repo.get("displayName")
        .and_then(Value::as_str)
        .unwrap_or_default()
}

/// Unknown or absent kinds are git (oracle `getRepoKind`).
pub fn repo_kind(repo: &Value) -> RepoKind {
    repo.get("kind")
        .and_then(Value::as_str)
        .and_then(RepoKind::parse)
        .unwrap_or(RepoKind::Git)
}

/// Map one `git worktree list` entry (spec §5.3).
pub fn git_worktree(repo: &Value, entry: &ade_git::GitWorktreeEntry) -> Worktree {
    let branch = entry.branch.clone().unwrap_or_default();
    Worktree::for_git_entry(
        repo_id(repo),
        repo_display_name(repo),
        &entry.path,
        &entry.head,
        &branch,
        entry.is_bare,
        entry.is_main_worktree,
    )
}

/// A folder workspace belongs to a repo only when both carry the same project
/// group. An ungrouped repo (absent/null `projectGroupId`) owns just its root
/// workspace, so a null group never pulls in other repos' workspaces.
fn folder_workspace_in_scope(repo: &Value, workspace: &Value) -> bool {
    match repo.get("projectGroupId").and_then(Value::as_str) {
        Some(group) => workspace.get("projectGroupId").and_then(Value::as_str) == Some(group),
        None => false,
    }
}

fn folder_worktree(repo: &Value, workspace: &Value) -> Worktree {
    let path = workspace
        .get("folderPath")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let name = workspace
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim();
    // Why: a blank name falls back to the repo label, matching
    // `mergeFolderWorkspace`'s `meta.displayName || repo.displayName`.
    let display_name = if name.is_empty() {
        repo_display_name(repo)
    } else {
        name
    };
    let mut worktree = Worktree::for_folder_workspace(repo_id(repo), display_name, path, false);
    if let Some(comment) = workspace.get("comment").and_then(Value::as_str) {
        worktree.comment = comment.to_string();
    }
    for (field, target) in [
        ("isArchived", &mut worktree.is_archived),
        ("isUnread", &mut worktree.is_unread),
        ("isPinned", &mut worktree.is_pinned),
    ] {
        if let Some(flag) = workspace.get(field).and_then(Value::as_bool) {
            *target = flag;
        }
    }
    if let Some(sort_order) = workspace.get("sortOrder").and_then(Value::as_u64) {
        worktree.sort_order = sort_order;
    }
    if let Some(last_activity_at) = workspace.get("lastActivityAt").and_then(Value::as_u64) {
        worktree.last_activity_at = last_activity_at;
    }
    if let Some(status) = workspace.get("workspaceStatus").and_then(Value::as_str) {
        if !status.is_empty() {
            worktree.workspace_status = status.to_string();
        }
    }
    worktree
}

/// Folder repos project their own root workspace first, then the repo's own
/// folder workspaces by `lastActivityAt` descending (spec §5.3).
fn folder_worktrees(repo: &Value, folder_workspaces: &[Value]) -> Vec<Worktree> {
    let main = Worktree::for_folder_workspace(
        repo_id(repo),
        repo_display_name(repo),
        repo_path(repo),
        true,
    );
    // Why normalized: the repo path may keep a `/folder/` spelling while a
    // folder workspace points at the same directory; raw id comparison would
    // then emit the root row twice.
    let main_path_key = normalize_for_comparison(&main.path);
    let mut extras: Vec<Worktree> = folder_workspaces
        .iter()
        .filter(|workspace| folder_workspace_in_scope(repo, workspace))
        .map(|workspace| folder_worktree(repo, workspace))
        .filter(|worktree| {
            !worktree.path.is_empty() && normalize_for_comparison(&worktree.path) != main_path_key
        })
        .collect();
    extras.sort_by_key(|worktree| std::cmp::Reverse(worktree.last_activity_at));
    let mut worktrees = Vec::with_capacity(extras.len() + 1);
    worktrees.push(main);
    worktrees.extend(extras);
    worktrees
}

/// `worktrees.list({repoId})`: git repos shell out to `git worktree list`
/// (prunable entries are already dropped by the porcelain parser); folder repos
/// project their workspaces. An unknown repo lists nothing.
pub fn list_worktrees(
    repo: &Value,
    folder_workspaces: &[Value],
) -> Result<Vec<Worktree>, BridgeError> {
    match repo_kind(repo) {
        RepoKind::Folder => Ok(folder_worktrees(repo, folder_workspaces)),
        RepoKind::Git => {
            let entries = ade_git::worktree_list(repo_path(repo))?;
            Ok(entries
                .iter()
                .map(|entry| git_worktree(repo, entry))
                .collect())
        }
    }
}

/// `worktrees.listAll()`: every repo's projection, in registry order.
pub fn list_all_worktrees(
    repos: &[Value],
    folder_workspaces: &[Value],
) -> Result<Vec<Worktree>, BridgeError> {
    let mut worktrees = Vec::new();
    for repo in repos {
        worktrees.extend(list_worktrees(repo, folder_workspaces)?);
    }
    Ok(worktrees)
}

/// Project the worktrees of one repo.
#[tauri::command]
#[specta::specta]
pub async fn worktrees_list(
    state: State<'_, AppState>,
    args: WorktreesListArgs,
) -> Result<Vec<Worktree>, BridgeError> {
    let (repo, folder_workspaces) = {
        let projects = lock(&state.projects);
        let repo = projects
            .repos()
            .into_iter()
            .find(|repo| repo_id(repo) == args.repo_id);
        (repo, projects.folder_workspaces())
    };
    let Some(repo) = repo else {
        return Ok(Vec::new());
    };
    run_blocking(move || list_worktrees(&repo, &folder_workspaces)).await
}

/// Project every repo's worktrees, merged in registry order.
#[tauri::command]
#[specta::specta]
pub async fn worktrees_list_all(
    state: State<'_, AppState>,
) -> Result<Vec<Worktree>, BridgeError> {
    let (repos, folder_workspaces) = {
        let projects = lock(&state.projects);
        (projects.repos(), projects.folder_workspaces())
    };
    run_blocking(move || list_all_worktrees(&repos, &folder_workspaces)).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use ade_git::GitWorktreeEntry;
    use serde_json::json;

    fn git_repo() -> Value {
        json!({ "id": "r1", "path": "/repo", "displayName": "Repo", "kind": "git" })
    }

    fn folder_repo() -> Value {
        json!({ "id": "f1", "path": "/folder", "displayName": "Folder", "kind": "folder" })
    }

    fn grouped_folder_repo(group: &str) -> Value {
        json!({
            "id": "f1",
            "path": "/folder",
            "displayName": "Folder",
            "kind": "folder",
            "projectGroupId": group
        })
    }

    #[test]
    fn kind_defaults_to_git_for_absent_or_unknown_values() {
        assert_eq!(repo_kind(&json!({ "id": "r" })), RepoKind::Git);
        assert_eq!(repo_kind(&json!({ "kind": "weird" })), RepoKind::Git);
        assert_eq!(repo_kind(&json!({ "kind": "folder" })), RepoKind::Folder);
    }

    #[test]
    fn git_entry_maps_branch_short_and_defaults() {
        let entry = GitWorktreeEntry {
            path: "/repo".to_string(),
            head: "abc".to_string(),
            branch: Some("refs/heads/main".to_string()),
            is_bare: false,
            is_main_worktree: true,
        };
        let worktree = git_worktree(&git_repo(), &entry);
        assert_eq!(worktree.id, "r1::/repo");
        assert_eq!(worktree.display_name, "main");
        assert_eq!(worktree.head, "abc");
        assert_eq!(worktree.branch, "refs/heads/main");
        assert!(worktree.is_main_worktree);
        assert_eq!(
            worktree.workspace_status,
            ade_core::models::worktree::DEFAULT_WORKSPACE_STATUS
        );
    }

    #[test]
    fn detached_entry_falls_back_to_repo_display_name() {
        let entry = GitWorktreeEntry {
            path: "/repo/wt".to_string(),
            head: "def".to_string(),
            branch: None,
            is_bare: false,
            is_main_worktree: false,
        };
        let worktree = git_worktree(&git_repo(), &entry);
        assert_eq!(worktree.display_name, "Repo");
        assert_eq!(worktree.branch, "");
        assert!(!worktree.is_main_worktree);
    }

    #[test]
    fn folder_repo_projects_main_workspace_without_git_metadata() {
        let worktrees = folder_worktrees(&folder_repo(), &[]);
        assert_eq!(worktrees.len(), 1);
        let main = &worktrees[0];
        assert_eq!(main.id, "f1::/folder");
        assert_eq!(main.path, "/folder");
        assert_eq!(main.display_name, "Folder");
        assert_eq!(main.head, "");
        assert_eq!(main.branch, "");
        assert!(main.is_main_worktree);
        assert!(!main.is_bare);
    }

    #[test]
    fn folder_workspaces_follow_main_sorted_by_last_activity_desc() {
        let workspaces = vec![
            json!({
                "id": "w1",
                "projectGroupId": "g1",
                "folderPath": "/folder/one",
                "name": "One",
                "lastActivityAt": 5
            }),
            json!({
                "id": "w2",
                "projectGroupId": "g1",
                "folderPath": "/folder/two",
                "name": "Two",
                "lastActivityAt": 10,
                "isPinned": true,
                "comment": "hi",
                "workspaceStatus": "done"
            }),
        ];
        let worktrees = folder_worktrees(&grouped_folder_repo("g1"), &workspaces);
        assert_eq!(
            worktrees.iter().map(|w| w.id.as_str()).collect::<Vec<_>>(),
            vec!["f1::/folder", "f1::/folder/two", "f1::/folder/one"]
        );
        assert!(worktrees[0].is_main_worktree);
        assert!(!worktrees[1].is_main_worktree);
        assert_eq!(worktrees[1].display_name, "Two");
        assert_eq!(worktrees[1].last_activity_at, 10);
        assert!(worktrees[1].is_pinned);
        assert_eq!(worktrees[1].comment, "hi");
        assert_eq!(worktrees[1].workspace_status, "done");
        assert_eq!(worktrees[1].head, "");
        assert_eq!(worktrees[1].branch, "");
    }

    #[test]
    fn folder_workspaces_are_scoped_to_the_repo_group() {
        let workspaces = vec![
            json!({
                "id": "w1",
                "projectGroupId": "g1",
                "folderPath": "/folder/one",
                "name": "One",
                "lastActivityAt": 1
            }),
            json!({
                "id": "w2",
                "projectGroupId": "g2",
                "folderPath": "/folder/two",
                "name": "Two",
                "lastActivityAt": 2
            }),
            json!({
                "id": "w3",
                "projectGroupId": null,
                "folderPath": "/folder/three",
                "name": "Three",
                "lastActivityAt": 3
            }),
        ];

        let first = folder_worktrees(&grouped_folder_repo("g1"), &workspaces);
        assert_eq!(
            first.iter().map(|w| w.path.as_str()).collect::<Vec<_>>(),
            vec!["/folder", "/folder/one"]
        );

        let mut second_repo = grouped_folder_repo("g2");
        second_repo["id"] = json!("f2");
        second_repo["path"] = json!("/other");
        let second = folder_worktrees(&second_repo, &workspaces);
        assert_eq!(
            second.iter().map(|w| w.path.as_str()).collect::<Vec<_>>(),
            vec!["/other", "/folder/two"]
        );
    }

    #[test]
    fn ungrouped_folder_repo_sees_only_its_root() {
        let workspaces = vec![
            json!({
                "id": "w1",
                "projectGroupId": null,
                "folderPath": "/folder/one",
                "name": "One",
                "lastActivityAt": 1
            }),
            json!({
                "id": "w2",
                "projectGroupId": "g1",
                "folderPath": "/folder/two",
                "name": "Two",
                "lastActivityAt": 2
            }),
        ];
        let worktrees = folder_worktrees(&folder_repo(), &workspaces);
        assert_eq!(worktrees.len(), 1);
        assert_eq!(worktrees[0].id, "f1::/folder");
        assert!(worktrees[0].is_main_worktree);
    }

    #[test]
    fn folder_workspace_at_the_repo_root_is_not_duplicated() {
        let workspaces = vec![
            // Same directory as the root, spelled with a trailing separator.
            json!({
                "id": "w1",
                "projectGroupId": "g1",
                "folderPath": "/folder/",
                "name": "Root",
                "lastActivityAt": 1
            }),
            json!({
                "id": "w2",
                "projectGroupId": "g1",
                "folderPath": "/folder/child",
                "name": "Child",
                "lastActivityAt": 2
            }),
        ];
        let worktrees = folder_worktrees(&grouped_folder_repo("g1"), &workspaces);
        assert_eq!(
            worktrees.iter().map(|w| w.id.as_str()).collect::<Vec<_>>(),
            vec!["f1::/folder", "f1::/folder/child"]
        );
        assert_eq!(worktrees[0].display_name, "Folder");
    }

    #[test]
    fn blank_folder_workspace_name_falls_back_to_repo_display_name() {
        let workspaces = vec![json!({
            "id": "w1",
            "projectGroupId": "g1",
            "folderPath": "/folder/one",
            "name": "   ",
            "lastActivityAt": 1
        })];
        let worktrees = folder_worktrees(&grouped_folder_repo("g1"), &workspaces);
        assert_eq!(worktrees[1].display_name, "Folder");
    }

    #[test]
    fn list_all_merges_repos_in_registry_order() {
        let repos = vec![
            folder_repo(),
            json!({ "id": "f2", "path": "/other", "displayName": "Other", "kind": "folder" }),
        ];
        let worktrees = list_all_worktrees(&repos, &[]).unwrap();
        assert_eq!(
            worktrees.iter().map(|w| w.id.as_str()).collect::<Vec<_>>(),
            vec!["f1::/folder", "f2::/other"]
        );
    }
}
