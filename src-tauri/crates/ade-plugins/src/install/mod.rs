//! Install pipeline: staging, atomic publication, provenance, lockfile,
//! rollback, and uninstall for both scopes.
//!
//! Semantics ported from `orca:src/main/plugins/plugin-install*.ts`:
//! manifest → engine gate → artifact validation → content hash → staged copy
//! with rehash/reparse/revalidate → atomic rename, then publication as
//! provenance → current pointer → lockfile with pointer rollback on lockfile
//! failure, keeping exactly current + one rollback version.
//!
//! Fork specifics:
//! - Every root comes from [`ScopeRoots`] (global `<app_data_dir>`, project
//!   `<repo>/.ade`); the lockfile path is carried by the roots, not derived
//!   from the install directory.
//! - A project-scope install seeds `<repo>/.ade/.gitignore` denying
//!   `plugins/` and `plugin-data/` without ever overwriting an existing file.
//! - The consent fingerprint is injected through [`InstallContext`] so the
//!   install pipeline stays independent of the consent layer (spec §3.1
//!   `consent.rs` computes it; the service layer wires the two together).

use std::fs;
use std::io::{ErrorKind, Write};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::artifact::validate_declared_artifacts;
use crate::content_hash::hash_plugin_tree;
use crate::discovery::{is_plugin_content_hash, is_qualified_plugin_key};
use crate::paths::{validate_containment, ScopeRoots};
use crate::types::{
    PluginError, PluginScope, CONTENT_CHANGED, INVALID_MANIFEST, IO_ERROR, UNSAFE_PATH,
};

pub mod atomic_write;
pub mod lockfile;
pub mod pointer;
pub mod provenance;
pub mod publication;
pub mod staging;

use self::lockfile::{read_lockfile, remove_entry, write_lockfile_atomic, LockEntry, LockSource};
use self::pointer::read_pointer;
use self::provenance::read_provenance;
use self::publication::publish_install;
use self::staging::{read_install_manifest, stage_plugin_tree};

pub const PROJECT_GITIGNORE_FILENAME: &str = ".gitignore";
/// Deny list written once into a project's `.ade` state directory.
pub const PROJECT_GITIGNORE_CONTENT: &str = "plugins/\nplugin-data/\n";

/// Scope context for one install. `repo_id` identifies the project for
/// consent/UI bookkeeping; `host_version` drives the engines gate;
/// `consent_fingerprint` is computed by the consent layer and injected here so
/// the install pipeline carries no consent dependency.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstallContext {
    pub scope: PluginScope,
    pub repo_id: Option<String>,
    pub host_version: String,
    pub consent_fingerprint: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallResult {
    pub plugin_key: String,
    pub version: String,
    pub content_hash: String,
    pub consent_fingerprint: String,
    /// Hash naming the immutable install directory; equals `content_hash` for
    /// the hash-addressed layout and is surfaced separately for discovery
    /// consumers that already speak in pointer terms.
    pub install_hash: String,
    pub scope: PluginScope,
    /// Exact Git commit the installed bytes came from; `None` for local-path
    /// installs (the TS ok-shape requires `resolvedCommit: string | null`).
    pub resolved_commit: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UninstallResult {
    pub removed_install: bool,
    pub removed_data: bool,
}

/// Installs a local plugin directory into the hash-addressed layout for
/// `context.scope`, publishing provenance, pointer, and lockfile.
pub fn install_from_local(
    root: &Path,
    roots: &ScopeRoots,
    context: &InstallContext,
) -> Result<InstallResult, PluginError> {
    let manifest = read_install_manifest(root, &context.host_version)?;
    let plugin_key = manifest.qualified_key();
    if context.scope == PluginScope::Project {
        ensure_project_gitignore(roots)?;
    }
    let plugin_dir = roots.install_dir.join(&plugin_key);
    let (content_hash, version_dir) = stage_plugin_tree(root, &plugin_dir, &manifest)?;
    let staged_manifest = read_install_manifest(&version_dir, &context.host_version)?;
    if staged_manifest.qualified_key() != plugin_key {
        return Err(PluginError::new(
            INVALID_MANIFEST,
            "staged plugin manifest identity does not match its directory",
        ));
    }
    let entry = LockEntry {
        plugin_key: plugin_key.clone(),
        version: staged_manifest.version.clone(),
        source: LockSource::LocalPath {
            path: root.to_string_lossy().into_owned(),
        },
        resolved_commit: None,
        content_hash: content_hash.clone(),
        consent_fingerprint: context.consent_fingerprint.clone(),
        installed_at: now_millis(),
    };
    publish_install(
        roots,
        &plugin_key,
        &content_hash,
        &entry,
        &roots.lockfile_path,
    )?;
    Ok(InstallResult {
        plugin_key,
        version: staged_manifest.version,
        content_hash: content_hash.clone(),
        consent_fingerprint: context.consent_fingerprint.clone(),
        install_hash: content_hash,
        scope: context.scope,
        resolved_commit: None,
    })
}

/// Restores the single retained immutable predecessor. The old consent
/// fingerprint becomes current again, so enablement still fails closed until
/// the user has approved those exact bytes.
pub fn rollback(
    plugin_key: &str,
    roots: &ScopeRoots,
    host_version: &str,
    scope: PluginScope,
) -> Result<InstallResult, PluginError> {
    if !is_qualified_plugin_key(plugin_key) {
        return Err(PluginError::new(
            INVALID_MANIFEST,
            "invalid qualified plugin key",
        ));
    }
    let plugin_dir = roots.install_dir.join(plugin_key);
    let current_content_hash = read_pointer(&plugin_dir)
        .unwrap_or(None)
        .filter(|hash| is_plugin_content_hash(hash));
    let Some(current_content_hash) = current_content_hash else {
        return Err(PluginError::new(
            IO_ERROR,
            "installed plugin has no current version",
        ));
    };
    let mut candidates: Vec<String> = fs::read_dir(&plugin_dir)
        .map_err(|error| {
            PluginError::new(
                IO_ERROR,
                format!("failed to read the plugin directory: {error}"),
            )
        })?
        .flatten()
        .filter(|entry| entry.file_type().is_ok_and(|file_type| file_type.is_dir()))
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| is_plugin_content_hash(name) && name != &current_content_hash)
        .collect();
    candidates.sort();
    let content_hash = match candidates.len() {
        0 => {
            return Err(PluginError::new(
                IO_ERROR,
                "no rollback version is available",
            ))
        }
        1 => candidates.remove(0),
        _ => return Err(PluginError::new(IO_ERROR, "rollback state is ambiguous")),
    };
    let provenance = read_provenance(&plugin_dir, &content_hash)
        .filter(|provenance| {
            provenance.entry.plugin_key == plugin_key
                && provenance.entry.content_hash == content_hash
        })
        .ok_or_else(|| {
            PluginError::new(IO_ERROR, "rollback version has no valid install provenance")
        })?;
    let version_dir = plugin_dir.join(&content_hash);
    let manifest = read_install_manifest(&version_dir, host_version)?;
    if manifest.qualified_key() != plugin_key {
        return Err(PluginError::new(
            INVALID_MANIFEST,
            "rollback version manifest identity does not match its directory",
        ));
    }
    validate_declared_artifacts(&version_dir, &manifest)?;
    let actual_hash = hash_plugin_tree(&version_dir)?;
    if actual_hash != content_hash {
        return Err(PluginError::new(
            CONTENT_CHANGED,
            "rollback version failed integrity verification",
        ));
    }
    let entry = provenance.entry;
    publish_install(
        roots,
        plugin_key,
        &content_hash,
        &entry,
        &roots.lockfile_path,
    )?;
    Ok(InstallResult {
        plugin_key: plugin_key.to_string(),
        version: manifest.version,
        content_hash: content_hash.clone(),
        consent_fingerprint: entry.consent_fingerprint,
        install_hash: content_hash,
        scope,
        resolved_commit: entry.resolved_commit,
    })
}

/// Removes the install directory, the plugin's data directory, and the lock
/// entry, validating each removal target against its realpath root first.
pub fn uninstall(plugin_key: &str, roots: &ScopeRoots) -> Result<UninstallResult, PluginError> {
    if !is_qualified_plugin_key(plugin_key) {
        return Err(PluginError::new(
            INVALID_MANIFEST,
            "invalid qualified plugin key",
        ));
    }
    let removed_install = remove_plugin_directory(&roots.install_dir, plugin_key)?;
    let removed_data = remove_plugin_directory(&roots.data_dir, plugin_key)?;
    let lock = read_lockfile(&roots.lockfile_path);
    let next = remove_entry(&lock, plugin_key);
    write_lockfile_atomic(&roots.lockfile_path, &next)?;
    Ok(UninstallResult {
        removed_install,
        removed_data,
    })
}

fn remove_plugin_directory(root: &Path, plugin_key: &str) -> Result<bool, PluginError> {
    if !root.exists() {
        return Ok(false);
    }
    let target = root.join(plugin_key);
    match validate_containment(root, &target) {
        Ok(_) => {}
        Err(error) if error.code() == UNSAFE_PATH => {
            return Err(PluginError::new(
                UNSAFE_PATH,
                "refusing to remove plugin path outside the scope root",
            ))
        }
        Err(error) if error.code() == IO_ERROR && error.message() == "not found" => {
            return Ok(false)
        }
        Err(error) => return Err(error),
    }
    let metadata = match fs::symlink_metadata(&target) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(false),
        Err(error) => {
            return Err(PluginError::new(
                IO_ERROR,
                format!("failed to inspect the plugin directory: {error}"),
            ))
        }
    };
    let result = if metadata.file_type().is_symlink() {
        fs::remove_file(&target)
    } else {
        fs::remove_dir_all(&target)
    };
    match result {
        Ok(()) => Ok(true),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(false),
        Err(error) => Err(PluginError::new(
            IO_ERROR,
            format!("failed to remove the plugin directory: {error}"),
        )),
    }
}

/// Creates `<repo>/.ade/.gitignore` denying plugin state when absent; an
/// existing file is never touched.
pub fn ensure_project_gitignore(roots: &ScopeRoots) -> Result<(), PluginError> {
    let Some(state_dir) = roots.install_dir.parent() else {
        return Err(PluginError::new(
            IO_ERROR,
            "project roots have no .ade state directory",
        ));
    };
    fs::create_dir_all(state_dir).map_err(|error| {
        PluginError::new(
            IO_ERROR,
            format!("failed to create the project state directory: {error}"),
        )
    })?;
    let path = state_dir.join(PROJECT_GITIGNORE_FILENAME);
    match fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
    {
        Ok(mut file) => file
            .write_all(PROJECT_GITIGNORE_CONTENT.as_bytes())
            .map_err(|error| {
                PluginError::new(
                    IO_ERROR,
                    format!("failed to write {}: {error}", path.display()),
                )
            }),
        Err(error) if error.kind() == ErrorKind::AlreadyExists => Ok(()),
        Err(error) => Err(PluginError::new(
            IO_ERROR,
            format!("failed to create {}: {error}", path.display()),
        )),
    }
}

fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use crate::install::lockfile::{read_lockfile, LockSource};
    use crate::install::pointer::read_pointer;
    use crate::install::{install_from_local, rollback, uninstall, InstallContext};
    use crate::paths::{global_roots, project_roots};
    use crate::types::PluginScope;
    use serde_json::json;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::path::Path;
    use tempfile::tempdir;

    fn write_plugin_source(root: &Path, version: &str, panel: &str) {
        fs::create_dir_all(root).unwrap();
        let manifest = json!({
            "manifestVersion": 1,
            "id": "demo",
            "publisher": "orca-samples",
            "name": "Demo",
            "version": version,
            "engines": { "orca": ">=1.0.0" },
            "pluginApi": 1,
            "contributes": {
                "panels": [{ "id": "panel", "title": "Panel", "entry": "panel.html" }]
            }
        });
        fs::write(
            root.join("ade-plugin.json"),
            serde_json::to_vec(&manifest).unwrap(),
        )
        .unwrap();
        fs::write(root.join("panel.html"), panel).unwrap();
    }

    fn context(scope: PluginScope, consent_fingerprint: &str) -> InstallContext {
        InstallContext {
            scope,
            repo_id: None,
            host_version: "1.4.0".to_string(),
            consent_fingerprint: consent_fingerprint.to_string(),
        }
    }

    fn version_dirs(install_dir: &Path, key: &str) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(install_dir.join(key))
            .unwrap()
            .flatten()
            .filter(|entry| entry.file_type().unwrap().is_dir())
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .filter(|name| name.len() == 64 && name.bytes().all(|b| b.is_ascii_hexdigit()))
            .collect();
        names.sort();
        names
    }

    #[test]
    fn installs_a_local_plugin_end_to_end() {
        let temp = tempdir().unwrap();
        let source = temp.path().join("source");
        let roots = global_roots(&temp.path().join("app-data"));
        write_plugin_source(&source, "1.0.0", "<h1>one</h1>");

        let result =
            install_from_local(&source, &roots, &context(PluginScope::Global, "sha256-one"))
                .expect("install succeeds");

        assert_eq!(result.plugin_key, "orca-samples.demo");
        assert_eq!(result.version, "1.0.0");
        assert_eq!(result.scope, PluginScope::Global);
        assert_eq!(result.consent_fingerprint, "sha256-one");
        assert_eq!(result.install_hash, result.content_hash);
        assert_eq!(result.content_hash.len(), 64);

        let plugin_dir = roots.install_dir.join("orca-samples.demo");
        let version_dir = plugin_dir.join(&result.content_hash);
        assert_eq!(
            fs::read(version_dir.join("panel.html")).unwrap(),
            b"<h1>one</h1>"
        );
        assert_eq!(
            read_pointer(&plugin_dir).unwrap().as_deref(),
            Some(result.content_hash.as_str())
        );

        let lock = read_lockfile(&roots.lockfile_path);
        let entry = lock.plugins.get("orca-samples.demo").expect("lock entry");
        assert_eq!(entry.version, "1.0.0");
        assert_eq!(entry.content_hash, result.content_hash);
        assert_eq!(entry.consent_fingerprint, "sha256-one");
        assert_eq!(
            entry.source,
            LockSource::LocalPath {
                path: source.to_string_lossy().into_owned()
            }
        );
        assert_eq!(entry.resolved_commit, None);

        let raw: serde_json::Value =
            serde_json::from_slice(&fs::read(&roots.lockfile_path).unwrap()).unwrap();
        let raw_entry = &raw["plugins"]["orca-samples.demo"];
        assert_eq!(raw_entry["capabilityHash"], "sha256-one");
        assert!(raw_entry.get("consentFingerprint").is_none());

        let provenance = plugin_dir
            .join(".install-provenance")
            .join(format!("{}.json", result.content_hash));
        let mode = fs::metadata(&provenance).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }

    #[test]
    fn reinstalling_the_same_content_is_idempotent() {
        let temp = tempdir().unwrap();
        let source = temp.path().join("source");
        let roots = global_roots(&temp.path().join("app-data"));
        write_plugin_source(&source, "1.0.0", "<h1>one</h1>");

        let first =
            install_from_local(&source, &roots, &context(PluginScope::Global, "sha256-a")).unwrap();
        let lock_before = fs::read(&roots.lockfile_path).unwrap();
        let pointer_before = read_pointer(&roots.install_dir.join("orca-samples.demo")).unwrap();

        let second =
            install_from_local(&source, &roots, &context(PluginScope::Global, "sha256-a")).unwrap();

        assert_eq!(first.content_hash, second.content_hash);
        assert_eq!(
            read_pointer(&roots.install_dir.join("orca-samples.demo")).unwrap(),
            pointer_before
        );
        assert_eq!(fs::read(&roots.lockfile_path).unwrap(), lock_before);
        assert_eq!(
            version_dirs(&roots.install_dir, "orca-samples.demo"),
            vec![first.content_hash.clone()]
        );
    }

    #[test]
    fn lockfile_write_failure_rolls_back_the_pointer() {
        let temp = tempdir().unwrap();
        let source = temp.path().join("source");
        let app_data = temp.path().join("app-data");
        let roots = global_roots(&app_data);
        write_plugin_source(&source, "1.0.0", "<h1>one</h1>");
        let first =
            install_from_local(&source, &roots, &context(PluginScope::Global, "sha256-a")).unwrap();

        write_plugin_source(&source, "1.0.0", "<h1>two</h1>");
        let blocked = temp.path().join("blocked");
        fs::write(&blocked, b"not a directory").unwrap();
        let mut broken_roots = global_roots(&app_data);
        broken_roots.lockfile_path = blocked.join("plugins.lock.json");

        let failed = install_from_local(
            &source,
            &broken_roots,
            &context(PluginScope::Global, "sha256-a"),
        );
        assert!(failed.is_err(), "lockfile publication must fail");

        let plugin_dir = roots.install_dir.join("orca-samples.demo");
        assert_eq!(
            read_pointer(&plugin_dir).unwrap().as_deref(),
            Some(first.content_hash.as_str())
        );
        let lock = read_lockfile(&roots.lockfile_path);
        assert_eq!(
            lock.plugins["orca-samples.demo"].content_hash,
            first.content_hash
        );
    }

    #[test]
    fn fresh_install_lockfile_failure_removes_the_pointer() {
        let temp = tempdir().unwrap();
        let source = temp.path().join("source");
        let app_data = temp.path().join("app-data");
        write_plugin_source(&source, "1.0.0", "<h1>one</h1>");
        let blocked = temp.path().join("blocked");
        fs::write(&blocked, b"not a directory").unwrap();
        let mut roots = global_roots(&app_data);
        roots.lockfile_path = blocked.join("plugins.lock.json");

        let failed = install_from_local(&source, &roots, &context(PluginScope::Global, "sha256-a"));
        assert!(failed.is_err());
        let plugin_dir = roots.install_dir.join("orca-samples.demo");
        assert_eq!(read_pointer(&plugin_dir).unwrap(), None);
        assert!(!plugin_dir.join("current").exists());
    }

    #[test]
    fn rollback_restores_the_previous_version_and_keeps_current_plus_one() {
        let temp = tempdir().unwrap();
        let source = temp.path().join("source");
        let roots = global_roots(&temp.path().join("app-data"));
        let mut hashes = Vec::new();
        for (panel, fingerprint) in [
            ("<h1>one</h1>", "sha256-one"),
            ("<h1>two</h1>", "sha256-two"),
            ("<h1>three</h1>", "sha256-three"),
        ] {
            write_plugin_source(&source, "1.0.0", panel);
            let result =
                install_from_local(&source, &roots, &context(PluginScope::Global, fingerprint))
                    .unwrap();
            hashes.push(result.content_hash);
        }
        assert_eq!(version_dirs(&roots.install_dir, "orca-samples.demo"), {
            let mut retained = hashes[1..].to_vec();
            retained.sort();
            retained
        });

        let rolled = rollback("orca-samples.demo", &roots, "1.4.0", PluginScope::Global)
            .expect("rollback succeeds");

        assert_eq!(rolled.content_hash, hashes[1]);
        assert_eq!(rolled.consent_fingerprint, "sha256-two");
        assert_eq!(rolled.install_hash, hashes[1]);
        let plugin_dir = roots.install_dir.join("orca-samples.demo");
        assert_eq!(
            read_pointer(&plugin_dir).unwrap().as_deref(),
            Some(hashes[1].as_str())
        );
        let lock = read_lockfile(&roots.lockfile_path);
        assert_eq!(lock.plugins["orca-samples.demo"].content_hash, hashes[1]);
        assert_eq!(
            lock.plugins["orca-samples.demo"].consent_fingerprint,
            "sha256-two"
        );
        assert_eq!(
            lock.plugins["orca-samples.demo"].source,
            LockSource::LocalPath {
                path: source.to_string_lossy().into_owned()
            }
        );
        assert_eq!(version_dirs(&roots.install_dir, "orca-samples.demo"), {
            let mut retained = hashes[1..].to_vec();
            retained.sort();
            retained
        });
    }

    #[test]
    fn uninstall_removes_install_data_directories_and_lock_entry() {
        let temp = tempdir().unwrap();
        let source = temp.path().join("source");
        let other = temp.path().join("other");
        let roots = global_roots(&temp.path().join("app-data"));
        write_plugin_source(&source, "1.0.0", "<h1>one</h1>");
        install_from_local(&source, &roots, &context(PluginScope::Global, "sha256-a")).unwrap();
        fs::create_dir_all(&other).unwrap();
        fs::write(
            other.join("ade-plugin.json"),
            serde_json::to_vec(&json!({
                "manifestVersion": 1,
                "id": "other",
                "publisher": "orca-samples",
                "name": "Other",
                "version": "1.0.0",
                "engines": { "orca": ">=1.0.0" },
                "pluginApi": 1
            }))
            .unwrap(),
        )
        .unwrap();
        install_from_local(&other, &roots, &context(PluginScope::Global, "sha256-b")).unwrap();
        let data_dir = roots.data_dir.join("orca-samples.demo");
        fs::create_dir_all(&data_dir).unwrap();
        fs::write(data_dir.join("storage.json"), b"{}").unwrap();

        let removed = uninstall("orca-samples.demo", &roots).unwrap();

        assert!(removed.removed_install);
        assert!(removed.removed_data);
        assert!(!roots.install_dir.join("orca-samples.demo").exists());
        assert!(!data_dir.exists());
        let lock = read_lockfile(&roots.lockfile_path);
        assert!(!lock.plugins.contains_key("orca-samples.demo"));
        assert!(lock.plugins.contains_key("orca-samples.other"));
    }

    #[test]
    fn uninstall_rejects_unqualified_keys_and_symlinks_escaping_the_root() {
        let temp = tempdir().unwrap();
        let roots = global_roots(&temp.path().join("app-data"));
        fs::create_dir_all(&roots.install_dir).unwrap();
        assert!(uninstall("../escape", &roots).is_err());

        let outside = temp.path().join("outside");
        fs::create_dir_all(&outside).unwrap();
        fs::write(outside.join("keep.txt"), b"keep").unwrap();
        std::os::unix::fs::symlink(&outside, roots.install_dir.join("orca-samples.demo")).unwrap();

        let error = uninstall("orca-samples.demo", &roots).unwrap_err();
        assert_eq!(error.code(), crate::types::UNSAFE_PATH);
        assert!(error.message().contains("outside"));
        assert_eq!(fs::read(outside.join("keep.txt")).unwrap(), b"keep");
    }

    #[test]
    fn project_install_writes_ade_gitignore_and_never_overwrites_it() {
        let temp = tempdir().unwrap();
        let source = temp.path().join("source");
        write_plugin_source(&source, "1.0.0", "<h1>one</h1>");

        let repo = temp.path().join("repo");
        let roots = project_roots(&repo);
        fs::create_dir_all(repo.join(".ade")).unwrap();
        fs::write(repo.join(".ade").join(".gitignore"), b"# custom\n").unwrap();
        install_from_local(&source, &roots, &context(PluginScope::Project, "sha256-a")).unwrap();
        assert_eq!(
            fs::read(repo.join(".ade").join(".gitignore")).unwrap(),
            b"# custom\n"
        );

        let fresh_repo = temp.path().join("fresh-repo");
        let fresh_roots = project_roots(&fresh_repo);
        let result = install_from_local(
            &source,
            &fresh_roots,
            &context(PluginScope::Project, "sha256-a"),
        )
        .unwrap();
        assert_eq!(result.scope, PluginScope::Project);
        assert_eq!(
            fs::read(fresh_repo.join(".ade").join(".gitignore")).unwrap(),
            b"plugins/\nplugin-data/\n"
        );
        assert!(fresh_roots.lockfile_path.exists());
    }

    #[test]
    fn same_content_reinstall_backfills_and_preserves_accepted_provenance() {
        let temp = tempdir().unwrap();
        let first_source = temp.path().join("first-source");
        let second_source = temp.path().join("second-source");
        let roots = global_roots(&temp.path().join("app-data"));
        write_plugin_source(&first_source, "1.0.0", "<h1>one</h1>");
        write_plugin_source(&second_source, "1.0.0", "<h1>one</h1>");

        let first = install_from_local(
            &first_source,
            &roots,
            &context(PluginScope::Global, "sha256-a"),
        )
        .unwrap();
        let plugin_dir = roots.install_dir.join("orca-samples.demo");
        fs::remove_file(
            plugin_dir
                .join(".install-provenance")
                .join(format!("{}.json", first.content_hash)),
        )
        .unwrap();

        let second = install_from_local(
            &second_source,
            &roots,
            &context(PluginScope::Global, "sha256-b"),
        )
        .unwrap();

        assert_eq!(second.content_hash, first.content_hash);
        let lock = read_lockfile(&roots.lockfile_path);
        let entry = &lock.plugins["orca-samples.demo"];
        assert_eq!(
            entry.source,
            LockSource::LocalPath {
                path: first_source.to_string_lossy().into_owned(),
            }
        );
        assert_eq!(entry.consent_fingerprint, "sha256-a");

        let provenance = plugin_dir
            .join(".install-provenance")
            .join(format!("{}.json", first.content_hash));
        assert!(provenance.is_file(), "provenance must be backfilled");
    }

    #[test]
    fn refuses_to_repoint_at_a_tampered_existing_version() {
        let temp = tempdir().unwrap();
        let source = temp.path().join("source");
        let roots = global_roots(&temp.path().join("app-data"));
        write_plugin_source(&source, "1.0.0", "<h1>one</h1>");
        let first =
            install_from_local(&source, &roots, &context(PluginScope::Global, "sha256-a")).unwrap();
        let version_dir = roots
            .install_dir
            .join("orca-samples.demo")
            .join(&first.content_hash);
        fs::write(version_dir.join("panel.html"), "<h1>tampered</h1>").unwrap();

        let second = install_from_local(&source, &roots, &context(PluginScope::Global, "sha256-a"));

        let error = second.expect_err("tampered install must be rejected");
        assert!(
            error.message().contains("integrity verification"),
            "unexpected error: {}",
            error.message()
        );
    }

    #[test]
    fn rejects_a_plugin_whose_engine_requirement_is_not_met() {
        let temp = tempdir().unwrap();
        let source = temp.path().join("source");
        let roots = global_roots(&temp.path().join("app-data"));
        write_plugin_source(&source, "1.0.0", "<h1>one</h1>");
        let mut manifest: serde_json::Value =
            serde_json::from_slice(&fs::read(source.join("ade-plugin.json")).unwrap()).unwrap();
        manifest["engines"]["orca"] = json!(">=9.0.0");
        fs::write(
            source.join("ade-plugin.json"),
            serde_json::to_vec(&manifest).unwrap(),
        )
        .unwrap();

        let error = install_from_local(&source, &roots, &context(PluginScope::Global, "sha256-a"))
            .expect_err("engine gate rejects");
        assert_eq!(error.code(), crate::types::ENGINE_INCOMPATIBLE);
        assert!(error.message().contains(">=9.0.0"));
    }

    #[test]
    fn rejects_a_source_without_a_manifest() {
        let temp = tempdir().unwrap();
        let source = temp.path().join("source");
        fs::create_dir_all(&source).unwrap();
        let roots = global_roots(&temp.path().join("app-data"));

        let error = install_from_local(&source, &roots, &context(PluginScope::Global, "sha256-a"))
            .expect_err("missing manifest rejects");
        assert_eq!(error.code(), crate::types::INVALID_MANIFEST);
        assert!(error.message().contains("missing ade-plugin.json"));
    }
}
