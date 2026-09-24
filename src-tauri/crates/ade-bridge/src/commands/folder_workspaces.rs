use ade_core::models::folder_workspace::{
    assert_folder_workspace_path_usable, folder_workspace_path_status_for_path,
    new_folder_workspace, normalize_folder_workspace_name, FolderWorkspacePathStatus,
};
use ade_core::models::repo::now_ms;
use ade_core::path_compare::normalize_for_comparison;
use ade_fs::FsService;
use ade_store::projects_store::ProjectsStore;
use serde::Deserialize;
use serde_json::{Map, Value};
use tauri::State;

use crate::commands::project_groups::revoke_root_if_unused;
use crate::errors::BridgeError;
use crate::events;
use crate::json::Json;
use crate::state::{lock, AppState};

/// Fields `folder_workspaces_update` accepts (oracle
/// `FolderWorkspaceUpdateArgs`).
pub const FOLDER_WORKSPACE_UPDATE_FIELDS: &[&str] = &[
    "name",
    "folderPath",
    "linkedTask",
    "linkedTaskSourceContext",
    "comment",
    "isArchived",
    "isUnread",
    "isPinned",
    "sortOrder",
    "manualOrder",
    "workspaceStatus",
    "createdWithAgent",
    "pendingFirstAgentMessageRename",
    "firstAgentMessageRenameError",
    "lastActivityAt",
    "diffComments",
];

fn group_id(group: &Value) -> &str {
    group.get("id").and_then(Value::as_str).unwrap_or_default()
}

fn workspace_id(workspace: &Value) -> &str {
    workspace
        .get("id")
        .and_then(Value::as_str)
        .unwrap_or_default()
}

fn workspace_name(workspace: &Value) -> &str {
    workspace
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or_default()
}

fn group_name(group: &Value) -> &str {
    group
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or_default()
}

fn find_workspace(store: &ProjectsStore, workspace_id_value: &str) -> Option<Value> {
    store
        .folder_workspaces()
        .into_iter()
        .find(|workspace| workspace_id(workspace) == workspace_id_value)
}

#[derive(Debug, Clone, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct FolderWorkspacesCreateArgs {
    pub project_group_id: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub folder_path: Option<String>,
    #[serde(default)]
    pub connection_id: Option<String>,
    #[serde(default)]
    pub linked_task: Option<Json>,
    #[serde(default)]
    pub linked_task_source_context: Option<Json>,
    #[serde(default)]
    pub created_with_agent: Option<String>,
    #[serde(default)]
    pub pending_first_agent_message_rename: Option<bool>,
}

#[derive(Debug, Clone, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct FolderWorkspacesUpdateArgs {
    pub folder_workspace_id: String,
    pub updates: Json,
}

#[derive(Debug, Clone, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct FolderWorkspacesDeleteArgs {
    pub folder_workspace_id: String,
}

#[derive(Debug, Clone, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct FolderWorkspacesGetPathStatusArgs {
    pub scope: String,
    #[serde(default)]
    pub folder_workspace_id: Option<String>,
    #[serde(default)]
    pub project_group_id: Option<String>,
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub connection_id: Option<String>,
}

/// `folderWorkspaces.list`: sorted by `sortOrder` descending then name (oracle
/// `getFolderWorkspaces`).
pub fn list_folder_workspaces(store: &ProjectsStore) -> Vec<Value> {
    let mut workspaces = store.folder_workspaces();
    workspaces.sort_by(|left, right| {
        let left_order = left
            .get("sortOrder")
            .and_then(Value::as_f64)
            .unwrap_or_default();
        let right_order = right
            .get("sortOrder")
            .and_then(Value::as_f64)
            .unwrap_or_default();
        right_order
            .partial_cmp(&left_order)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| workspace_name(left).cmp(workspace_name(right)))
    });
    workspaces
}

fn authorize_root(fs: &FsService, path: &str) {
    if let Err(error) = fs.authorize_root(path) {
        eprintln!("[ade-bridge] failed to authorize folder workspace root '{path}': {error}");
    }
}

/// Validate the optional create-time fields the composer sends; wrong types
/// reject the whole request (oracle zod `FolderWorkspaceCreateArgs`).
fn sanitize_create_extras(
    args: &FolderWorkspacesCreateArgs,
) -> Result<Map<String, Value>, BridgeError> {
    let mut extras = Map::new();
    if let Some(linked_task) = &args.linked_task {
        if !linked_task.0.is_null() && !linked_task.0.is_object() {
            return Err(BridgeError::message("invalid_folder_workspace_create_args"));
        }
        extras.insert("linkedTask".to_string(), linked_task.0.clone());
    }
    if let Some(source_context) = &args.linked_task_source_context {
        if !source_context.0.is_null() && !source_context.0.is_object() {
            return Err(BridgeError::message("invalid_folder_workspace_create_args"));
        }
        extras.insert(
            "linkedTaskSourceContext".to_string(),
            source_context.0.clone(),
        );
    }
    if let Some(agent) = &args.created_with_agent {
        extras.insert("createdWithAgent".to_string(), Value::String(agent.clone()));
    }
    // Why: the pending rename badge only means something when an agent was
    // requested (oracle `createFolderWorkspace`).
    if let Some(pending) = args.pending_first_agent_message_rename {
        if args.created_with_agent.is_some() {
            extras.insert(
                "pendingFirstAgentMessageRename".to_string(),
                Value::Bool(pending),
            );
        }
    }
    Ok(extras)
}

/// `folderWorkspaces.create` (spec §5.2): the group must exist, the effective
/// folder path (`folderPath ?? group.parentPath`) must be usable, and the new
/// row gets the oracle defaults plus an authorized fs root.
pub fn create_folder_workspace(
    store: &mut ProjectsStore,
    fs: &FsService,
    args: &FolderWorkspacesCreateArgs,
    now: u64,
) -> Result<Value, BridgeError> {
    let groups = store.project_groups();
    let group = groups
        .iter()
        .find(|group| group_id(group) == args.project_group_id)
        .ok_or_else(|| BridgeError::message("folder_workspace_project_group_not_found"))?;
    let folder_path = args
        .folder_path
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .or_else(|| {
            group
                .get("parentPath")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
        })
        .ok_or_else(|| BridgeError::message("folder_workspace_project_group_not_found"))?;
    let connection_id = args.connection_id.clone().or_else(|| {
        group
            .get("connectionId")
            .and_then(Value::as_str)
            .map(str::to_string)
    });

    let status = folder_workspace_path_status_for_path(
        &folder_path,
        Some(&args.project_group_id),
        connection_id.as_deref(),
        &groups,
        &store.repos(),
    );
    if let Err(message) = assert_folder_workspace_path_usable(&status) {
        return Err(BridgeError::message(message));
    }

    let extras = sanitize_create_extras(args)?;
    // Why: the fallback is `<group name> workspace`, not the model default
    // (oracle `createFolderWorkspace`).
    let name = normalize_folder_workspace_name(
        args.name.as_deref(),
        &format!("{} workspace", group_name(group)),
    );
    let mut workspace = new_folder_workspace(
        &ade_core::ids::new_uuid(),
        &args.project_group_id,
        &name,
        &folder_path,
        connection_id.as_deref(),
        now,
        now,
    );
    if let Some(map) = workspace.as_object_mut() {
        for (key, value) in extras {
            map.insert(key, value);
        }
    }
    store.mutate_folder_workspaces(|workspaces| workspaces.insert(0, workspace.clone()))?;
    authorize_root(fs, &folder_path);
    Ok(workspace)
}

fn finite_number(value: &Value) -> Option<Value> {
    if let Some(integer) = value.as_i64() {
        return Some(Value::from(integer));
    }
    value
        .as_f64()
        .filter(|number| number.is_finite())
        .map(Value::from)
}

/// Keep only contract fields with valid types (oracle zod
/// `FolderWorkspaceUpdateArgs`); a blank `folderPath` is a zod-valid no-op.
pub fn sanitize_folder_workspace_updates(
    updates: &Value,
) -> Result<Map<String, Value>, BridgeError> {
    let invalid = || BridgeError::message("invalid_folder_workspace_update_args");
    let mut sanitized = Map::new();
    let Some(input) = updates.as_object() else {
        return Err(invalid());
    };
    for field in FOLDER_WORKSPACE_UPDATE_FIELDS {
        let Some(value) = input.get(*field) else {
            continue;
        };
        let accepted = match *field {
            "name" | "comment" | "workspaceStatus" | "createdWithAgent" => {
                value.as_str().map(|text| Value::String(text.to_string()))
            }
            "folderPath" => {
                let Some(text) = value.as_str() else {
                    return Err(invalid());
                };
                if text.trim().is_empty() {
                    continue;
                }
                Some(Value::String(text.to_string()))
            }
            "linkedTask" | "linkedTaskSourceContext" => {
                if value.is_null() || value.is_object() {
                    Some(value.clone())
                } else {
                    None
                }
            }
            "isArchived" | "isUnread" | "isPinned" | "pendingFirstAgentMessageRename" => {
                value.as_bool().map(Value::Bool)
            }
            "sortOrder" | "lastActivityAt" => finite_number(value),
            "manualOrder" => {
                if value.is_null() {
                    Some(Value::Null)
                } else {
                    finite_number(value)
                }
            }
            "firstAgentMessageRenameError" => {
                if value.is_null() {
                    Some(Value::Null)
                } else {
                    value.as_str().map(|text| Value::String(text.to_string()))
                }
            }
            "diffComments" => {
                if value.is_array() {
                    Some(value.clone())
                } else {
                    None
                }
            }
            _ => None,
        };
        let Some(accepted) = accepted else {
            return Err(invalid());
        };
        sanitized.insert((*field).to_string(), accepted);
    }
    Ok(sanitized)
}

/// Merge sanitized updates into one workspace row; `name` normalizes against
/// the current name and `manualOrder: null` removes the field (oracle
/// `updateFolderWorkspace`).
pub fn apply_folder_workspace_updates(workspace: &mut Value, updates: &Map<String, Value>) {
    let Some(map) = workspace.as_object_mut() else {
        return;
    };
    for (key, value) in updates {
        match key.as_str() {
            "name" => {
                let fallback = map
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string();
                let name = normalize_folder_workspace_name(
                    Some(value.as_str().unwrap_or_default()),
                    &fallback,
                );
                map.insert("name".to_string(), Value::String(name));
            }
            "manualOrder" if value.is_null() => {
                map.remove("manualOrder");
            }
            _ => {
                map.insert(key.clone(), value.clone());
            }
        }
    }
}

/// `folderWorkspaces.update`: `None` when the workspace is gone. A new
/// `folderPath` is pre-validated and re-authorized; the old root is revoked
/// only when nothing else still points at it.
pub fn update_folder_workspace(
    store: &mut ProjectsStore,
    fs: &FsService,
    workspace_id_value: &str,
    updates: &Value,
    now: u64,
) -> Result<Option<Value>, BridgeError> {
    if workspace_id_value.is_empty() {
        return Err(BridgeError::message("invalid_folder_workspace_update_args"));
    }
    let sanitized = sanitize_folder_workspace_updates(updates)?;
    let Some(existing) = find_workspace(store, workspace_id_value) else {
        return Ok(None);
    };
    if let Some(new_path) = sanitized.get("folderPath").and_then(Value::as_str) {
        let groups = store.project_groups();
        let group_id_value = existing
            .get("projectGroupId")
            .and_then(Value::as_str)
            .map(str::to_string);
        let group_connection = group_id_value
            .as_deref()
            .and_then(|group_id_value| {
                groups
                    .iter()
                    .find(|group| group_id(group) == group_id_value)
            })
            .and_then(|group| group.get("connectionId"))
            .and_then(Value::as_str)
            .map(str::to_string);
        let connection_id = existing
            .get("connectionId")
            .and_then(Value::as_str)
            .map(str::to_string)
            .or(group_connection);
        let status = folder_workspace_path_status_for_path(
            new_path,
            group_id_value.as_deref(),
            connection_id.as_deref(),
            &groups,
            &store.repos(),
        );
        if let Err(message) = assert_folder_workspace_path_usable(&status) {
            return Err(BridgeError::message(message));
        }
    }
    let old_path = existing
        .get("folderPath")
        .and_then(Value::as_str)
        .map(str::to_string);
    let workspaces = store.mutate_folder_workspaces(|workspaces| {
        if let Some(workspace) = workspaces
            .iter_mut()
            .find(|workspace| workspace_id(workspace) == workspace_id_value)
        {
            apply_folder_workspace_updates(workspace, &sanitized);
            if let Some(map) = workspace.as_object_mut() {
                map.insert("updatedAt".to_string(), Value::from(now));
            }
        }
    })?;
    let updated = workspaces
        .into_iter()
        .find(|workspace| workspace_id(workspace) == workspace_id_value);
    if let Some(new_path) = sanitized.get("folderPath").and_then(Value::as_str) {
        if let Some(old_path) = old_path {
            if normalize_for_comparison(&old_path) != normalize_for_comparison(new_path) {
                authorize_root(fs, new_path);
                revoke_root_if_unused(store, fs, &old_path);
            }
        }
    }
    Ok(updated)
}

/// `folderWorkspaces.delete`: `false` when the workspace is gone; a successful
/// delete revokes the folder root once nothing else uses it.
pub fn delete_folder_workspace(
    store: &mut ProjectsStore,
    fs: &FsService,
    workspace_id_value: &str,
) -> Result<bool, BridgeError> {
    if workspace_id_value.is_empty() {
        return Err(BridgeError::message("invalid_folder_workspace_delete_args"));
    }
    let Some(existing) = find_workspace(store, workspace_id_value) else {
        return Ok(false);
    };
    store.mutate_folder_workspaces(|workspaces| {
        workspaces.retain(|workspace| workspace_id(workspace) != workspace_id_value)
    })?;
    if let Some(path) = existing.get("folderPath").and_then(Value::as_str) {
        revoke_root_if_unused(store, fs, path);
    }
    Ok(true)
}

/// Resolve the three `getPathStatus` scopes to `(path, groupId, connectionId)`
/// (oracle `resolveFolderWorkspaceStatusPath`).
pub fn resolve_path_status_scope(
    store: &ProjectsStore,
    args: &FolderWorkspacesGetPathStatusArgs,
) -> Result<(String, Option<String>, Option<String>), BridgeError> {
    let invalid = || BridgeError::message("invalid_folder_workspace_path_status_args");
    match args.scope.as_str() {
        "folder-workspace" => {
            let workspace_id_value = args
                .folder_workspace_id
                .as_deref()
                .filter(|value| !value.is_empty())
                .ok_or_else(invalid)?;
            let workspace = find_workspace(store, workspace_id_value)
                .ok_or_else(|| BridgeError::message("folder_workspace_path_scope_not_found"))?;
            let group_id_value = workspace
                .get("projectGroupId")
                .and_then(Value::as_str)
                .map(str::to_string);
            let group_connection = group_id_value
                .as_deref()
                .and_then(|group_id_value| {
                    store
                        .project_groups()
                        .into_iter()
                        .find(|group| group_id(group) == group_id_value)
                })
                .and_then(|group| {
                    group
                        .get("connectionId")
                        .and_then(Value::as_str)
                        .map(str::to_string)
                });
            let connection_id = workspace
                .get("connectionId")
                .and_then(Value::as_str)
                .map(str::to_string)
                .or(group_connection);
            let path = workspace
                .get("folderPath")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            Ok((path, group_id_value, connection_id))
        }
        "project-group" => {
            let group_id_value = args
                .project_group_id
                .as_deref()
                .filter(|value| !value.is_empty())
                .ok_or_else(invalid)?;
            let group = store
                .project_groups()
                .into_iter()
                .find(|group| group_id(group) == group_id_value)
                .ok_or_else(|| BridgeError::message("folder_workspace_path_scope_not_found"))?;
            let path = group
                .get("parentPath")
                .and_then(Value::as_str)
                .filter(|value| !value.is_empty())
                .ok_or_else(|| BridgeError::message("folder_workspace_path_scope_not_found"))?
                .to_string();
            let connection_id = group
                .get("connectionId")
                .and_then(Value::as_str)
                .map(str::to_string);
            Ok((path, Some(group_id_value.to_string()), connection_id))
        }
        "path" => {
            let path = args
                .path
                .as_deref()
                .filter(|value| !value.is_empty())
                .ok_or_else(invalid)?;
            Ok((path.to_string(), None, args.connection_id.clone()))
        }
        _ => Err(invalid()),
    }
}

/// `folderWorkspaces.getPathStatus` (oracle `getFolderWorkspacePathStatus`).
pub fn get_path_status(
    store: &ProjectsStore,
    args: &FolderWorkspacesGetPathStatusArgs,
) -> Result<FolderWorkspacePathStatus, BridgeError> {
    let (path, group_id_value, connection_id) = resolve_path_status_scope(store, args)?;
    Ok(folder_workspace_path_status_for_path(
        &path,
        group_id_value.as_deref(),
        connection_id.as_deref(),
        &store.project_groups(),
        &store.repos(),
    ))
}

/// Read the projects registry's folder workspaces (spec §5.2).
#[tauri::command]
#[specta::specta]
pub async fn folder_workspaces_list(state: State<'_, AppState>) -> Result<Json, BridgeError> {
    Ok(Json::new(Value::Array(list_folder_workspaces(&lock(
        &state.projects,
    )))))
}

/// Create one folder workspace; requires a usable folder path.
#[tauri::command]
#[specta::specta]
pub async fn folder_workspaces_create(
    state: State<'_, AppState>,
    args: FolderWorkspacesCreateArgs,
) -> Result<Json, BridgeError> {
    let workspace = {
        let mut projects = lock(&state.projects);
        create_folder_workspace(&mut projects, &state.fs, &args, now_ms())?
    };
    events::emit_repos_changed(&state.app);
    Ok(Json::new(workspace))
}

/// Update the contract-allowed fields of one folder workspace.
#[tauri::command]
#[specta::specta]
pub async fn folder_workspaces_update(
    state: State<'_, AppState>,
    args: FolderWorkspacesUpdateArgs,
) -> Result<Json, BridgeError> {
    let updated = {
        let mut projects = lock(&state.projects);
        update_folder_workspace(
            &mut projects,
            &state.fs,
            &args.folder_workspace_id,
            &args.updates.0,
            now_ms(),
        )?
    };
    match updated {
        Some(workspace) => {
            events::emit_repos_changed(&state.app);
            Ok(Json::new(workspace))
        }
        None => Ok(Json::new(Value::Null)),
    }
}

/// Delete one folder workspace; revokes its folder root.
#[tauri::command]
#[specta::specta]
pub async fn folder_workspaces_delete(
    state: State<'_, AppState>,
    args: FolderWorkspacesDeleteArgs,
) -> Result<bool, BridgeError> {
    let deleted = {
        let mut projects = lock(&state.projects);
        delete_folder_workspace(&mut projects, &state.fs, &args.folder_workspace_id)?
    };
    if deleted {
        events::emit_repos_changed(&state.app);
    }
    Ok(deleted)
}

/// Stat one folder path for the three contract scopes; TTL caching is the
/// renderer's job (spec §5.2).
#[tauri::command]
#[specta::specta]
pub async fn folder_workspaces_get_path_status(
    state: State<'_, AppState>,
    args: FolderWorkspacesGetPathStatusArgs,
) -> Result<FolderWorkspacePathStatus, BridgeError> {
    get_path_status(&lock(&state.projects), &args)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ade_core::models::folder_workspace::FolderWorkspacePathStatusReason;
    use ade_core::models::project_group::{new_project_group, ProjectGroupCreatedFrom};
    use serde_json::json;

    struct TestDir {
        path: std::path::PathBuf,
    }

    impl TestDir {
        fn new(name: &str) -> Self {
            let path = std::env::temp_dir()
                .join(format!("ade-bridge-folders-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&path);
            std::fs::create_dir_all(&path).expect("create test dir");
            Self { path }
        }

        fn file(&self, name: &str) -> std::path::PathBuf {
            self.path.join(name)
        }

        fn dir(&self, name: &str) -> std::path::PathBuf {
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

    fn store(dir: &TestDir) -> ProjectsStore {
        ProjectsStore::load(dir.file("projects.json"))
    }

    fn add_group(store: &mut ProjectsStore, parent_path: Option<&str>) -> String {
        let group = new_project_group(
            &ade_core::ids::new_uuid(),
            "Group",
            parent_path,
            None,
            None,
            ProjectGroupCreatedFrom::Manual,
            0,
            1,
        );
        let id = group_id(&group).to_string();
        store.mutate_groups(|groups| groups.push(group)).unwrap();
        id
    }

    fn create_args(group_id_value: &str, folder_path: Option<&str>) -> FolderWorkspacesCreateArgs {
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
    fn create_requires_an_existing_group_and_a_path() {
        let dir = TestDir::new("create-validation");
        let fs = FsService::new();
        let mut store = store(&dir);
        assert_eq!(
            create_folder_workspace(&mut store, &fs, &create_args("missing", None), 1)
                .unwrap_err()
                .to_string(),
            "folder_workspace_project_group_not_found"
        );
        let group_id_value = add_group(&mut store, None);
        assert_eq!(
            create_folder_workspace(&mut store, &fs, &create_args(&group_id_value, None), 1)
                .unwrap_err()
                .to_string(),
            "folder_workspace_project_group_not_found"
        );
    }

    #[test]
    fn create_validates_the_path_status() {
        let dir = TestDir::new("create-path");
        let fs = FsService::new();
        let mut store = store(&dir);
        let group_id_value = add_group(&mut store, Some(dir.dir("group").to_str().unwrap()));

        let missing = dir.path.join("missing");
        assert_eq!(
            create_folder_workspace(
                &mut store,
                &fs,
                &create_args(&group_id_value, missing.to_str()),
                1
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
                &mut store,
                &fs,
                &create_args(&group_id_value, file.to_str()),
                1
            )
            .unwrap_err()
            .to_string(),
            format!(
                "folder_workspace_path_not_directory:{}",
                file.to_str().unwrap()
            )
        );
    }

    #[test]
    fn create_uses_defaults_and_authorizes_the_root() {
        let dir = TestDir::new("create-defaults");
        let fs = FsService::new();
        let mut store = store(&dir);
        let parent = dir.dir("group");
        let group_id_value = add_group(&mut store, Some(parent.to_str().unwrap()));

        let workspace =
            create_folder_workspace(&mut store, &fs, &create_args(&group_id_value, None), 42)
                .unwrap();
        assert_eq!(workspace["projectGroupId"], group_id_value);
        assert_eq!(workspace["name"], "Group workspace");
        assert_eq!(workspace["folderPath"], parent.to_str().unwrap());
        assert_eq!(workspace["connectionId"], Value::Null);
        assert_eq!(workspace["creatorProvenance"], json!({ "kind": "host" }));
        assert_eq!(workspace["comment"], "");
        assert_eq!(workspace["sortOrder"], 42);
        assert_eq!(workspace["isArchived"], false);
        assert_eq!(workspace["lastActivityAt"], 0);
        assert_eq!(workspace["createdAt"], 42);
        assert_eq!(store.folder_workspaces().len(), 1);
        assert!(fs.resolve(parent.to_str().unwrap()).is_ok());

        let named = create_folder_workspace(
            &mut store,
            &fs,
            &FolderWorkspacesCreateArgs {
                name: Some("  Custom  ".to_string()),
                ..create_args(&group_id_value, None)
            },
            43,
        )
        .unwrap();
        assert_eq!(named["name"], "Custom");
    }

    #[test]
    fn create_stores_the_trimmed_folder_path() {
        let dir = TestDir::new("create-trimmed-path");
        let fs = FsService::new();
        let mut store = store(&dir);
        let group_id_value = add_group(&mut store, None);
        let parent = dir.dir("parent");
        let padded = format!("  {}  ", parent.to_str().unwrap());

        let workspace = create_folder_workspace(
            &mut store,
            &fs,
            &create_args(&group_id_value, Some(&padded)),
            1,
        )
        .unwrap();

        assert_eq!(workspace["folderPath"], parent.to_str().unwrap());
    }

    #[test]
    fn create_accepts_but_validates_optional_extras() {
        let dir = TestDir::new("create-extras");
        let fs = FsService::new();
        let mut store = store(&dir);
        let parent = dir.dir("group");
        let group_id_value = add_group(&mut store, Some(parent.to_str().unwrap()));

        let workspace = create_folder_workspace(
            &mut store,
            &fs,
            &FolderWorkspacesCreateArgs {
                linked_task: Some(Json::new(json!({ "kind": "issue", "id": "1" }))),
                created_with_agent: Some("claude".to_string()),
                pending_first_agent_message_rename: Some(true),
                ..create_args(&group_id_value, None)
            },
            1,
        )
        .unwrap();
        assert_eq!(
            workspace["linkedTask"],
            json!({ "kind": "issue", "id": "1" })
        );
        assert_eq!(workspace["createdWithAgent"], "claude");
        assert_eq!(workspace["pendingFirstAgentMessageRename"], true);

        let error = create_folder_workspace(
            &mut store,
            &fs,
            &FolderWorkspacesCreateArgs {
                linked_task: Some(Json::new(json!("nope"))),
                ..create_args(&group_id_value, None)
            },
            2,
        )
        .unwrap_err();
        assert_eq!(error.to_string(), "invalid_folder_workspace_create_args");
    }

    #[test]
    fn update_normalizes_and_validates() {
        let dir = TestDir::new("update");
        let fs = FsService::new();
        let mut store = store(&dir);
        let parent = dir.dir("group");
        let group_id_value = add_group(&mut store, Some(parent.to_str().unwrap()));
        let workspace =
            create_folder_workspace(&mut store, &fs, &create_args(&group_id_value, None), 1)
                .unwrap();
        let id = workspace_id(&workspace).to_string();

        let updated = update_folder_workspace(
            &mut store,
            &fs,
            &id,
            &json!({ "name": "  Renamed  ", "comment": "note", "isPinned": true, "sortOrder": 5 }),
            9,
        )
        .unwrap()
        .expect("workspace exists");
        assert_eq!(updated["name"], "Renamed");
        assert_eq!(updated["comment"], "note");
        assert_eq!(updated["isPinned"], true);
        assert_eq!(updated["sortOrder"], 5);
        assert_eq!(updated["updatedAt"], 9);

        let cleared = update_folder_workspace(
            &mut store,
            &fs,
            &id,
            &json!({ "name": "   ", "manualOrder": null }),
            10,
        )
        .unwrap()
        .expect("workspace exists");
        assert_eq!(cleared["name"], "Renamed");
        assert!(cleared.get("manualOrder").is_none());

        assert_eq!(
            update_folder_workspace(&mut store, &fs, &id, &json!({ "name": 7 }), 11)
                .unwrap_err()
                .to_string(),
            "invalid_folder_workspace_update_args"
        );
        assert!(
            update_folder_workspace(&mut store, &fs, "missing", &json!({}), 12)
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn update_rejects_an_unusable_new_path_and_reauthorizes() {
        let dir = TestDir::new("update-path");
        let fs = FsService::new();
        let mut store = store(&dir);
        let first = dir.dir("first");
        let second = dir.dir("second");
        let group_id_value = add_group(&mut store, Some(first.to_str().unwrap()));
        let workspace =
            create_folder_workspace(&mut store, &fs, &create_args(&group_id_value, None), 1)
                .unwrap();
        let id = workspace_id(&workspace).to_string();

        let missing = dir.path.join("missing");
        assert_eq!(
            update_folder_workspace(
                &mut store,
                &fs,
                &id,
                &json!({ "folderPath": missing.to_str().unwrap() }),
                2
            )
            .unwrap_err()
            .to_string(),
            format!(
                "folder_workspace_path_missing:{}",
                missing.to_str().unwrap()
            )
        );

        let updated = update_folder_workspace(
            &mut store,
            &fs,
            &id,
            &json!({ "folderPath": second.to_str().unwrap() }),
            3,
        )
        .unwrap()
        .expect("workspace exists");
        assert_eq!(updated["folderPath"], second.to_str().unwrap());
        assert!(fs.resolve(second.to_str().unwrap()).is_ok());
        assert!(
            matches!(
                fs.resolve(first.to_str().unwrap()),
                Err(ade_fs::FsError::PathAccessDenied)
            ),
            "the old root is revoked once unused"
        );
    }

    #[test]
    fn delete_returns_false_for_missing_and_revokes_the_root() {
        let dir = TestDir::new("delete");
        let fs = FsService::new();
        let mut store = store(&dir);
        let parent = dir.dir("group");
        let group_id_value = add_group(&mut store, Some(parent.to_str().unwrap()));
        let workspace =
            create_folder_workspace(&mut store, &fs, &create_args(&group_id_value, None), 1)
                .unwrap();
        let id = workspace_id(&workspace).to_string();

        assert!(delete_folder_workspace(&mut store, &fs, &id).unwrap());
        assert!(store.folder_workspaces().is_empty());
        assert!(matches!(
            fs.resolve(parent.to_str().unwrap()),
            Err(ade_fs::FsError::PathAccessDenied)
        ));
        assert!(!delete_folder_workspace(&mut store, &fs, &id).unwrap());
    }

    #[test]
    fn path_status_resolves_all_three_scopes() {
        let dir = TestDir::new("status-scopes");
        let fs = FsService::new();
        let mut store = store(&dir);
        let parent = dir.dir("group");
        let group_id_value = add_group(&mut store, Some(parent.to_str().unwrap()));
        let workspace =
            create_folder_workspace(&mut store, &fs, &create_args(&group_id_value, None), 1)
                .unwrap();
        let id = workspace_id(&workspace).to_string();

        let by_workspace = get_path_status(
            &store,
            &FolderWorkspacesGetPathStatusArgs {
                scope: "folder-workspace".to_string(),
                folder_workspace_id: Some(id),
                project_group_id: None,
                path: None,
                connection_id: None,
            },
        )
        .unwrap();
        assert!(by_workspace.exists);
        assert_eq!(by_workspace.path, parent.to_str().unwrap());

        let by_group = get_path_status(
            &store,
            &FolderWorkspacesGetPathStatusArgs {
                scope: "project-group".to_string(),
                folder_workspace_id: None,
                project_group_id: Some(group_id_value),
                path: None,
                connection_id: None,
            },
        )
        .unwrap();
        assert!(by_group.exists);

        let by_path = get_path_status(
            &store,
            &FolderWorkspacesGetPathStatusArgs {
                scope: "path".to_string(),
                folder_workspace_id: None,
                project_group_id: None,
                path: Some(parent.to_str().unwrap().to_string()),
                connection_id: None,
            },
        )
        .unwrap();
        assert!(by_path.exists);

        let missing = get_path_status(
            &store,
            &FolderWorkspacesGetPathStatusArgs {
                scope: "path".to_string(),
                folder_workspace_id: None,
                project_group_id: None,
                path: Some(dir.path.join("missing").to_str().unwrap().to_string()),
                connection_id: None,
            },
        )
        .unwrap();
        assert!(!missing.exists);
        assert_eq!(
            missing.reason,
            Some(FolderWorkspacePathStatusReason::Missing)
        );

        assert_eq!(
            get_path_status(
                &store,
                &FolderWorkspacesGetPathStatusArgs {
                    scope: "project-group".to_string(),
                    folder_workspace_id: None,
                    project_group_id: Some("missing".to_string()),
                    path: None,
                    connection_id: None,
                }
            )
            .unwrap_err()
            .to_string(),
            "folder_workspace_path_scope_not_found"
        );
        assert_eq!(
            get_path_status(
                &store,
                &FolderWorkspacesGetPathStatusArgs {
                    scope: "nonsense".to_string(),
                    folder_workspace_id: None,
                    project_group_id: None,
                    path: None,
                    connection_id: None,
                }
            )
            .unwrap_err()
            .to_string(),
            "invalid_folder_workspace_path_status_args"
        );
    }

    #[test]
    fn list_sorts_by_sort_order_then_name() {
        let dir = TestDir::new("list");
        let mut store = store(&dir);
        store
            .mutate_folder_workspaces(|workspaces| {
                workspaces.push(json!({ "id": "a", "name": "Alpha", "sortOrder": 1 }));
                workspaces.push(json!({ "id": "b", "name": "Beta", "sortOrder": 2 }));
                workspaces.push(json!({ "id": "c", "name": "Aardvark", "sortOrder": 1 }));
            })
            .unwrap();
        let listed = list_folder_workspaces(&store);
        assert_eq!(
            listed.iter().map(workspace_id).collect::<Vec<_>>(),
            vec!["b", "c", "a"]
        );
    }
}
