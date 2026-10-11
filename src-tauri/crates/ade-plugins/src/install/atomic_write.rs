//! Atomic file writes for plugin state (pointer, provenance, lockfile).
//!
//! Ported from `orca:src/main/plugins/plugin-atomic-file-write.ts`: write to a
//! unique temporary sibling, then rename over the target. The Windows
//! EPERM/EACCES/EBUSY rename retry with [50, 100, 150, 200, 250] ms backoff is
//! kept (antivirus/indexer holds); on macOS it is inert but harmless.

use std::ffi::OsStr;
use std::fs;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::Duration;

use uuid::Uuid;

use crate::types::{PluginError, IO_ERROR};

pub const RENAME_RETRY_DELAYS_MS: [u64; 5] = [50, 100, 150, 200, 250];

/// Writes `bytes` to `path` atomically with an optional exact Unix mode. The
/// parent directory must already exist; callers that create state directories
/// (`write_lockfile_atomic`, `write_provenance`) do so explicitly.
pub fn write_atomic(path: &Path, bytes: &[u8], mode: Option<u32>) -> Result<(), PluginError> {
    let temporary = temporary_path(path);
    let result = write_and_rename(&temporary, path, bytes, mode);
    let _ = fs::remove_file(&temporary);
    result.map_err(|error| PluginError::new(IO_ERROR, format!("{}: {error}", path.display())))
}

fn write_and_rename(
    temporary: &Path,
    target: &Path,
    bytes: &[u8],
    mode: Option<u32>,
) -> std::io::Result<()> {
    let mut file = fs::File::create(temporary)?;
    if let Some(mode) = mode {
        file.set_permissions(fs::Permissions::from_mode(mode))?;
    }
    file.write_all(bytes)?;
    drop(file);
    rename_with_retry(temporary, target)
}

fn temporary_path(path: &Path) -> PathBuf {
    let mut name = path.file_name().map(OsStr::to_owned).unwrap_or_default();
    name.push(format!(
        ".{}.{}.tmp",
        std::process::id(),
        Uuid::new_v4().simple()
    ));
    path.with_file_name(name)
}

/// Rename with the oracle's Windows retry semantics: only EPERM/EACCES/EBUSY
/// are retried, and the original error surfaces once the backoff is exhausted.
pub fn rename_with_retry(source: &Path, target: &Path) -> std::io::Result<()> {
    let mut attempt = 0;
    loop {
        match fs::rename(source, target) {
            Ok(()) => return Ok(()),
            Err(error) => {
                let retryable = matches!(
                    error.raw_os_error(),
                    Some(libc::EPERM) | Some(libc::EACCES) | Some(libc::EBUSY)
                );
                if !retryable || attempt >= RENAME_RETRY_DELAYS_MS.len() {
                    return Err(error);
                }
                thread::sleep(Duration::from_millis(RENAME_RETRY_DELAYS_MS[attempt]));
                attempt += 1;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn writes_and_replaces_files_atomically() {
        let dir = tempdir().unwrap();
        let target = dir.path().join("state.json");

        write_atomic(&target, b"first", None).unwrap();
        assert_eq!(fs::read(&target).unwrap(), b"first");
        write_atomic(&target, b"second", None).unwrap();
        assert_eq!(fs::read(&target).unwrap(), b"second");

        let leftovers: Vec<String> = fs::read_dir(dir.path())
            .unwrap()
            .flatten()
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .filter(|name| name.ends_with(".tmp"))
            .collect();
        assert!(
            leftovers.is_empty(),
            "temporary files left behind: {leftovers:?}"
        );
    }

    #[test]
    fn applies_the_requested_mode() {
        let dir = tempdir().unwrap();
        let target = dir.path().join("provenance.json");
        write_atomic(&target, b"{}", Some(0o600)).unwrap();
        let mode = fs::metadata(&target).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }

    #[test]
    fn missing_parent_directory_is_an_io_error() {
        let dir = tempdir().unwrap();
        let error = write_atomic(&dir.path().join("missing/state.json"), b"x", None).unwrap_err();
        assert_eq!(error.code(), IO_ERROR);
    }

    #[test]
    fn rename_with_retry_replaces_the_target() {
        let dir = tempdir().unwrap();
        let source = dir.path().join("source");
        let target = dir.path().join("target");
        fs::write(&source, b"new").unwrap();
        fs::write(&target, b"old").unwrap();
        rename_with_retry(&source, &target).unwrap();
        assert_eq!(fs::read(&target).unwrap(), b"new");
        assert!(!source.exists());
    }
}
