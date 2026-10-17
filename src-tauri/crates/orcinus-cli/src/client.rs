//! Local runtime metadata reader: `runtime.json` presence and identity for
//! `orcinus status`. The runtime RPC client that used to live here was removed
//! with the computer-use trim — nothing serves `runtime.sock` anymore — so this
//! module never connects; it only reads the file.

use std::path::{Path, PathBuf};

use serde::Deserialize;

/// Runtime bootstrap metadata file name inside the app data dir.
pub const RUNTIME_METADATA_FILE: &str = "runtime.json";

/// Environment override for the app data dir (spec §3.5/§3.6).
pub const USER_DATA_PATH_ENV: &str = "ORCINUS_USER_DATA_PATH";
/// Default app data dir under `~/Library/Application Support`.
pub const DEFAULT_APP_DIR_NAME: &str = "dev.itsuka.orcinus";

/// Runtime metadata path: `ORCINUS_USER_DATA_PATH` override else
/// `~/Library/Application Support/dev.itsuka.orcinus/runtime.json`.
pub fn metadata_path() -> PathBuf {
    if let Ok(path) = std::env::var(USER_DATA_PATH_ENV) {
        if !path.trim().is_empty() {
            return PathBuf::from(path).join(RUNTIME_METADATA_FILE);
        }
    }
    let home = std::env::var("HOME").unwrap_or_default();
    PathBuf::from(home)
        .join("Library")
        .join("Application Support")
        .join(DEFAULT_APP_DIR_NAME)
        .join(RUNTIME_METADATA_FILE)
}

/// Metadata read state: absent (never started), unparseable (stale bootstrap)
/// or parseable (identity available).
#[derive(Debug)]
pub enum MetadataRead {
    Missing,
    Unreadable,
    Ready(RuntimeMetadata),
}

/// Read `runtime.json`. `Missing` only when the file does not exist;
/// unparseable metadata is `Unreadable` so `status` can distinguish "never
/// started" from "started with a broken bootstrap".
pub fn read_metadata(path: &Path) -> MetadataRead {
    let raw = match std::fs::read_to_string(path) {
        Ok(raw) => raw,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return MetadataRead::Missing
        }
        Err(_) => return MetadataRead::Unreadable,
    };
    match serde_json::from_str(&raw) {
        Ok(metadata) => MetadataRead::Ready(metadata),
        Err(_) => MetadataRead::Unreadable,
    }
}

/// Runtime metadata identity (`runtime.json`, camelCase on the wire).
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeMetadata {
    pub runtime_id: String,
}
