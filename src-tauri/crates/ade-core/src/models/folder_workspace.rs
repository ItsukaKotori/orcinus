use std::collections::HashSet;
use std::io::ErrorKind;

use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

use super::project_group::project_group_subtree_ids;
use crate::path_compare::is_path_inside_or_equal;

/// Fallback used when a create/update name normalizes to blank (oracle
/// `normalizeFolderWorkspaceName`).
pub const DEFAULT_FOLDER_WORKSPACE_NAME: &str = "Untitled workspace";

/// Renderer-side cache TTL for `get_path_status` (oracle
/// `FOLDER_WORKSPACE_PATH_STATUS_TTL_MS`); Rust does not cache.
pub const FOLDER_WORKSPACE_PATH_STATUS_TTL_MS: u64 = 10_000;

/// `trim(name) || fallback` (oracle `normalizeFolderWorkspaceName`).
pub fn normalize_folder_workspace_name(name: Option<&str>, fallback: &str) -> String {
    let trimmed = name.unwrap_or_default().trim();
    if trimmed.is_empty() {
        fallback.to_string()
    } else {
        trimmed.to_string()
    }
}

/// Build a new folder workspace row (spec §5.2): `id`, `projectGroupId`, `name`,
/// `folderPath`, `connectionId`, `creatorProvenance={kind:'host'}`,
/// `linkedTask=null`, `comment=''`, `sortOrder`, flags false, timestamps.
#[allow(clippy::too_many_arguments)]
pub fn new_folder_workspace(
    id: &str,
    project_group_id: &str,
    name: &str,
    folder_path: &str,
    connection_id: Option<&str>,
    sort_order: u64,
    now: u64,
) -> Value {
    let mut workspace = Map::new();
    workspace.insert("id".to_string(), Value::String(id.to_string()));
    workspace.insert(
        "projectGroupId".to_string(),
        Value::String(project_group_id.to_string()),
    );
    workspace.insert(
        "name".to_string(),
        Value::String(normalize_folder_workspace_name(
            Some(name),
            DEFAULT_FOLDER_WORKSPACE_NAME,
        )),
    );
    workspace.insert(
        "folderPath".to_string(),
        Value::String(folder_path.to_string()),
    );
    workspace.insert(
        "connectionId".to_string(),
        match connection_id {
            Some(value) => Value::String(value.to_string()),
            None => Value::Null,
        },
    );
    workspace.insert("creatorProvenance".to_string(), json!({ "kind": "host" }));
    workspace.insert("linkedTask".to_string(), Value::Null);
    workspace.insert("linkedTaskSourceContext".to_string(), Value::Null);
    workspace.insert("comment".to_string(), Value::String(String::new()));
    workspace.insert("isArchived".to_string(), Value::Bool(false));
    workspace.insert("isUnread".to_string(), Value::Bool(false));
    workspace.insert("isPinned".to_string(), Value::Bool(false));
    workspace.insert("sortOrder".to_string(), Value::from(sort_order));
    workspace.insert("lastActivityAt".to_string(), Value::from(0));
    workspace.insert("createdAt".to_string(), Value::from(now));
    workspace.insert("updatedAt".to_string(), Value::from(now));
    Value::Object(workspace)
}

/// Why a folder workspace path is unusable (oracle
/// `FolderWorkspacePathStatusReason`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub enum FolderWorkspacePathStatusReason {
    Missing,
    NotDirectory,
    Unavailable,
    AmbiguousConnection,
}

impl FolderWorkspacePathStatusReason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Missing => "missing",
            Self::NotDirectory => "not-directory",
            Self::Unavailable => "unavailable",
            Self::AmbiguousConnection => "ambiguous-connection",
        }
    }
}

/// `getPathStatus` payload (oracle `FolderWorkspacePathStatus`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct FolderWorkspacePathStatus {
    pub path: String,
    pub exists: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<FolderWorkspacePathStatusReason>,
}

impl FolderWorkspacePathStatus {
    fn new(path: &str, exists: bool, reason: Option<FolderWorkspacePathStatusReason>) -> Self {
        Self {
            path: path.to_string(),
            exists,
            reason,
        }
    }
}

/// `assertFolderWorkspacePathUsable` (oracle): the exact error strings the
/// renderer maps back to its path-status reasons.
pub fn assert_folder_workspace_path_usable(
    status: &FolderWorkspacePathStatus,
) -> Result<(), String> {
    if status.exists {
        return Ok(());
    }
    let message = match status.reason {
        Some(FolderWorkspacePathStatusReason::Missing) => {
            format!("folder_workspace_path_missing:{}", status.path)
        }
        Some(FolderWorkspacePathStatusReason::NotDirectory) => {
            format!("folder_workspace_path_not_directory:{}", status.path)
        }
        Some(FolderWorkspacePathStatusReason::AmbiguousConnection) => {
            format!("folder_workspace_connection_ambiguous:{}", status.path)
        }
        _ => format!("folder_workspace_path_unavailable:{}", status.path),
    };
    Err(message)
}

/// Which connection a folder path belongs to (oracle
/// `FolderWorkspacePathConnectionResolution`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FolderWorkspacePathConnection {
    Local,
    Ssh(String),
    Ambiguous,
}

fn repo_connection(repo: &Value) -> Option<&str> {
    repo.get("connectionId")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
}

/// Repos considered when resolving a folder scope's connection: every repo in
/// the group subtree, plus repos whose path sits inside the folder that are not
/// already in the group (oracle `getFolderScopeCandidateRepos`).
fn folder_scope_candidate_repos<'a>(
    folder_path: &str,
    project_group_id: Option<&str>,
    connection_id: Option<&str>,
    groups: &[Value],
    repos: &'a [Value],
) -> Vec<&'a Value> {
    let group_ids = project_group_id.map(|group_id| project_group_subtree_ids(groups, group_id));
    let group_repo = |repo: &Value| -> bool {
        group_ids.as_ref().is_some_and(|ids| {
            repo.get("projectGroupId")
                .and_then(Value::as_str)
                .is_some_and(|group| ids.contains(group))
        })
    };
    let group_repos: Vec<&Value> = repos.iter().filter(|repo| group_repo(repo)).collect();
    let path_repos: Vec<&Value> = repos
        .iter()
        .filter(|repo| {
            !group_repo(repo)
                && repo
                    .get("path")
                    .and_then(Value::as_str)
                    .is_some_and(|path| is_path_inside_or_equal(folder_path, path))
        })
        .collect();

    let connection_id = connection_id.filter(|value| !value.is_empty());
    if let Some(connection_id) = connection_id {
        let mut candidates = group_repos;
        candidates.extend(
            path_repos
                .into_iter()
                .filter(|repo| repo_connection(repo) == Some(connection_id)),
        );
        return candidates;
    }
    if group_repos.is_empty() {
        return path_repos;
    }
    let group_connections: HashSet<Option<&str>> = group_repos
        .iter()
        .map(|repo| repo_connection(repo))
        .collect();
    let mut candidates = group_repos;
    candidates.extend(
        path_repos
            .into_iter()
            .filter(|repo| group_connections.contains(&repo_connection(repo))),
    );
    candidates
}

/// Port of the oracle's `inferFolderWorkspacePathConnection`: local unless every
/// candidate repo agrees on one SSH connection.
pub fn infer_folder_workspace_path_connection(
    folder_path: &str,
    project_group_id: Option<&str>,
    connection_id: Option<&str>,
    groups: &[Value],
    repos: &[Value],
) -> FolderWorkspacePathConnection {
    let candidates =
        folder_scope_candidate_repos(folder_path, project_group_id, connection_id, groups, repos);
    let mut has_local_repo = false;
    let mut connection_ids: Vec<&str> = Vec::new();
    for repo in &candidates {
        match repo
            .get("connectionId")
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
        {
            Some(value) => connection_ids.push(value),
            None => has_local_repo = true,
        }
    }

    let connection_id = connection_id.filter(|value| !value.is_empty());
    if let Some(connection_id) = connection_id {
        let conflicting = connection_ids
            .iter()
            .any(|existing| *existing != connection_id);
        if has_local_repo || conflicting {
            return FolderWorkspacePathConnection::Ambiguous;
        }
        return FolderWorkspacePathConnection::Ssh(connection_id.to_string());
    }
    if has_local_repo && !connection_ids.is_empty() {
        return FolderWorkspacePathConnection::Ambiguous;
    }
    if connection_ids.is_empty() {
        return FolderWorkspacePathConnection::Local;
    }
    if connection_ids.len() == 1 {
        return FolderWorkspacePathConnection::Ssh(connection_ids[0].to_string());
    }
    FolderWorkspacePathConnection::Ambiguous
}

/// Stat a folder path under its resolved connection. A has no SSH provider, so
/// SSH-scoped paths answer `unavailable` exactly like the oracle when the
/// provider is missing (oracle `statFolderPath`).
pub fn folder_workspace_path_status_for_path(
    folder_path: &str,
    project_group_id: Option<&str>,
    connection_id: Option<&str>,
    groups: &[Value],
    repos: &[Value],
) -> FolderWorkspacePathStatus {
    let connection = infer_folder_workspace_path_connection(
        folder_path,
        project_group_id,
        connection_id,
        groups,
        repos,
    );
    match connection {
        FolderWorkspacePathConnection::Ambiguous => FolderWorkspacePathStatus::new(
            folder_path,
            false,
            Some(FolderWorkspacePathStatusReason::AmbiguousConnection),
        ),
        FolderWorkspacePathConnection::Ssh(_) => FolderWorkspacePathStatus::new(
            folder_path,
            false,
            Some(FolderWorkspacePathStatusReason::Unavailable),
        ),
        FolderWorkspacePathConnection::Local => match std::fs::metadata(folder_path) {
            Ok(metadata) if metadata.is_dir() => {
                FolderWorkspacePathStatus::new(folder_path, true, None)
            }
            Ok(_) => FolderWorkspacePathStatus::new(
                folder_path,
                false,
                Some(FolderWorkspacePathStatusReason::NotDirectory),
            ),
            Err(error) => match error.kind() {
                ErrorKind::NotFound | ErrorKind::NotADirectory => FolderWorkspacePathStatus::new(
                    folder_path,
                    false,
                    Some(FolderWorkspacePathStatusReason::Missing),
                ),
                _ => FolderWorkspacePathStatus::new(
                    folder_path,
                    false,
                    Some(FolderWorkspacePathStatusReason::Unavailable),
                ),
            },
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn name_normalizes_to_trimmed_or_fallback() {
        assert_eq!(
            normalize_folder_workspace_name(Some("  Nice  "), "fallback"),
            "Nice"
        );
        assert_eq!(
            normalize_folder_workspace_name(Some("   "), "fallback"),
            "fallback"
        );
        assert_eq!(
            normalize_folder_workspace_name(None, "fallback"),
            "fallback"
        );
    }

    #[test]
    fn new_workspace_uses_spec_defaults() {
        let workspace = new_folder_workspace("w1", "g1", "  Notes  ", "/notes", None, 42, 7);
        assert_eq!(
            workspace,
            json!({
                "id": "w1",
                "projectGroupId": "g1",
                "name": "Notes",
                "folderPath": "/notes",
                "connectionId": null,
                "creatorProvenance": { "kind": "host" },
                "linkedTask": null,
                "linkedTaskSourceContext": null,
                "comment": "",
                "isArchived": false,
                "isUnread": false,
                "isPinned": false,
                "sortOrder": 42,
                "lastActivityAt": 0,
                "createdAt": 7,
                "updatedAt": 7
            })
        );
    }

    #[test]
    fn path_status_assert_messages_match_the_oracle() {
        let missing = FolderWorkspacePathStatus::new(
            "/gone",
            false,
            Some(FolderWorkspacePathStatusReason::Missing),
        );
        assert_eq!(
            assert_folder_workspace_path_usable(&missing).unwrap_err(),
            "folder_workspace_path_missing:/gone"
        );
        let file = FolderWorkspacePathStatus::new(
            "/file",
            false,
            Some(FolderWorkspacePathStatusReason::NotDirectory),
        );
        assert_eq!(
            assert_folder_workspace_path_usable(&file).unwrap_err(),
            "folder_workspace_path_not_directory:/file"
        );
        let ambiguous = FolderWorkspacePathStatus::new(
            "/mixed",
            false,
            Some(FolderWorkspacePathStatusReason::AmbiguousConnection),
        );
        assert_eq!(
            assert_folder_workspace_path_usable(&ambiguous).unwrap_err(),
            "folder_workspace_connection_ambiguous:/mixed"
        );
        let unavailable = FolderWorkspacePathStatus::new(
            "/ssh",
            false,
            Some(FolderWorkspacePathStatusReason::Unavailable),
        );
        assert_eq!(
            assert_folder_workspace_path_usable(&unavailable).unwrap_err(),
            "folder_workspace_path_unavailable:/ssh"
        );
        assert!(
            assert_folder_workspace_path_usable(&FolderWorkspacePathStatus::new("/ok", true, None))
                .is_ok()
        );
    }

    #[test]
    fn connection_inference_prefers_local_until_ssh_disagrees() {
        let groups = vec![json!({ "id": "g1" })];
        assert_eq!(
            infer_folder_workspace_path_connection("/root", Some("g1"), None, &groups, &[]),
            FolderWorkspacePathConnection::Local
        );

        let local_repo = json!({ "id": "r1", "path": "/root/a" });
        let ssh_repo = json!({ "id": "r2", "path": "/root/b", "connectionId": "box" });
        assert_eq!(
            infer_folder_workspace_path_connection(
                "/root",
                None,
                None,
                &groups,
                &[local_repo.clone(), ssh_repo.clone()]
            ),
            FolderWorkspacePathConnection::Ambiguous
        );
        assert_eq!(
            infer_folder_workspace_path_connection(
                "/root",
                None,
                Some("box"),
                &groups,
                std::slice::from_ref(&ssh_repo)
            ),
            FolderWorkspacePathConnection::Ssh("box".to_string())
        );
        // An explicit connection wins when no candidate repo contradicts it
        // (oracle: path repos on other connections are filtered out first).
        assert_eq!(
            infer_folder_workspace_path_connection(
                "/root",
                None,
                Some("other"),
                &groups,
                std::slice::from_ref(&ssh_repo)
            ),
            FolderWorkspacePathConnection::Ssh("other".to_string())
        );
        // ...but a local repo inside the group subtree makes it ambiguous.
        let group_local_repo = json!({ "id": "r3", "path": "/elsewhere", "projectGroupId": "g1" });
        assert_eq!(
            infer_folder_workspace_path_connection(
                "/root",
                Some("g1"),
                Some("other"),
                &groups,
                &[ssh_repo, group_local_repo]
            ),
            FolderWorkspacePathConnection::Ambiguous
        );
        assert_eq!(
            infer_folder_workspace_path_connection("/root", None, None, &groups, &[local_repo]),
            FolderWorkspacePathConnection::Local
        );
    }

    #[test]
    fn path_status_three_states_for_local_paths() {
        let dir = std::env::temp_dir().join(format!("ade-fw-status-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create dir");
        let file = dir.join("file.txt");
        std::fs::write(&file, "hi").expect("write file");

        let exists =
            folder_workspace_path_status_for_path(dir.to_str().unwrap(), None, None, &[], &[]);
        assert!(exists.exists);
        assert_eq!(exists.reason, None);

        let not_directory =
            folder_workspace_path_status_for_path(file.to_str().unwrap(), None, None, &[], &[]);
        assert!(!not_directory.exists);
        assert_eq!(
            not_directory.reason,
            Some(FolderWorkspacePathStatusReason::NotDirectory)
        );

        let missing_path = dir.join("missing");
        let missing = folder_workspace_path_status_for_path(
            missing_path.to_str().unwrap(),
            None,
            None,
            &[],
            &[],
        );
        assert!(!missing.exists);
        assert_eq!(
            missing.reason,
            Some(FolderWorkspacePathStatusReason::Missing)
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn path_status_serializes_without_reason_when_present() {
        let status = FolderWorkspacePathStatus::new("/ok", true, None);
        assert_eq!(
            serde_json::to_value(&status).unwrap(),
            json!({ "path": "/ok", "exists": true })
        );
        let missing = FolderWorkspacePathStatus::new(
            "/gone",
            false,
            Some(FolderWorkspacePathStatusReason::Missing),
        );
        assert_eq!(
            serde_json::to_value(&missing).unwrap(),
            json!({ "path": "/gone", "exists": false, "reason": "missing" })
        );
    }
}
