//! Immutable install provenance, prewritten before the executable current
//! pointer moves.
//!
//! Ported from `orca:src/main/plugins/plugin-install-provenance.ts`: one
//! `{version: 1, entry}` document per content hash under
//! `<key-dir>/.install-provenance/<hash>.json`, written atomically with mode
//! `0600` and capped at 64 KiB on read. Provenance is the durable identity a
//! rollback republishes and a same-byte reinstall must not replace.

use std::collections::HashSet;
use std::fs;
use std::io::{ErrorKind, Read};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::discovery::is_plugin_content_hash;
use crate::install::atomic_write::write_atomic;
use crate::install::lockfile::LockEntry;
use crate::types::{PluginError, INVALID_MANIFEST, IO_ERROR};

pub const PROVENANCE_DIRECTORY_NAME: &str = ".install-provenance";
pub const PROVENANCE_MAX_BYTES: u64 = 64 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Provenance {
    pub version: u32,
    pub entry: LockEntry,
}

impl Provenance {
    pub fn new(entry: LockEntry) -> Self {
        Self { version: 1, entry }
    }
}

fn provenance_path(plugin_dir: &Path, content_hash: &str) -> Option<PathBuf> {
    if !is_plugin_content_hash(content_hash) {
        return None;
    }
    Some(
        plugin_dir
            .join(PROVENANCE_DIRECTORY_NAME)
            .join(format!("{content_hash}.json")),
    )
}

/// Validates and prewrites the immutable record for `content_hash`.
pub fn write_provenance(
    plugin_dir: &Path,
    content_hash: &str,
    provenance: &Provenance,
) -> Result<(), PluginError> {
    if !is_plugin_content_hash(content_hash) {
        return Err(PluginError::new(
            INVALID_MANIFEST,
            "invalid plugin content hash",
        ));
    }
    if provenance.version != 1 {
        return Err(PluginError::new(
            INVALID_MANIFEST,
            "unsupported provenance version",
        ));
    }
    provenance.entry.validate()?;
    let directory = plugin_dir.join(PROVENANCE_DIRECTORY_NAME);
    fs::create_dir_all(&directory).map_err(|error| {
        PluginError::new(
            IO_ERROR,
            format!("failed to create the provenance directory: {error}"),
        )
    })?;
    let bytes = serde_json::to_vec_pretty(provenance).map_err(|error| {
        PluginError::new(IO_ERROR, format!("failed to serialize provenance: {error}"))
    })?;
    let Some(path) = provenance_path(plugin_dir, content_hash) else {
        return Err(PluginError::new(
            INVALID_MANIFEST,
            "invalid plugin content hash",
        ));
    };
    write_atomic(&path, &bytes, Some(0o600))
}

/// Missing, oversized, corrupt, or unknown-version provenance reads as `None`.
pub fn read_provenance(plugin_dir: &Path, content_hash: &str) -> Option<Provenance> {
    let path = provenance_path(plugin_dir, content_hash)?;
    let file = fs::File::open(path).ok()?;
    let mut bytes = Vec::new();
    file.take(PROVENANCE_MAX_BYTES + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    if bytes.len() as u64 > PROVENANCE_MAX_BYTES {
        return None;
    }
    let provenance: Provenance = serde_json::from_slice(&bytes).ok()?;
    (provenance.version == 1).then_some(provenance)
}

/// Removes provenance records whose hash is not in `retained`; missing
/// directories are fine.
pub fn prune_provenance(plugin_dir: &Path, retained: &HashSet<String>) -> Result<(), PluginError> {
    let directory = plugin_dir.join(PROVENANCE_DIRECTORY_NAME);
    let entries = match fs::read_dir(&directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(()),
        Err(error) => {
            return Err(PluginError::new(
                IO_ERROR,
                format!("failed to read the provenance directory: {error}"),
            ))
        }
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let Some(stem) = name.strip_suffix(".json") else {
            continue;
        };
        if !is_plugin_content_hash(stem) || retained.contains(stem) {
            continue;
        }
        let _ = fs::remove_file(entry.path());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::install::lockfile::LockSource;
    use tempfile::tempdir;

    const HASH: &str = "abababababababababababababababababababababababababababababababab";

    fn entry() -> LockEntry {
        LockEntry {
            plugin_key: "orca-samples.demo".to_string(),
            version: "1.0.0".to_string(),
            source: LockSource::LocalPath {
                path: "/tmp/source".to_string(),
            },
            resolved_commit: None,
            content_hash: HASH.to_string(),
            consent_fingerprint: "sha256-abc".to_string(),
            installed_at: 5,
        }
    }

    #[test]
    fn provenance_round_trips_with_mode_0600() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempdir().unwrap();
        let plugin_dir = dir.path().join("orca-samples.demo");
        fs::create_dir_all(&plugin_dir).unwrap();

        write_provenance(&plugin_dir, HASH, &Provenance::new(entry())).unwrap();

        let path = plugin_dir
            .join(PROVENANCE_DIRECTORY_NAME)
            .join(format!("{HASH}.json"));
        let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
        let raw: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        assert_eq!(raw["version"], 1);
        assert_eq!(raw["entry"]["capabilityHash"], "sha256-abc");

        let read = read_provenance(&plugin_dir, HASH).unwrap();
        assert_eq!(read.entry, entry());
    }

    #[test]
    fn invalid_provenance_reads_none() {
        let dir = tempdir().unwrap();
        let provenance_dir = dir.path().join(PROVENANCE_DIRECTORY_NAME);
        fs::create_dir_all(&provenance_dir).unwrap();
        let path = provenance_dir.join(format!("{HASH}.json"));

        assert!(read_provenance(dir.path(), HASH).is_none());
        assert!(read_provenance(dir.path(), "not-a-hash").is_none());

        fs::write(&path, b"garbage").unwrap();
        assert!(read_provenance(dir.path(), HASH).is_none());

        fs::write(&path, br#"{"version": 2, "entry": {}}"#).unwrap();
        assert!(read_provenance(dir.path(), HASH).is_none());

        fs::write(&path, br#"{"version": 1, "entry": null}"#).unwrap();
        assert!(read_provenance(dir.path(), HASH).is_none());
    }

    #[test]
    fn oversized_provenance_reads_none() {
        let dir = tempdir().unwrap();
        let provenance_dir = dir.path().join(PROVENANCE_DIRECTORY_NAME);
        fs::create_dir_all(&provenance_dir).unwrap();
        let path = provenance_dir.join(format!("{HASH}.json"));
        fs::write(&path, b"").unwrap();
        fs::OpenOptions::new()
            .write(true)
            .open(&path)
            .unwrap()
            .set_len(PROVENANCE_MAX_BYTES + 1)
            .unwrap();
        assert!(read_provenance(dir.path(), HASH).is_none());
    }

    #[test]
    fn writes_reject_invalid_inputs() {
        let dir = tempdir().unwrap();
        let plugin_dir = dir.path().join("orca-samples.demo");
        fs::create_dir_all(&plugin_dir).unwrap();

        assert!(write_provenance(&plugin_dir, "not-a-hash", &Provenance::new(entry())).is_err());

        let mut invalid = entry();
        invalid.consent_fingerprint = String::new();
        assert!(write_provenance(&plugin_dir, HASH, &Provenance::new(invalid)).is_err());

        let mut wrong_version = Provenance::new(entry());
        wrong_version.version = 2;
        assert!(write_provenance(&plugin_dir, HASH, &wrong_version).is_err());
    }

    #[test]
    fn prune_removes_unretained_records() {
        let dir = tempdir().unwrap();
        let plugin_dir = dir.path().join("orca-samples.demo");
        fs::create_dir_all(&plugin_dir).unwrap();
        let other_hash = "cd".repeat(32);

        write_provenance(&plugin_dir, HASH, &Provenance::new(entry())).unwrap();
        write_provenance(&plugin_dir, &other_hash, &Provenance::new(entry())).unwrap();
        let mut retained = HashSet::new();
        retained.insert(HASH.to_string());
        prune_provenance(&plugin_dir, &retained).unwrap();

        assert!(read_provenance(&plugin_dir, HASH).is_some());
        assert!(read_provenance(&plugin_dir, &other_hash).is_none());

        prune_provenance(&dir.path().join("missing"), &retained).unwrap();
    }
}
