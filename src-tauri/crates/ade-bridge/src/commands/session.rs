//! Workspace session state persistence (spec §3.2): an opaque JSON document
//! store. The renderer is the only writer; Rust never mirrors the
//! `WorkspaceSessionState` type — each top-level field is one row.

use ade_store::sqlite::Store;
use serde_json::{Map, Value};

use crate::errors::BridgeError;

fn parse_object(text: &str) -> Result<Vec<(String, Value)>, BridgeError> {
    let parsed = serde_json::from_str::<Value>(text)
        .map_err(|error| BridgeError::message(format!("invalid session payload: {error}")))?;
    match parsed {
        Value::Object(map) => Ok(map.into_iter().collect()),
        _ => Err(BridgeError::message("session payload must be a JSON object")),
    }
}

/// `session_get` core: assemble every row into one JSON object text.
pub(crate) fn assemble_state_text(store: &Store) -> Result<String, BridgeError> {
    let mut object = Map::new();
    for (key, value) in store.all()? {
        // Values are written by us as valid JSON; a stray unreadable row degrades
        // to null instead of failing the whole restore.
        let parsed = serde_json::from_str::<Value>(&value).unwrap_or(Value::Null);
        object.insert(key, parsed);
    }
    Ok(Value::Object(object).to_string())
}

/// `session_patch` core: replace each present top-level key wholesale
/// (`WorkspaceSessionPatch = Partial<WorkspaceSessionState>` semantics).
pub(crate) fn apply_patch(store: &Store, payload: &str) -> Result<(), BridgeError> {
    for (key, value) in parse_object(payload)? {
        store.put(&key, &value.to_string())?;
    }
    Ok(())
}

/// `session_set` core: whole-state replace, deleting keys absent from the payload.
pub(crate) fn apply_set(store: &Store, payload: &str) -> Result<(), BridgeError> {
    let entries = parse_object(payload)?
        .into_iter()
        .map(|(key, value)| (key, value.to_string()))
        .collect::<Vec<_>>();
    store.replace_all(&entries)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store_in(dir: &tempfile::TempDir) -> Store {
        Store::open(&dir.path().join("ade.sqlite")).unwrap()
    }

    #[test]
    fn patch_round_trips_through_assemble() {
        let dir = tempfile::tempdir().unwrap();
        let store = store_in(&dir);
        apply_patch(&store, r#"{"tabsByWorktree":{"w1":[]},"activeTabId":null}"#).unwrap();
        let assembled = assemble_state_text(&store).unwrap();
        let value: Value = serde_json::from_str(&assembled).unwrap();
        assert_eq!(value["tabsByWorktree"]["w1"], Value::Array(vec![]));
        assert_eq!(value["activeTabId"], Value::Null);
    }

    #[test]
    fn patch_replaces_top_level_keys_wholesale() {
        let dir = tempfile::tempdir().unwrap();
        let store = store_in(&dir);
        apply_patch(&store, r#"{"tabsByWorktree":{"w1":["a"]}}"#).unwrap();
        apply_patch(&store, r#"{"tabsByWorktree":{"w2":["b"]}}"#).unwrap();
        let assembled = assemble_state_text(&store).unwrap();
        let value: Value = serde_json::from_str(&assembled).unwrap();
        // Whole-key replacement: w1 must be gone, not deep-merged.
        assert_eq!(value["tabsByWorktree"]["w2"], serde_json::json!(["b"]));
        assert!(value["tabsByWorktree"].get("w1").is_none());
    }

    #[test]
    fn set_deletes_keys_absent_from_payload_and_tolerates_unknown_keys() {
        let dir = tempfile::tempdir().unwrap();
        let store = store_in(&dir);
        apply_patch(&store, r#"{"keep":1,"drop":2,"futureKey":{}}"#).unwrap();
        apply_set(&store, r#"{"keep":3}"#).unwrap();
        let value: Value = serde_json::from_str(&assemble_state_text(&store).unwrap()).unwrap();
        assert_eq!(value["keep"], 3);
        assert!(value.get("drop").is_none());
        assert!(value.get("futureKey").is_none());
    }

    #[test]
    fn non_object_payloads_are_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let store = store_in(&dir);
        assert!(apply_patch(&store, "[]").is_err());
        assert!(apply_patch(&store, "not json").is_err());
        assert!(apply_set(&store, "42").is_err());
    }
}

use tauri::State;

use crate::state::AppState;

/// Read the full workspace session state as one JSON object text.
#[tauri::command]
#[specta::specta]
pub async fn session_get(state: State<'_, AppState>) -> Result<String, BridgeError> {
    assemble_state_text(state.session_store())
}

/// Replace each present top-level key wholesale (opaque JSON payload).
#[tauri::command]
#[specta::specta]
pub async fn session_patch(state: State<'_, AppState>, args: String) -> Result<(), BridgeError> {
    apply_patch(state.session_store(), &args)
}

/// Whole-state replace; keys absent from the payload are deleted.
#[tauri::command]
#[specta::specta]
pub async fn session_set(state: State<'_, AppState>, args: String) -> Result<(), BridgeError> {
    apply_set(state.session_store(), &args)
}

/// Explicit flush point; WAL commits already hold durability, so this only
/// folds the WAL (spec §3.2).
#[tauri::command]
#[specta::specta]
pub async fn session_flush(state: State<'_, AppState>) -> Result<(), BridgeError> {
    Ok(state.session_store().checkpoint_passive()?)
}
