//! The `current` pointer: a tiny file naming the hash-addressed version
//! directory that is active for one plugin.
//!
//! Ported from `orca:src/main/plugins/plugin-current-pointer.ts`: reads are
//! capped at 128 bytes so a corrupt sparse file cannot be allocated during
//! startup, missing pointers resolve to `None`, and writes are atomic. The
//! filename/limit constants are re-exported from discovery, which already
//! consumes them for list projection.

use std::fs;
use std::io::{ErrorKind, Read};
use std::path::Path;

pub use crate::discovery::{PLUGIN_CURRENT_POINTER_FILENAME, PLUGIN_CURRENT_POINTER_MAX_BYTES};

use crate::install::atomic_write::write_atomic;
use crate::types::{PluginError, IO_ERROR};

/// Reads the pointer; a missing file yields `None`, an oversized one is an
/// error (mirrors the oracle's streaming cap).
pub fn read_pointer(key_dir: &Path) -> Result<Option<String>, PluginError> {
    let path = key_dir.join(PLUGIN_CURRENT_POINTER_FILENAME);
    let file = match fs::File::open(&path) {
        Ok(file) => file,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(PluginError::new(
                IO_ERROR,
                format!("failed to read the current-version pointer: {error}"),
            ))
        }
    };
    let mut bytes = Vec::new();
    file.take(PLUGIN_CURRENT_POINTER_MAX_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| {
            PluginError::new(
                IO_ERROR,
                format!("failed to read the current-version pointer: {error}"),
            )
        })?;
    if bytes.len() as u64 > PLUGIN_CURRENT_POINTER_MAX_BYTES {
        return Err(PluginError::new(
            IO_ERROR,
            "current-version pointer exceeds its size limit",
        ));
    }
    Ok(Some(String::from_utf8_lossy(&bytes).trim().to_string()))
}

pub fn write_pointer(key_dir: &Path, content_hash: &str) -> Result<(), PluginError> {
    write_atomic(
        &key_dir.join(PLUGIN_CURRENT_POINTER_FILENAME),
        content_hash.as_bytes(),
        None,
    )
}

/// Restores the pointer to `previous`; `None` removes it so a failed first
/// publication leaves no current version behind.
pub fn restore_pointer(key_dir: &Path, previous: Option<&str>) -> Result<(), PluginError> {
    match previous {
        Some(content_hash) => write_pointer(key_dir, content_hash),
        None => {
            let path = key_dir.join(PLUGIN_CURRENT_POINTER_FILENAME);
            match fs::remove_file(&path) {
                Ok(()) => Ok(()),
                Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
                Err(error) => Err(PluginError::new(
                    IO_ERROR,
                    format!("failed to remove the current-version pointer: {error}"),
                )),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn missing_pointer_reads_as_none() {
        let dir = tempdir().unwrap();
        assert_eq!(read_pointer(dir.path()).unwrap(), None);
    }

    #[test]
    fn pointer_round_trips_and_trims() {
        let dir = tempdir().unwrap();
        let hash = "ab".repeat(32);
        write_pointer(dir.path(), &hash).unwrap();
        assert_eq!(
            read_pointer(dir.path()).unwrap().as_deref(),
            Some(hash.as_str())
        );
        assert_eq!(
            fs::read(dir.path().join("current")).unwrap(),
            hash.as_bytes()
        );
    }

    #[test]
    fn oversized_pointer_is_rejected() {
        let dir = tempdir().unwrap();
        fs::write(
            dir.path().join(PLUGIN_CURRENT_POINTER_FILENAME),
            "a".repeat(PLUGIN_CURRENT_POINTER_MAX_BYTES as usize + 1),
        )
        .unwrap();
        let error = read_pointer(dir.path()).unwrap_err();
        assert_eq!(error.code(), IO_ERROR);
        assert_eq!(
            error.message(),
            "current-version pointer exceeds its size limit"
        );
    }

    #[test]
    fn restore_none_removes_the_pointer() {
        let dir = tempdir().unwrap();
        write_pointer(dir.path(), &"cd".repeat(32)).unwrap();
        restore_pointer(dir.path(), None).unwrap();
        assert!(!dir.path().join(PLUGIN_CURRENT_POINTER_FILENAME).exists());
        restore_pointer(dir.path(), None).unwrap();

        let previous = "ef".repeat(32);
        restore_pointer(dir.path(), Some(&previous)).unwrap();
        assert_eq!(
            read_pointer(dir.path()).unwrap().as_deref(),
            Some(previous.as_str())
        );
    }
}
