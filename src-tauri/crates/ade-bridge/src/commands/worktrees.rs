use ade_core::models::repo::RepoKind;
use ade_core::models::worktree::Worktree;
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

fn folder_worktree(repo_id_value: &str, workspace: &Value) -> Worktree {
    let path = workspace
        .get("folderPath")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let name = workspace
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let mut worktree = Worktree::for_folder_workspace(repo_id_value, name, path, false);
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

/// Folder repos project their own root workspace first, then the repo's folder
/// workspaces by `lastActivityAt` descending (spec §5.3).
fn folder_worktrees(repo: &Value, folder_workspaces: &[Value]) -> Vec<Worktree> {
    let repo_id_value = repo_id(repo);
    let main = Worktree::for_folder_workspace(
        repo_id_value,
        repo_display_name(repo),
        repo_path(repo),
        true,
    );
    let mut extras: Vec<Worktree> = folder_workspaces
        .iter()
        .map(|workspace| folder_worktree(repo_id_value, workspace))
        .filter(|worktree| worktree.id != main.id && !worktree.path.is_empty())
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
            json!({ "id": "w1", "folderPath": "/folder/one", "name": "One", "lastActivityAt": 5 }),
            json!({
                "id": "w2",
                "folderPath": "/folder/two",
                "name": "Two",
                "lastActivityAt": 10,
                "isPinned": true,
                "comment": "hi",
                "workspaceStatus": "done"
            }),
        ];
        let worktrees = folder_worktrees(&folder_repo(), &workspaces);
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
    fn folder_workspace_at_the_repo_root_is_not_duplicated() {
        let workspaces = vec![
            json!({ "id": "w1", "folderPath": "/folder", "name": "Root", "lastActivityAt": 1 }),
        ];
        let worktrees = folder_worktrees(&folder_repo(), &workspaces);
        assert_eq!(worktrees.len(), 1);
        assert_eq!(worktrees[0].display_name, "Folder");
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
