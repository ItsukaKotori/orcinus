use std::collections::HashSet;
use std::path::PathBuf;

use serde_json::{Map, Value};

use crate::json_file::JsonFile;
use crate::{StoreError, SCHEMA_VERSION};

const REPOS: &str = "repos";
const PROJECT_GROUPS: &str = "projectGroups";
const FOLDER_WORKSPACES: &str = "folderWorkspaces";

pub struct ProjectsStore {
    file: JsonFile,
    current: Map<String, Value>,
}

impl ProjectsStore {
    pub fn load(path: impl Into<PathBuf>) -> Self {
        let file = JsonFile::new(path);
        let current = normalize_state(file.load());
        Self { file, current }
    }

    pub fn schema_version(&self) -> u64 {
        self.current
            .get("schemaVersion")
            .and_then(Value::as_u64)
            .unwrap_or(SCHEMA_VERSION)
    }

    pub fn repos(&self) -> Vec<Value> {
        collection(&self.current, REPOS)
    }

    pub fn project_groups(&self) -> Vec<Value> {
        collection(&self.current, PROJECT_GROUPS)
    }

    pub fn folder_workspaces(&self) -> Vec<Value> {
        collection(&self.current, FOLDER_WORKSPACES)
    }

    pub fn mutate_repos(
        &mut self,
        mutate: impl FnOnce(&mut Vec<Value>),
    ) -> Result<Vec<Value>, StoreError> {
        self.mutate_collection(REPOS, mutate)
    }

    pub fn mutate_groups(
        &mut self,
        mutate: impl FnOnce(&mut Vec<Value>),
    ) -> Result<Vec<Value>, StoreError> {
        self.mutate_collection(PROJECT_GROUPS, mutate)
    }

    pub fn mutate_folder_workspaces(
        &mut self,
        mutate: impl FnOnce(&mut Vec<Value>),
    ) -> Result<Vec<Value>, StoreError> {
        self.mutate_collection(FOLDER_WORKSPACES, mutate)
    }

    fn mutate_collection(
        &mut self,
        key: &str,
        mutate: impl FnOnce(&mut Vec<Value>),
    ) -> Result<Vec<Value>, StoreError> {
        let mut items = collection(&self.current, key);
        mutate(&mut items);
        dedupe_by_id(&mut items);
        self.current
            .insert(key.to_string(), Value::Array(items.clone()));
        self.file.save(&Value::Object(self.current.clone()))?;
        Ok(items)
    }
}

fn normalize_state(stored: Value) -> Map<String, Value> {
    let mut state = match stored {
        Value::Object(map) => map,
        _ => Map::new(),
    };
    let schema_version = state
        .get("schemaVersion")
        .and_then(Value::as_u64)
        .unwrap_or(SCHEMA_VERSION);
    state.insert("schemaVersion".to_string(), Value::from(schema_version));
    for key in [REPOS, PROJECT_GROUPS, FOLDER_WORKSPACES] {
        if !state.get(key).is_some_and(Value::is_array) {
            state.insert(key.to_string(), Value::Array(Vec::new()));
        }
    }
    state
}

fn collection(state: &Map<String, Value>, key: &str) -> Vec<Value> {
    state
        .get(key)
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
}

fn dedupe_by_id(items: &mut Vec<Value>) {
    let mut seen: HashSet<String> = HashSet::new();
    items.retain(|item| match item.get("id").and_then(Value::as_str) {
        Some(id) => seen.insert(id.to_string()),
        None => true,
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TestDir;
    use serde_json::json;

    fn load_store(dir: &TestDir) -> ProjectsStore {
        ProjectsStore::load(dir.file("projects.json"))
    }

    #[test]
    fn load_returns_default_structure_when_file_is_missing() {
        let dir = TestDir::new("projects-missing");
        let store = load_store(&dir);
        assert_eq!(store.schema_version(), 1);
        assert_eq!(store.repos(), Vec::<Value>::new());
        assert_eq!(store.project_groups(), Vec::<Value>::new());
        assert_eq!(store.folder_workspaces(), Vec::<Value>::new());
    }

    #[test]
    fn load_normalizes_missing_and_invalid_collections() {
        let dir = TestDir::new("projects-normalize");
        std::fs::write(
            dir.file("projects.json"),
            r#"{"schemaVersion":1,"repos":{},"projectGroups":null}"#,
        )
        .unwrap();
        let mut store = load_store(&dir);
        assert_eq!(store.repos(), Vec::<Value>::new());
        assert_eq!(store.project_groups(), Vec::<Value>::new());
        assert_eq!(store.folder_workspaces(), Vec::<Value>::new());

        store
            .mutate_groups(|groups| groups.push(json!({"id": "g1"})))
            .unwrap();
        let saved = JsonFile::new(dir.file("projects.json")).load();
        assert!(saved["repos"].is_array());
        assert!(saved["projectGroups"].is_array());
        assert!(saved["folderWorkspaces"].is_array());
    }

    #[test]
    fn mutate_repos_add_update_remove_round_trip() {
        let dir = TestDir::new("projects-repos-crud");
        let mut store = load_store(&dir);
        store
            .mutate_repos(|repos| repos.push(json!({"id": "r1", "path": "/repo/a"})))
            .unwrap();
        assert_eq!(store.repos().len(), 1);

        store
            .mutate_repos(|repos| {
                let repo = repos
                    .iter_mut()
                    .find(|repo| repo["id"] == "r1")
                    .expect("repo exists");
                repo["displayName"] = json!("Renamed");
            })
            .unwrap();
        assert_eq!(store.repos()[0]["displayName"], "Renamed");
        assert_eq!(store.repos()[0]["path"], "/repo/a");

        store
            .mutate_repos(|repos| repos.retain(|repo| repo["id"] != "r1"))
            .unwrap();
        assert_eq!(store.repos(), Vec::<Value>::new());
    }

    #[test]
    fn mutate_groups_round_trip() {
        let dir = TestDir::new("projects-groups-crud");
        let mut store = load_store(&dir);
        let groups = store
            .mutate_groups(|groups| groups.push(json!({"id": "g1", "name": "Group"})))
            .unwrap();
        assert_eq!(groups.len(), 1);
        assert_eq!(store.project_groups()[0]["name"], "Group");
        assert_eq!(store.repos(), Vec::<Value>::new());
    }

    #[test]
    fn mutate_folder_workspaces_round_trip() {
        let dir = TestDir::new("projects-folders-crud");
        let mut store = load_store(&dir);
        store
            .mutate_folder_workspaces(|workspaces| {
                workspaces.push(json!({"id": "w1", "projectGroupId": "g1"}))
            })
            .unwrap();
        assert_eq!(store.folder_workspaces().len(), 1);
        assert_eq!(store.folder_workspaces()[0]["projectGroupId"], "g1");
    }

    #[test]
    fn mutate_dedupes_ids_keeping_first() {
        let dir = TestDir::new("projects-dedupe");
        let mut store = load_store(&dir);
        store
            .mutate_repos(|repos| {
                repos.push(json!({"id": "r1", "path": "/first"}));
                repos.push(json!({"id": "r1", "path": "/second"}));
                repos.push(json!({"path": "/no-id"}));
            })
            .unwrap();
        let repos = store.repos();
        assert_eq!(repos.len(), 2);
        assert_eq!(repos[0]["path"], "/first");
        assert_eq!(repos[1]["path"], "/no-id");
    }

    #[test]
    fn mutations_persist_across_reload() {
        let dir = TestDir::new("projects-persist");
        let mut store = load_store(&dir);
        store
            .mutate_repos(|repos| repos.push(json!({"id": "r1", "path": "/repo/a"})))
            .unwrap();
        let reloaded = load_store(&dir);
        assert_eq!(reloaded.repos()[0]["id"], "r1");
        assert_eq!(reloaded.repos()[0]["path"], "/repo/a");
    }

    #[test]
    fn saved_file_keeps_schema_version_and_unknown_keys() {
        let dir = TestDir::new("projects-forward-compat");
        std::fs::write(
            dir.file("projects.json"),
            r#"{"schemaVersion":1,"repos":[],"futureThing":{"x":1}}"#,
        )
        .unwrap();
        let mut store = load_store(&dir);
        store
            .mutate_repos(|repos| repos.push(json!({"id": "r1"})))
            .unwrap();
        let saved = JsonFile::new(dir.file("projects.json")).load();
        assert_eq!(saved["schemaVersion"], 1);
        assert_eq!(saved["futureThing"], json!({"x": 1}));
        assert_eq!(saved["repos"].as_array().unwrap().len(), 1);
    }
}
