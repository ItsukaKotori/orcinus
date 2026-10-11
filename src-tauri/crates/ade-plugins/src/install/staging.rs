//! Staging pipeline: validate a source tree, copy it into a `<hash>.staging`
//! sibling without root `.git` metadata, prove the copied bytes, then rename
//! atomically into the hash-addressed install directory.
//!
//! Ported from `orca:src/main/plugins/plugin-install-staging.ts`:
//! manifest → engine gate → declared-artifact validation → content hash →
//! existing-hash verification or copy with rehash/reparse/revalidate →
//! atomic rename. An existing `<hash>` directory is never repointed at
//! without proving its bytes first, so a partial or tampered install cannot
//! be revived by a reinstall.

use std::ffi::OsStr;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

use crate::artifact::validate_declared_artifacts;
use crate::content_hash::hash_plugin_tree;
use crate::install::atomic_write::rename_with_retry;
use crate::manifest::{
    engine_satisfies, parse_manifest, Manifest, LEGACY_MANIFEST_FILENAME, MANIFEST_FILENAME,
    MAX_MANIFEST_BYTES,
};
use crate::types::{
    PluginError, CONTENT_CHANGED, ENGINE_INCOMPATIBLE, INVALID_MANIFEST, IO_ERROR,
    SYMLINK_NOT_ALLOWED, UNSUPPORTED_ENTRY,
};

/// Reads the install manifest (`ade-plugin.json`, falling back to
/// `orca-plugin.json`) and applies the v1 engines gate.
pub fn read_install_manifest(root: &Path, host_version: &str) -> Result<Manifest, PluginError> {
    let manifest = reparse_manifest(root)?;
    if !engine_satisfies(&manifest.engines.orca, host_version) {
        return Err(PluginError::new(
            ENGINE_INCOMPATIBLE,
            format!(
                "plugin requires Orca {} (this is {})",
                manifest.engines.orca, host_version
            ),
        ));
    }
    Ok(manifest)
}

fn manifest_path(root: &Path) -> Result<PathBuf, PluginError> {
    let ade = root.join(MANIFEST_FILENAME);
    if ade.is_file() {
        return Ok(ade);
    }
    let legacy = root.join(LEGACY_MANIFEST_FILENAME);
    if legacy.is_file() {
        return Ok(legacy);
    }
    Err(PluginError::new(
        INVALID_MANIFEST,
        format!("missing {MANIFEST_FILENAME}"),
    ))
}

/// Re-reads and re-parses an installed tree's manifest (`ade-plugin.json`,
/// falling back to `orca-plugin.json`) with the manifest size bound applied.
/// Shared with the integrity verifier, which must reparse the same bytes.
pub fn reparse_manifest(root: &Path) -> Result<Manifest, PluginError> {
    let path = manifest_path(root)?;
    let filename = path
        .file_name()
        .and_then(OsStr::to_str)
        .unwrap_or(MANIFEST_FILENAME)
        .to_string();
    let bytes = read_bounded(&path, &filename, MAX_MANIFEST_BYTES)?;
    parse_manifest(&bytes).map_err(|error| {
        PluginError::new(
            error.code(),
            format!("invalid manifest: {}", error.message()),
        )
    })
}

fn read_bounded(path: &Path, filename: &str, max_bytes: usize) -> Result<Vec<u8>, PluginError> {
    let file = fs::File::open(path).map_err(|error| {
        PluginError::new(IO_ERROR, format!("failed to read {filename}: {error}"))
    })?;
    let mut bytes = Vec::new();
    file.take(max_bytes as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| {
            PluginError::new(IO_ERROR, format!("failed to read {filename}: {error}"))
        })?;
    if bytes.len() > max_bytes {
        return Err(PluginError::new(
            INVALID_MANIFEST,
            format!("{filename} exceeds {max_bytes} bytes"),
        ));
    }
    Ok(bytes)
}

/// Validates `manifest` against the source tree, hashes it, and materializes
/// it under `dest_parent` (the plugin's `<install_dir>/<key>` directory).
///
/// Returns `(content_hash, version_dir)`. When the hash directory already
/// exists its bytes are rehashed, reparsed, and revalidated instead of being
/// overwritten.
pub fn stage_plugin_tree(
    source_root: &Path,
    dest_parent: &Path,
    manifest: &Manifest,
) -> Result<(String, PathBuf), PluginError> {
    validate_declared_artifacts(source_root, manifest)?;
    let content_hash = hash_plugin_tree(source_root)?;
    let version_dir = dest_parent.join(&content_hash);
    if version_dir.is_dir() {
        verify_installed_version(&version_dir, &content_hash, &manifest.qualified_key())?;
        return Ok((content_hash, version_dir));
    }

    let staged_dir = dest_parent.join(format!("{content_hash}.staging"));
    let _ = fs::remove_dir_all(&staged_dir);
    let result = stage_copy(
        source_root,
        &staged_dir,
        &version_dir,
        &content_hash,
        manifest,
    );
    let _ = fs::remove_dir_all(&staged_dir);
    result?;
    Ok((content_hash, version_dir))
}

fn stage_copy(
    source_root: &Path,
    staged_dir: &Path,
    version_dir: &Path,
    expected_hash: &str,
    source_manifest: &Manifest,
) -> Result<(), PluginError> {
    if let Some(parent) = staged_dir.parent() {
        fs::create_dir_all(parent).map_err(|error| io_error(parent, error))?;
    }
    fs::create_dir_all(staged_dir).map_err(|error| io_error(staged_dir, error))?;
    copy_tree(source_root, staged_dir, true)?;
    // Source trees can change while copying. Hashing the destination closes
    // that race before the immutable directory becomes current.
    let copied_hash = hash_plugin_tree(staged_dir)?;
    if copied_hash != expected_hash {
        return Err(PluginError::new(
            CONTENT_CHANGED,
            "plugin content changed while it was being copied",
        ));
    }
    let copied_manifest = reparse_manifest(staged_dir)?;
    if copied_manifest.qualified_key() != source_manifest.qualified_key() {
        return Err(PluginError::new(
            INVALID_MANIFEST,
            "plugin manifest identity changed while it was being staged",
        ));
    }
    validate_declared_artifacts(staged_dir, &copied_manifest).map_err(|error| {
        PluginError::new(
            error.code(),
            format!("copied artifact validation failed: {}", error.message()),
        )
    })?;
    rename_with_retry(staged_dir, version_dir).map_err(|error| {
        PluginError::new(
            IO_ERROR,
            format!("failed to activate the staged plugin directory: {error}"),
        )
    })
}

fn verify_installed_version(
    version_dir: &Path,
    expected_hash: &str,
    expected_key: &str,
) -> Result<(), PluginError> {
    // Never repoint at an existing hash directory without proving its bytes;
    // a previous partial/tampered install must not be revived by reinstall.
    let existing_hash = hash_plugin_tree(version_dir)?;
    if existing_hash != expected_hash {
        return Err(PluginError::new(
            CONTENT_CHANGED,
            "existing plugin content failed integrity verification",
        ));
    }
    let manifest = reparse_manifest(version_dir)?;
    if manifest.qualified_key() != expected_key {
        return Err(PluginError::new(
            INVALID_MANIFEST,
            "installed plugin manifest identity does not match its directory",
        ));
    }
    validate_declared_artifacts(version_dir, &manifest).map_err(|error| {
        PluginError::new(
            error.code(),
            format!("installed artifact validation failed: {}", error.message()),
        )
    })?;
    Ok(())
}

/// Recursive copy. Root `.git` metadata is skipped at the copy boundary so a
/// large local repository cannot bypass install limits; nested `.git`
/// directories are plugin content (matching the content hash).
fn copy_tree(source: &Path, dest: &Path, is_root: bool) -> Result<(), PluginError> {
    let read_dir = fs::read_dir(source).map_err(|error| io_error(source, error))?;
    let mut entries: Vec<PathBuf> = Vec::new();
    for entry in read_dir {
        let entry = entry.map_err(|error| io_error(source, error))?;
        entries.push(entry.path());
    }
    entries.sort_by(|left, right| left.file_name().cmp(&right.file_name()));

    for from in entries {
        let name = from.file_name().unwrap_or_default().to_owned();
        if is_root && name == OsStr::new(".git") {
            continue;
        }
        let to = dest.join(&name);
        let metadata = fs::symlink_metadata(&from).map_err(|error| io_error(&from, error))?;
        let file_type = metadata.file_type();
        if file_type.is_symlink() {
            return Err(PluginError::new(
                SYMLINK_NOT_ALLOWED,
                format!("symlink not allowed in plugin content: {}", from.display()),
            ));
        }
        if file_type.is_dir() {
            fs::create_dir_all(&to).map_err(|error| io_error(&to, error))?;
            copy_tree(&from, &to, false)?;
        } else if file_type.is_file() {
            fs::copy(&from, &to).map_err(|error| io_error(&to, error))?;
        } else {
            return Err(PluginError::new(
                UNSUPPORTED_ENTRY,
                format!("unsupported plugin entry type: {}", from.display()),
            ));
        }
    }
    Ok(())
}

fn io_error(path: &Path, error: std::io::Error) -> PluginError {
    PluginError::new(IO_ERROR, format!("{}: {error}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::TOO_MANY_FILES;
    use serde_json::json;
    use tempfile::tempdir;

    fn manifest_value(id: &str, range: &str) -> serde_json::Value {
        json!({
            "manifestVersion": 1,
            "id": id,
            "publisher": "orca-samples",
            "name": id,
            "version": "1.0.0",
            "engines": { "orca": range },
            "pluginApi": 1,
            "contributes": {
                "panels": [{ "id": "panel", "title": "Panel", "entry": "panel.html" }]
            }
        })
    }

    fn write_source(root: &Path, id: &str, panel: &str) {
        fs::create_dir_all(root).unwrap();
        fs::write(
            root.join(MANIFEST_FILENAME),
            serde_json::to_vec(&manifest_value(id, ">=1.0.0")).unwrap(),
        )
        .unwrap();
        fs::write(root.join("panel.html"), panel).unwrap();
    }

    fn read_manifest(root: &Path) -> Manifest {
        read_install_manifest(root, "1.4.0").unwrap()
    }

    #[test]
    fn stages_a_copy_without_root_git_metadata() {
        let temp = tempdir().unwrap();
        let source = temp.path().join("source");
        write_source(&source, "demo", "<h1>panel</h1>");
        fs::create_dir_all(source.join(".git")).unwrap();
        fs::write(source.join(".git/HEAD"), b"ref: refs/heads/main\n").unwrap();

        let dest_parent = temp.path().join("install/orca-samples.demo");
        let manifest = read_manifest(&source);
        let (hash, version_dir) = stage_plugin_tree(&source, &dest_parent, &manifest).unwrap();

        assert_eq!(version_dir, dest_parent.join(&hash));
        assert!(version_dir.join(MANIFEST_FILENAME).is_file());
        assert_eq!(
            fs::read(version_dir.join("panel.html")).unwrap(),
            b"<h1>panel</h1>"
        );
        assert!(!version_dir.join(".git").exists());
        assert!(!dest_parent.join(format!("{hash}.staging")).exists());
    }

    #[test]
    fn nested_git_directories_are_copied_as_content() {
        let temp = tempdir().unwrap();
        let source = temp.path().join("source");
        write_source(&source, "demo", "<h1>panel</h1>");
        fs::create_dir_all(source.join("sub/.git")).unwrap();
        fs::write(source.join("sub/.git/HEAD"), b"ref: refs/heads/main\n").unwrap();

        let dest_parent = temp.path().join("install/orca-samples.demo");
        let manifest = read_manifest(&source);
        let (hash, _) = stage_plugin_tree(&source, &dest_parent, &manifest).unwrap();
        assert!(dest_parent.join(&hash).join("sub/.git/HEAD").is_file());
    }

    #[test]
    fn existing_version_is_verified_and_reused() {
        let temp = tempdir().unwrap();
        let source = temp.path().join("source");
        write_source(&source, "demo", "<h1>panel</h1>");
        let dest_parent = temp.path().join("install/orca-samples.demo");
        let manifest = read_manifest(&source);

        let (first_hash, version_dir) =
            stage_plugin_tree(&source, &dest_parent, &manifest).unwrap();

        let (second_hash, second_dir) =
            stage_plugin_tree(&source, &dest_parent, &manifest).unwrap();
        assert_eq!(second_hash, first_hash);
        assert_eq!(second_dir, version_dir);
        assert!(version_dir.join("panel.html").is_file());
        assert!(!dest_parent.join(format!("{first_hash}.staging")).exists());
    }

    #[test]
    fn tampered_existing_version_is_rejected() {
        let temp = tempdir().unwrap();
        let source = temp.path().join("source");
        write_source(&source, "demo", "<h1>panel</h1>");
        let dest_parent = temp.path().join("install/orca-samples.demo");
        let manifest = read_manifest(&source);
        let (hash, version_dir) = stage_plugin_tree(&source, &dest_parent, &manifest).unwrap();
        fs::write(version_dir.join("panel.html"), b"<h1>tampered</h1>").unwrap();

        let error = stage_plugin_tree(&source, &dest_parent, &manifest).unwrap_err();
        assert_eq!(error.code(), CONTENT_CHANGED);
        assert_eq!(
            error.message(),
            "existing plugin content failed integrity verification"
        );
        assert!(!dest_parent.join(format!("{hash}.staging")).exists());
    }

    #[test]
    fn engine_gate_rejects_incompatible_hosts() {
        let temp = tempdir().unwrap();
        let source = temp.path().join("source");
        fs::create_dir_all(&source).unwrap();
        fs::write(
            source.join(MANIFEST_FILENAME),
            serde_json::to_vec(&manifest_value("demo", ">=9.0.0")).unwrap(),
        )
        .unwrap();

        let error = read_install_manifest(&source, "1.4.0").unwrap_err();
        assert_eq!(error.code(), ENGINE_INCOMPATIBLE);
        assert_eq!(
            error.message(),
            "plugin requires Orca >=9.0.0 (this is 1.4.0)"
        );
        assert!(read_install_manifest(&source, "9.1.0").is_ok());
    }

    #[test]
    fn missing_manifest_is_invalid_manifest() {
        let temp = tempdir().unwrap();
        let error = read_install_manifest(temp.path(), "1.4.0").unwrap_err();
        assert_eq!(error.code(), INVALID_MANIFEST);
        assert_eq!(error.message(), "missing ade-plugin.json");
    }

    #[test]
    fn legacy_manifest_filename_is_read() {
        let temp = tempdir().unwrap();
        let source = temp.path().join("source");
        fs::create_dir_all(&source).unwrap();
        fs::write(
            source.join(LEGACY_MANIFEST_FILENAME),
            serde_json::to_vec(&manifest_value("demo", ">=1.0.0")).unwrap(),
        )
        .unwrap();
        fs::write(source.join("panel.html"), b"<h1/>").unwrap();

        let manifest = read_install_manifest(&source, "1.4.0").unwrap();
        assert_eq!(manifest.qualified_key(), "orca-samples.demo");
    }

    #[test]
    fn oversized_manifest_is_rejected_without_unbounded_read() {
        let temp = tempdir().unwrap();
        let source = temp.path().join("source");
        fs::create_dir_all(&source).unwrap();
        let path = source.join(MANIFEST_FILENAME);
        fs::write(&path, b"").unwrap();
        fs::OpenOptions::new()
            .write(true)
            .open(&path)
            .unwrap()
            .set_len(MAX_MANIFEST_BYTES as u64 + 1)
            .unwrap();

        let error = read_install_manifest(&source, "1.4.0").unwrap_err();
        assert_eq!(error.code(), INVALID_MANIFEST);
        assert!(error.message().contains("exceeds"));
    }

    #[test]
    fn declared_artifact_failure_stops_staging_before_copy() {
        let temp = tempdir().unwrap();
        let source = temp.path().join("source");
        write_source(&source, "demo", "<h1>panel</h1>");
        fs::remove_file(source.join("panel.html")).unwrap();
        let dest_parent = temp.path().join("install/orca-samples.demo");
        let manifest = read_manifest(&source);

        let error = stage_plugin_tree(&source, &dest_parent, &manifest).unwrap_err();
        assert_eq!(error.code(), IO_ERROR);
        assert!(!dest_parent.exists());
    }

    #[test]
    fn symlinks_in_the_source_are_rejected() {
        let temp = tempdir().unwrap();
        let source = temp.path().join("source");
        write_source(&source, "demo", "<h1>panel</h1>");
        std::os::unix::fs::symlink("panel.html", source.join("link.html")).unwrap();
        let dest_parent = temp.path().join("install/orca-samples.demo");
        let manifest = read_manifest(&source);

        let error = stage_plugin_tree(&source, &dest_parent, &manifest).unwrap_err();
        assert_eq!(error.code(), SYMLINK_NOT_ALLOWED);
        assert!(!dest_parent.exists());
    }

    #[test]
    fn entry_limits_still_apply_to_the_source_tree() {
        let temp = tempdir().unwrap();
        let source = temp.path().join("source");
        write_source(&source, "demo", "<h1>panel</h1>");
        for index in 0..2100 {
            fs::write(source.join(format!("f{index:04}.txt")), b"x").unwrap();
        }
        let manifest = read_manifest(&source);
        let error = stage_plugin_tree(
            &source,
            &temp.path().join("install/orca-samples.demo"),
            &manifest,
        )
        .unwrap_err();
        assert_eq!(error.code(), TOO_MANY_FILES);
    }
}
