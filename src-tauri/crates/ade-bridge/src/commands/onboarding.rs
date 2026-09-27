use std::sync::Mutex;

use ade_store::onboarding_store::OnboardingStore;
use serde_json::Value;
use tauri::State;

use crate::errors::BridgeError;
use crate::json::Json;
use crate::state::{lock, AppState};

/// Read the full effective onboarding state (`defaults ∪ stored`).
#[tauri::command]
#[specta::specta]
pub async fn onboarding_get(state: State<'_, AppState>) -> Result<Json, BridgeError> {
    Ok(onboarding_get_from(&state.onboarding))
}

/// Merge a renderer partial into the snapshot and persist it synchronously.
#[tauri::command]
#[specta::specta]
pub async fn onboarding_update(
    state: State<'_, AppState>,
    args: Json,
) -> Result<Json, BridgeError> {
    onboarding_update_in(&state.onboarding, args.into_inner())
}

/// Tauri-free core of [`onboarding_get`], so the read path is testable without
/// a runtime.
pub(crate) fn onboarding_get_from(store: &Mutex<OnboardingStore>) -> Json {
    Json::new(lock(store).get())
}

/// Tauri-free core of [`onboarding_update`], so the write path is testable
/// without a runtime.
pub(crate) fn onboarding_update_in(
    store: &Mutex<OnboardingStore>,
    partial: Value,
) -> Result<Json, BridgeError> {
    Ok(Json::new(lock(store).update(partial)?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use ade_core::defaults::onboarding_defaults;
    use serde_json::json;
    use std::sync::Arc;

    fn store_in(path: std::path::PathBuf) -> Arc<Mutex<OnboardingStore>> {
        Arc::new(Mutex::new(OnboardingStore::load(
            path,
            onboarding_defaults(),
        )))
    }

    #[test]
    fn get_returns_defaults_initially() {
        let dir = tempfile::tempdir().unwrap();
        let store = store_in(dir.path().join("onboarding.json"));
        assert_eq!(onboarding_get_from(&store).into_inner(), onboarding_defaults());
    }

    #[test]
    fn update_persists_across_a_fresh_store_load() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("onboarding.json");
        let store = store_in(path.clone());

        let returned = onboarding_update_in(
            &store,
            json!({ "lastCompletedStep": 3, "checklist": { "choseAgent": true } }),
        )
        .unwrap()
        .into_inner();
        assert_eq!(returned["lastCompletedStep"], 3);
        assert_eq!(returned["checklist"]["choseAgent"], true);

        let reloaded = onboarding_get_from(&store_in(path)).into_inner();
        assert_eq!(reloaded["lastCompletedStep"], 3);
        assert_eq!(reloaded["checklist"]["choseAgent"], true);
        assert_eq!(reloaded["checklist"]["addedRepo"], false);
    }
}
