use std::fs::{self, OpenOptions};
use std::io;
use std::path::Path;

use ade_core::ids::new_uuid;
use ade_core::path_compare::normalize_for_comparison;

use crate::{FsError, FsService};

impl FsService {
    /// Atomic write: stage a `.ade-tmp-*` file in the target directory, then
    /// rename it over the target so readers never observe a partial file.
    pub fn write_file(&self, path: &str, content: &str) -> Result<(), FsError> {
        let target = self.resolve(path)?;
        if fs::metadata(&target)
            .map(|meta| meta.is_dir())
            .unwrap_or(false)
        {
            return Err(FsError::InvalidInput(
                "Cannot write to a directory".to_string(),
            ));
        }
        let parent = target
            .parent()
            .ok_or_else(|| FsError::InvalidInput(format!("Invalid path: {path}")))?;
        let temp_path = parent.join(format!(".ade-tmp-{}", new_uuid()));
        let write_result = write_atomic(&temp_path, &target, content);
        if write_result.is_err() {
            let _ = fs::remove_file(&temp_path);
        }
        write_result.map_err(FsError::Io)
    }

    /// Create an empty file if it does not exist; the parent directory must exist.
    pub fn create_file(&self, path: &str) -> Result<(), FsError> {
        let target = self.resolve(path)?;
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&target)
        {
            Ok(_) => Ok(()),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                Err(already_exists(&target))
            }
            Err(error) => Err(FsError::Io(error)),
        }
    }

    pub fn create_dir(&self, path: &str) -> Result<(), FsError> {
        let target = self.resolve(path)?;
        if fs::symlink_metadata(&target).is_ok() {
            return Err(already_exists(&target));
        }
        fs::create_dir_all(&target).map_err(FsError::Io)
    }

    pub fn rename(&self, old_path: &str, new_path: &str) -> Result<(), FsError> {
        let old = self.resolve_preserving_symlink(old_path)?;
        let new = self.resolve_preserving_symlink(new_path)?;
        assert_no_clobber(&old, &new)?;
        fs::rename(&old, &new).map_err(FsError::Io)
    }

    /// Copy a file or a directory tree; refuses to overwrite an existing destination.
    pub fn copy(&self, source_path: &str, destination_path: &str) -> Result<(), FsError> {
        let source = self.resolve_preserving_symlink(source_path)?;
        let destination = self.resolve_preserving_symlink(destination_path)?;
        if destination.starts_with(&source) {
            return Err(FsError::InvalidInput(format!(
                "Cannot copy a path into itself: {}",
                destination.display()
            )));
        }
        match fs::symlink_metadata(&destination) {
            Ok(_) => return Err(already_exists(&destination)),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(FsError::Io(error)),
        }
        copy_recursive(&source, &destination)
    }

    /// Move a path to the system recycle bin. Refuses to trash an authorized
    /// root itself and treats an already-missing path as deleted.
    pub fn delete_path(&self, path: &str) -> Result<(), FsError> {
        let target = self.resolve_preserving_symlink(path)?;
        if self.is_authorized_root(&target) {
            return Err(FsError::InvalidInput(format!(
                "Cannot delete an authorized root: {}",
                target.display()
            )));
        }
        match fs::symlink_metadata(&target) {
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(FsError::Io(error)),
        }
        trash::delete(&target).map_err(|error| FsError::Trash(error.to_string()))
    }
}

fn write_atomic(temp_path: &Path, target: &Path, content: &str) -> io::Result<()> {
    fs::write(temp_path, content)?;
    if let Ok(metadata) = fs::metadata(target) {
        let _ = fs::set_permissions(temp_path, metadata.permissions());
    }
    fs::rename(temp_path, target)
}

fn assert_no_clobber(old: &Path, new: &Path) -> Result<(), FsError> {
    let new_metadata = match fs::symlink_metadata(new) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(FsError::Io(error)),
    };
    let old_metadata = fs::symlink_metadata(old)?;
    if same_directory_entry(old, new, &old_metadata, &new_metadata) {
        return Ok(());
    }
    Err(already_exists(new))
}

/// Same directory entry, mirroring oracle `isSameDirectoryEntryRename`.
/// dev+ino identity is required first: canonical equality alone conflates a
/// symlink with its target and would let a rename destroy the target's
/// content. The basename fallback applies only when realpath fails (dangling
/// symlinks), keeping case-only renames of such links working.
fn same_directory_entry(
    old: &Path,
    new: &Path,
    old_metadata: &fs::Metadata,
    new_metadata: &fs::Metadata,
) -> bool {
    if old.parent() != new.parent() || !same_file_identity(old_metadata, new_metadata) {
        return false;
    }
    if old == new {
        return true;
    }
    match (old.canonicalize(), new.canonicalize()) {
        (Ok(old_real), Ok(new_real)) => old_real == new_real,
        _ => {
            let old_name = file_name(old);
            let new_name = file_name(new);
            old_name != new_name && folded_basename(old) == folded_basename(new)
        }
    }
}

#[cfg(unix)]
fn same_file_identity(a: &fs::Metadata, b: &fs::Metadata) -> bool {
    use std::os::unix::fs::MetadataExt;
    a.dev() == b.dev() && a.ino() == b.ino()
}

/// Windows identity accessors (`volume_serial_number`/`file_index`) are still
/// unstable in std, so fail closed there: never allow a clobber. Windows is
/// outside this phase's verification scope.
#[cfg(not(unix))]
fn same_file_identity(_a: &fs::Metadata, _b: &fs::Metadata) -> bool {
    false
}

fn folded_basename(path: &Path) -> String {
    normalize_for_comparison(&file_name(path)).to_lowercase()
}

fn copy_recursive(source: &Path, destination: &Path) -> Result<(), FsError> {
    let metadata = fs::symlink_metadata(source)?;
    if metadata.is_symlink() {
        return copy_symlink(source, destination);
    }
    if metadata.is_dir() {
        fs::create_dir_all(destination)?;
        for entry in fs::read_dir(source)? {
            let entry = entry?;
            copy_recursive(&entry.path(), &destination.join(entry.file_name()))?;
        }
        return Ok(());
    }
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::copy(source, destination)?;
    Ok(())
}

#[cfg(unix)]
fn copy_symlink(source: &Path, destination: &Path) -> Result<(), FsError> {
    let link_target = fs::read_link(source)?;
    std::os::unix::fs::symlink(link_target, destination)?;
    Ok(())
}

#[cfg(not(unix))]
fn copy_symlink(_source: &Path, _destination: &Path) -> Result<(), FsError> {
    Err(FsError::InvalidInput(
        "Copying symlinks is not supported on this platform".to_string(),
    ))
}

fn already_exists(path: &Path) -> FsError {
    FsError::AlreadyExists(file_name(path))
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_string_lossy().into_owned())
}
