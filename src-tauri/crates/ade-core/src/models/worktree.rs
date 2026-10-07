use serde::{Deserialize, Serialize};

/// Default `workspaceStatus` (`DEFAULT_WORKSPACE_STATUS_ID`).
pub const DEFAULT_WORKSPACE_STATUS: &str = "in-progress";

/// Minimal worktree projection (spec §5.3): the fields the renderer contract
/// requires, with the oracle defaults for everything A does not persist.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct Worktree {
    pub id: String,
    pub repo_id: String,
    pub display_name: String,
    pub display_name_mode: String,
    pub comment: String,
    pub linked_issue: Option<u64>,
    #[serde(rename = "linkedPR")]
    pub linked_pr: Option<u64>,
    pub linked_linear_issue: Option<String>,
    pub is_archived: bool,
    pub is_unread: bool,
    pub is_pinned: bool,
    pub sort_order: u64,
    pub last_activity_at: u64,
    pub path: String,
    pub head: String,
    pub branch: String,
    pub is_bare: bool,
    pub is_main_worktree: bool,
    pub workspace_status: String,
    #[cfg_attr(feature = "specta", specta(type = Option<crate::json::Json>))]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub diff_comments: Option<serde_json::Value>,
}

/// `branchShort = branch.replace(/^refs\/heads\//, '')` (oracle
/// `mergeWorktree`).
pub fn branch_short(branch: &str) -> &str {
    branch.strip_prefix("refs/heads/").unwrap_or(branch)
}

/// `displayName = branchShort || repo.displayName || basename(path)`.
pub fn automatic_display_name(branch: &str, repo_display_name: &str, path: &str) -> String {
    let short = branch_short(branch);
    if !short.is_empty() {
        return short.to_string();
    }
    let trimmed = repo_display_name.trim();
    if !trimmed.is_empty() {
        return trimmed.to_string();
    }
    super::repo::basename(path)
}

impl Worktree {
    /// Projection of one `git worktree list` entry (spec §5.3).
    pub fn for_git_entry(
        repo_id: &str,
        repo_display_name: &str,
        path: &str,
        head: &str,
        branch: &str,
        is_bare: bool,
        is_main_worktree: bool,
    ) -> Self {
        Self {
            id: crate::ids::worktree_id(repo_id, path),
            repo_id: repo_id.to_string(),
            display_name: automatic_display_name(branch, repo_display_name, path),
            display_name_mode: "automatic".to_string(),
            comment: String::new(),
            linked_issue: None,
            linked_pr: None,
            linked_linear_issue: None,
            is_archived: false,
            is_unread: false,
            is_pinned: false,
            sort_order: 0,
            last_activity_at: 0,
            path: path.to_string(),
            head: head.to_string(),
            branch: branch.to_string(),
            is_bare,
            is_main_worktree,
            workspace_status: DEFAULT_WORKSPACE_STATUS.to_string(),
            diff_comments: None,
        }
    }

    /// Projection of a folder repo's own root workspace or one of its folder
    /// workspaces; folder rows carry no git head/branch.
    pub fn for_folder_workspace(
        repo_id: &str,
        display_name: &str,
        path: &str,
        is_main_worktree: bool,
    ) -> Self {
        Self {
            id: crate::ids::worktree_id(repo_id, path),
            repo_id: repo_id.to_string(),
            display_name: display_name.to_string(),
            display_name_mode: "automatic".to_string(),
            comment: String::new(),
            linked_issue: None,
            linked_pr: None,
            linked_linear_issue: None,
            is_archived: false,
            is_unread: false,
            is_pinned: false,
            sort_order: 0,
            last_activity_at: 0,
            path: path.to_string(),
            head: String::new(),
            branch: String::new(),
            is_bare: false,
            is_main_worktree,
            workspace_status: DEFAULT_WORKSPACE_STATUS.to_string(),
            diff_comments: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn branch_short_strips_only_the_heads_prefix() {
        assert_eq!(branch_short("refs/heads/feature/x"), "feature/x");
        assert_eq!(branch_short("refs/tags/v1"), "refs/tags/v1");
        assert_eq!(branch_short(""), "");
    }

    #[test]
    fn display_name_prefers_branch_then_repo_then_basename() {
        assert_eq!(
            automatic_display_name("refs/heads/main", "Repo", "/wt/main"),
            "main"
        );
        assert_eq!(automatic_display_name("", "Repo", "/wt/main"), "Repo");
        assert_eq!(automatic_display_name("", "  ", "/wt/main"), "main");
        assert_eq!(
            automatic_display_name("refs/tags/v1", "", "/wt/tagged"),
            "refs/tags/v1"
        );
    }

    #[test]
    fn git_entry_projection_uses_spec_defaults() {
        let worktree = Worktree::for_git_entry(
            "r1",
            "Repo",
            "/repo",
            "abc123",
            "refs/heads/main",
            false,
            true,
        );
        assert_eq!(worktree.id, "r1::/repo");
        assert_eq!(worktree.repo_id, "r1");
        assert_eq!(worktree.display_name, "main");
        assert_eq!(worktree.display_name_mode, "automatic");
        assert_eq!(worktree.head, "abc123");
        assert_eq!(worktree.branch, "refs/heads/main");
        assert!(worktree.is_main_worktree);
        assert!(!worktree.is_bare);
        assert_eq!(worktree.comment, "");
        assert!(worktree.linked_issue.is_none());
        assert!(worktree.linked_pr.is_none());
        assert!(worktree.linked_linear_issue.is_none());
        assert!(!worktree.is_archived);
        assert!(!worktree.is_unread);
        assert!(!worktree.is_pinned);
        assert_eq!(worktree.sort_order, 0);
        assert_eq!(worktree.last_activity_at, 0);
        assert_eq!(worktree.workspace_status, "in-progress");
    }

    #[test]
    fn folder_projection_has_no_git_metadata() {
        let worktree = Worktree::for_folder_workspace("r1", "Folder", "/folder", true);
        assert_eq!(worktree.id, "r1::/folder");
        assert_eq!(worktree.display_name, "Folder");
        assert_eq!(worktree.head, "");
        assert_eq!(worktree.branch, "");
        assert!(worktree.is_main_worktree);
        assert!(!worktree.is_bare);
        assert_eq!(worktree.workspace_status, "in-progress");
    }

    #[test]
    fn serializes_camel_case_with_null_links() {
        let value = serde_json::to_value(Worktree::for_git_entry(
            "r1",
            "Repo",
            "/repo",
            "",
            "",
            false,
            true,
        ))
        .unwrap();
        assert_eq!(value["repoId"], "r1");
        assert_eq!(value["displayName"], "Repo");
        assert_eq!(value["displayNameMode"], "automatic");
        assert_eq!(value["isMainWorktree"], true);
        assert_eq!(value["workspaceStatus"], "in-progress");
        assert_eq!(value["lastActivityAt"], 0);
        assert_eq!(
            value,
            json!({
                "id": "r1::/repo",
                "repoId": "r1",
                "displayName": "Repo",
                "displayNameMode": "automatic",
                "comment": "",
                "linkedIssue": null,
                "linkedPR": null,
                "linkedLinearIssue": null,
                "isArchived": false,
                "isUnread": false,
                "isPinned": false,
                "sortOrder": 0,
                "lastActivityAt": 0,
                "path": "/repo",
                "head": "",
                "branch": "",
                "isBare": false,
                "isMainWorktree": true,
                "workspaceStatus": "in-progress"
            })
        );
    }
}
