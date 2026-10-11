//! Plugin discovery across the global install directory, project install
//! directories, and dev plugin paths.
//!
//! Semantics ported from `orca:src/main/plugins/plugin-discovery.ts` and
//! `plugin-list-projection.ts`, extended for the fork's dual scope model:
//!
//! - Installed plugins live in hash-addressed trees under
//!   `<roots>/<key>/current` -> `<roots>/<key>/<hash>/<manifest>`.
//! - Dev plugin paths shadow installed plugins in the global scope.
//! - A project-scope plugin shadows the same-key global plugin in that
//!   repository's list context (the projection layer decides visibility).
//! - Duplicate plugin keys inside one scope become `invalid` entries with an
//!   error instead of silently dropping one (fork divergence from the
//!   oracle's last-dev-wins, per spec §3.8).
//! - List projections never carry a dev directory: [`DiscoveryProjection`]
//!   has no root field at all and every error message is path-free, so a
//!   private development path cannot leak over the wire.

use std::collections::HashSet;
use std::fs;
use std::io::{ErrorKind, Read};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::artifact::validate_declared_artifacts;
use crate::manifest::{
    parse_manifest, Manifest, LEGACY_MANIFEST_FILENAME, MANIFEST_FILENAME, MAX_MANIFEST_BYTES,
};
use crate::paths::{project_roots, ScopeRoots};
use crate::types::{is_safe_plugin_id, PluginScope};

pub const PLUGIN_CURRENT_POINTER_FILENAME: &str = "current";
pub const PLUGIN_CURRENT_POINTER_MAX_BYTES: u64 = 128;

/// Install-hash shape from the oracle's `PLUGIN_CONTENT_HASH_PATTERN`:
/// 32 or 64 lowercase hex characters.
pub fn is_plugin_content_hash(value: &str) -> bool {
    matches!(value.len(), 32 | 64)
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

/// Canonical install identity `<publisher>.<id>` (also the install dir name).
pub fn is_qualified_plugin_key(value: &str) -> bool {
    let parts: Vec<&str> = value.split('.').collect();
    parts.len() == 2 && is_safe_plugin_id(parts[0]) && is_safe_plugin_id(parts[1])
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PluginSourceKind {
    Installed,
    Dev,
    Bundled,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveredPlugin {
    /// Qualified `<publisher>.<id>`; `None` only when an invalid dev entry has
    /// no readable manifest (the projection then synthesizes a fallback key).
    pub key: Option<String>,
    pub scope: PluginScope,
    pub repo_id: Option<String>,
    /// Absolute plugin root. Never projected for dev entries.
    pub root: PathBuf,
    pub manifest: Option<Manifest>,
    pub source_kind: PluginSourceKind,
    /// Immutable install hash named by the `current` pointer; `None` for dev.
    pub install_hash: Option<String>,
    /// Validation failure for invalid entries.
    pub error: Option<String>,
}

impl DiscoveredPlugin {
    pub fn is_valid(&self) -> bool {
        self.error.is_none()
    }

    pub fn is_dev(&self) -> bool {
        self.source_kind == PluginSourceKind::Dev
    }

    fn valid(
        key: String,
        scope: PluginScope,
        repo_id: Option<String>,
        root: PathBuf,
        manifest: Manifest,
        source_kind: PluginSourceKind,
        install_hash: Option<String>,
    ) -> Self {
        Self {
            key: Some(key),
            scope,
            repo_id,
            root,
            manifest: Some(manifest),
            source_kind,
            install_hash,
            error: None,
        }
    }

    fn invalid(
        key: Option<String>,
        scope: PluginScope,
        repo_id: Option<String>,
        root: PathBuf,
        source_kind: PluginSourceKind,
        error: impl Into<String>,
    ) -> Self {
        Self {
            key,
            scope,
            repo_id,
            root,
            manifest: None,
            source_kind,
            install_hash: None,
            error: Some(error.into()),
        }
    }
}

/// Renderer-safe list projection. Deliberately has no `root`/path field so a
/// dev directory cannot leak; invalid entries without a key receive the
/// oracle's `invalid-development-plugin-<n>` fallback identity.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiscoveryProjection {
    pub plugin_key: String,
    pub scope: PluginScope,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repo_id: Option<String>,
    pub source_kind: PluginSourceKind,
    pub is_dev: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub install_hash: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub manifest: Option<Manifest>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl DiscoveryProjection {
    /// Projects the raw discovery list for one listing context.
    ///
    /// `repo_id = Some(repo)` yields that project's plugins plus global
    /// plugins whose key is not shadowed by a project-scope entry of that
    /// repo (presence-based masking: a broken project install still shadows
    /// the global plugin instead of silently falling back). `repo_id = None`
    /// yields the global-only view.
    pub fn project(discovered: &[DiscoveredPlugin], repo_id: Option<&str>) -> Vec<Self> {
        let shadowed_keys: HashSet<&str> = if repo_id.is_some() {
            discovered
                .iter()
                .filter(|entry| {
                    entry.scope == PluginScope::Project && entry.repo_id.as_deref() == repo_id
                })
                .filter_map(|entry| entry.key.as_deref())
                .collect()
        } else {
            HashSet::new()
        };
        discovered
            .iter()
            .enumerate()
            .filter_map(|(index, entry)| match entry.scope {
                PluginScope::Project => {
                    let matches_context = repo_id.is_some() && entry.repo_id.as_deref() == repo_id;
                    matches_context.then(|| Self::from_entry(entry, index))
                }
                PluginScope::Global => {
                    let shadowed = entry
                        .key
                        .as_deref()
                        .is_some_and(|key| shadowed_keys.contains(key));
                    (!shadowed).then(|| Self::from_entry(entry, index))
                }
            })
            .collect()
    }

    fn from_entry(entry: &DiscoveredPlugin, index: usize) -> Self {
        Self {
            plugin_key: entry
                .key
                .clone()
                .unwrap_or_else(|| format!("invalid-development-plugin-{}", index + 1)),
            scope: entry.scope,
            repo_id: entry.repo_id.clone(),
            source_kind: entry.source_kind,
            is_dev: entry.is_dev(),
            install_hash: entry.install_hash.clone(),
            manifest: entry.manifest.clone(),
            error: entry.error.clone(),
        }
    }
}

/// Discovers installed global plugins, installed project plugins, and dev
/// plugin paths. Directories are read in sorted order so results are
/// deterministic; missing directories simply yield nothing.
pub fn discover(
    global: &ScopeRoots,
    projects: &[(String, PathBuf)],
    dev_paths: &[PathBuf],
) -> Vec<DiscoveredPlugin> {
    let mut discovered: Vec<DiscoveredPlugin> = Vec::new();

    for (name, dir) in installed_entries(&global.install_dir) {
        discovered.push(read_installed_plugin(
            &dir,
            &name,
            PluginScope::Global,
            None,
        ));
    }

    for dev_path in dev_paths {
        let entry = read_dev_plugin(dev_path);
        let valid_key = entry.key.clone().filter(|_| entry.is_valid());
        let Some(key) = valid_key else {
            discovered.push(entry);
            continue;
        };
        let duplicate_dev = discovered.iter().any(|existing| {
            existing.is_valid()
                && existing.source_kind == PluginSourceKind::Dev
                && existing.scope == PluginScope::Global
                && existing.key.as_deref() == Some(key.as_str())
        });
        if duplicate_dev {
            discovered.push(DiscoveredPlugin::invalid(
                None,
                PluginScope::Global,
                None,
                dev_path.clone(),
                PluginSourceKind::Dev,
                format!("duplicate plugin key \"{key}\": another development path provides the same plugin"),
            ));
            continue;
        }
        discovered.retain(|existing| {
            !(existing.is_valid()
                && existing.scope == PluginScope::Global
                && existing.source_kind == PluginSourceKind::Installed
                && existing.key.as_deref() == Some(key.as_str()))
        });
        discovered.push(entry);
    }

    for (repo_id, repo_path) in projects {
        let roots = project_roots(repo_path);
        for (name, dir) in installed_entries(&roots.install_dir) {
            discovered.push(read_installed_plugin(
                &dir,
                &name,
                PluginScope::Project,
                Some(repo_id.clone()),
            ));
        }
    }

    discovered
}

/// Sorted `(dir name, path)` pairs for qualified-key directories. Symlinked
/// directories and non-key names are ignored, mirroring the oracle's
/// `readdir(..., { withFileTypes: true })` filter.
fn installed_entries(install_dir: &Path) -> Vec<(String, PathBuf)> {
    let Ok(read_dir) = fs::read_dir(install_dir) else {
        return Vec::new();
    };
    let mut entries: Vec<(String, PathBuf)> = Vec::new();
    for entry in read_dir.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if !is_qualified_plugin_key(&name) {
            continue;
        }
        if entry.file_type().is_ok_and(|file_type| file_type.is_dir()) {
            entries.push((name, entry.path()));
        }
    }
    entries.sort_by(|left, right| left.0.cmp(&right.0));
    entries
}

fn read_installed_plugin(
    plugin_dir: &Path,
    dir_name: &str,
    scope: PluginScope,
    repo_id: Option<String>,
) -> DiscoveredPlugin {
    let install_hash = match read_current_pointer(plugin_dir) {
        Ok(Some(pointer)) => pointer,
        Ok(None) => {
            return DiscoveredPlugin::invalid(
                Some(dir_name.to_string()),
                scope,
                repo_id,
                plugin_dir.to_path_buf(),
                PluginSourceKind::Installed,
                "missing current-version pointer",
            )
        }
        Err(error) => {
            return DiscoveredPlugin::invalid(
                Some(dir_name.to_string()),
                scope,
                repo_id,
                plugin_dir.to_path_buf(),
                PluginSourceKind::Installed,
                error,
            )
        }
    };
    if !is_plugin_content_hash(&install_hash) {
        return DiscoveredPlugin::invalid(
            Some(dir_name.to_string()),
            scope,
            repo_id,
            plugin_dir.to_path_buf(),
            PluginSourceKind::Installed,
            "corrupt current-version pointer",
        );
    }
    let version_dir = plugin_dir.join(&install_hash);
    let manifest = match read_plugin_manifest(&version_dir) {
        Ok(manifest) => manifest,
        Err(error) => {
            return DiscoveredPlugin::invalid(
                Some(dir_name.to_string()),
                scope,
                repo_id,
                version_dir,
                PluginSourceKind::Installed,
                error,
            )
        }
    };
    let key = manifest.qualified_key();
    if key != dir_name {
        return DiscoveredPlugin::invalid(
            Some(dir_name.to_string()),
            scope,
            repo_id,
            version_dir,
            PluginSourceKind::Installed,
            format!("manifest identity \"{key}\" does not match install directory \"{dir_name}\""),
        );
    }
    if let Err(error) = validate_declared_artifacts(&version_dir, &manifest) {
        return DiscoveredPlugin::invalid(
            Some(dir_name.to_string()),
            scope,
            repo_id,
            version_dir,
            PluginSourceKind::Installed,
            format!("invalid declared artifact: {}", error.message()),
        );
    }
    DiscoveredPlugin::valid(
        key,
        scope,
        repo_id,
        version_dir,
        manifest,
        PluginSourceKind::Installed,
        Some(install_hash),
    )
}

fn read_dev_plugin(dev_path: &Path) -> DiscoveredPlugin {
    let manifest = match read_plugin_manifest(dev_path) {
        Ok(manifest) => manifest,
        Err(error) => {
            return DiscoveredPlugin::invalid(
                None,
                PluginScope::Global,
                None,
                dev_path.to_path_buf(),
                PluginSourceKind::Dev,
                error,
            )
        }
    };
    let key = manifest.qualified_key();
    if let Err(error) = validate_declared_artifacts(dev_path, &manifest) {
        return DiscoveredPlugin::invalid(
            Some(key),
            PluginScope::Global,
            None,
            dev_path.to_path_buf(),
            PluginSourceKind::Dev,
            format!("invalid declared artifact: {}", error.message()),
        );
    }
    DiscoveredPlugin::valid(
        key,
        PluginScope::Global,
        None,
        dev_path.to_path_buf(),
        manifest,
        PluginSourceKind::Dev,
        None,
    )
}

fn read_current_pointer(plugin_dir: &Path) -> Result<Option<String>, String> {
    let path = plugin_dir.join(PLUGIN_CURRENT_POINTER_FILENAME);
    let file = match fs::File::open(&path) {
        Ok(file) => file,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(format!(
                "failed to read the current-version pointer: {error}"
            ))
        }
    };
    let mut bytes = Vec::new();
    file.take(PLUGIN_CURRENT_POINTER_MAX_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("failed to read the current-version pointer: {error}"))?;
    if bytes.len() as u64 > PLUGIN_CURRENT_POINTER_MAX_BYTES {
        return Err("current-version pointer exceeds its size limit".to_string());
    }
    Ok(Some(String::from_utf8_lossy(&bytes).trim().to_string()))
}

fn read_plugin_manifest(root: &Path) -> Result<Manifest, String> {
    let ade_manifest = root.join(MANIFEST_FILENAME);
    let legacy_manifest = root.join(LEGACY_MANIFEST_FILENAME);
    let filename = if ade_manifest.is_file() {
        MANIFEST_FILENAME
    } else if legacy_manifest.is_file() {
        LEGACY_MANIFEST_FILENAME
    } else {
        return Err(format!("missing {MANIFEST_FILENAME}"));
    };
    let bytes = read_manifest_bytes(&root.join(filename), filename)?;
    parse_manifest(&bytes).map_err(|error| format!("invalid manifest: {}", error.message()))
}

fn read_manifest_bytes(path: &Path, filename: &str) -> Result<Vec<u8>, String> {
    let file =
        fs::File::open(path).map_err(|error| format!("failed to read {filename}: {error}"))?;
    let mut bytes = Vec::new();
    file.take(MAX_MANIFEST_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("failed to read {filename}: {error}"))?;
    if bytes.len() > MAX_MANIFEST_BYTES {
        return Err(format!("{filename} exceeds {MAX_MANIFEST_BYTES} bytes"));
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::paths::global_roots;
    use serde_json::{json, Value};
    use std::os::unix::fs::symlink;
    use tempfile::tempdir;

    fn manifest_json(id: &str, publisher: &str, version: &str) -> Value {
        json!({
            "manifestVersion": 1,
            "id": id,
            "publisher": publisher,
            "name": id,
            "version": version,
            "engines": { "orca": ">=1.0.0" },
            "pluginApi": 1
        })
    }

    fn install_plugin(install_dir: &Path, key: &str, hash: &str, manifest: &Value) -> PathBuf {
        let key_dir = install_dir.join(key);
        let version_dir = key_dir.join(hash);
        fs::create_dir_all(&version_dir).unwrap();
        fs::write(key_dir.join(PLUGIN_CURRENT_POINTER_FILENAME), hash).unwrap();
        write_manifest(&version_dir, manifest);
        version_dir
    }

    fn write_manifest(dir: &Path, manifest: &Value) {
        fs::create_dir_all(dir).unwrap();
        fs::write(
            dir.join(MANIFEST_FILENAME),
            serde_json::to_vec(manifest).unwrap(),
        )
        .unwrap();
    }

    fn summary(entry: &DiscoveryProjection) -> (&str, PluginScope, Option<&str>) {
        (
            entry.plugin_key.as_str(),
            entry.scope,
            entry.repo_id.as_deref(),
        )
    }

    #[test]
    fn discovers_installed_global_and_project_plugins() {
        let root = tempdir().unwrap();
        let app_data = root.path().join("app-data");
        let repo = root.path().join("repo");
        let global = global_roots(&app_data);

        let full_hash = "ab".repeat(32);
        let short_hash = "cd".repeat(16);
        install_plugin(
            &global.install_dir,
            "orca-samples.demo",
            &full_hash,
            &manifest_json("demo", "orca-samples", "1.0.0"),
        );
        install_plugin(
            &project_roots(&repo).install_dir,
            "orca-samples.demo",
            &short_hash,
            &manifest_json("demo", "orca-samples", "2.0.0"),
        );

        let discovered = discover(&global, &[("repo-1".to_string(), repo.clone())], &[]);
        assert_eq!(discovered.len(), 2);

        let global_entry = &discovered[0];
        assert_eq!(global_entry.key.as_deref(), Some("orca-samples.demo"));
        assert_eq!(global_entry.scope, PluginScope::Global);
        assert_eq!(global_entry.repo_id, None);
        assert_eq!(global_entry.source_kind, PluginSourceKind::Installed);
        assert_eq!(
            global_entry.install_hash.as_deref(),
            Some(full_hash.as_str())
        );
        assert_eq!(
            global_entry.root,
            global
                .install_dir
                .join("orca-samples.demo")
                .join(&full_hash)
        );
        assert!(global_entry.is_valid());
        assert_eq!(global_entry.manifest.as_ref().unwrap().version, "1.0.0");

        let project_entry = &discovered[1];
        assert_eq!(project_entry.scope, PluginScope::Project);
        assert_eq!(project_entry.repo_id.as_deref(), Some("repo-1"));
        assert_eq!(
            project_entry.install_hash.as_deref(),
            Some(short_hash.as_str())
        );
        assert_eq!(project_entry.manifest.as_ref().unwrap().version, "2.0.0");
    }

    #[test]
    fn legacy_manifest_is_read_and_ade_manifest_wins() {
        let root = tempdir().unwrap();
        let global = global_roots(&root.path().join("app-data"));
        let hash = "11".repeat(32);

        let legacy_dir = install_plugin(
            &global.install_dir,
            "orca-samples.legacy",
            &hash,
            &manifest_json("legacy", "orca-samples", "1.0.0"),
        );
        fs::rename(
            legacy_dir.join(MANIFEST_FILENAME),
            legacy_dir.join(LEGACY_MANIFEST_FILENAME),
        )
        .unwrap();

        let both_dir = install_plugin(
            &global.install_dir,
            "orca-samples.both",
            &hash,
            &manifest_json("both", "orca-samples", "2.0.0"),
        );
        fs::write(
            both_dir.join(LEGACY_MANIFEST_FILENAME),
            serde_json::to_vec(&manifest_json("both", "orca-samples", "1.0.0")).unwrap(),
        )
        .unwrap();

        let discovered = discover(&global, &[], &[]);
        assert_eq!(discovered.len(), 2);
        let legacy = discovered
            .iter()
            .find(|entry| entry.key.as_deref() == Some("orca-samples.legacy"))
            .unwrap();
        assert_eq!(legacy.manifest.as_ref().unwrap().version, "1.0.0");
        let both = discovered
            .iter()
            .find(|entry| entry.key.as_deref() == Some("orca-samples.both"))
            .unwrap();
        assert_eq!(both.manifest.as_ref().unwrap().version, "2.0.0");
    }

    #[test]
    fn pointer_problems_become_invalid_entries_with_install_identity() {
        let root = tempdir().unwrap();
        let global = global_roots(&root.path().join("app-data"));

        let missing_dir = global.install_dir.join("orca-samples.missing");
        fs::create_dir_all(&missing_dir).unwrap();

        let oversized_dir = global.install_dir.join("orca-samples.oversized");
        fs::create_dir_all(&oversized_dir).unwrap();
        fs::write(
            oversized_dir.join(PLUGIN_CURRENT_POINTER_FILENAME),
            "a".repeat(PLUGIN_CURRENT_POINTER_MAX_BYTES as usize + 1),
        )
        .unwrap();

        let corrupt_dir = global.install_dir.join("orca-samples.corrupt");
        fs::create_dir_all(&corrupt_dir).unwrap();
        fs::write(
            corrupt_dir.join(PLUGIN_CURRENT_POINTER_FILENAME),
            "not-a-hash",
        )
        .unwrap();

        let discovered = discover(&global, &[], &[]);
        assert_eq!(discovered.len(), 3);
        for entry in &discovered {
            assert!(!entry.is_valid(), "{:?}", entry.key);
            assert_eq!(entry.source_kind, PluginSourceKind::Installed);
            assert_eq!(entry.install_hash, None);
        }
        let by_key = |key: &str| {
            discovered
                .iter()
                .find(|entry| entry.key.as_deref() == Some(key))
                .unwrap()
        };
        assert_eq!(
            by_key("orca-samples.missing").error.as_deref(),
            Some("missing current-version pointer")
        );
        assert!(by_key("orca-samples.oversized")
            .error
            .as_deref()
            .unwrap()
            .contains("size limit"));
        assert_eq!(
            by_key("orca-samples.corrupt").error.as_deref(),
            Some("corrupt current-version pointer")
        );
        assert_eq!(
            by_key("orca-samples.corrupt").root,
            global.install_dir.join("orca-samples.corrupt")
        );
    }

    #[test]
    fn manifest_problems_become_invalid_entries() {
        let root = tempdir().unwrap();
        let global = global_roots(&root.path().join("app-data"));
        let hash = "22".repeat(32);

        let mismatch_dir = install_plugin(
            &global.install_dir,
            "orca-samples.expected",
            &hash,
            &manifest_json("different", "orca-samples", "1.0.0"),
        );
        let bad_json_dir = install_plugin(
            &global.install_dir,
            "orca-samples.badjson",
            &hash,
            &manifest_json("badjson", "orca-samples", "1.0.0"),
        );
        fs::write(bad_json_dir.join(MANIFEST_FILENAME), b"{ not json").unwrap();
        let missing_manifest_dir = install_plugin(
            &global.install_dir,
            "orca-samples.nomanifest",
            &hash,
            &manifest_json("nomanifest", "orca-samples", "1.0.0"),
        );
        fs::remove_file(missing_manifest_dir.join(MANIFEST_FILENAME)).unwrap();
        let oversized_dir = install_plugin(
            &global.install_dir,
            "orca-samples.toolarge",
            &hash,
            &manifest_json("toolarge", "orca-samples", "1.0.0"),
        );
        let oversized_manifest = oversized_dir.join(MANIFEST_FILENAME);
        fs::write(&oversized_manifest, b"").unwrap();
        fs::OpenOptions::new()
            .write(true)
            .open(&oversized_manifest)
            .unwrap()
            .set_len(MAX_MANIFEST_BYTES as u64 + 1)
            .unwrap();

        let discovered = discover(&global, &[], &[]);
        let message = |key: &str| {
            discovered
                .iter()
                .find(|entry| entry.key.as_deref() == Some(key))
                .unwrap()
                .error
                .clone()
                .unwrap()
        };
        assert!(message("orca-samples.expected").contains("does not match install directory"));
        assert!(message("orca-samples.badjson").contains("invalid manifest"));
        assert!(message("orca-samples.nomanifest").contains("missing ade-plugin.json"));
        assert!(message("orca-samples.toolarge").contains("exceeds"));
        let mismatch = discovered
            .iter()
            .find(|entry| entry.key.as_deref() == Some("orca-samples.expected"))
            .unwrap();
        assert_eq!(mismatch.root, mismatch_dir);
    }

    #[test]
    fn declared_artifact_escape_invalidates_the_entry() {
        let root = tempdir().unwrap();
        let global = global_roots(&root.path().join("app-data"));
        let outside = root.path().join("outside");
        fs::create_dir_all(&outside).unwrap();
        fs::write(outside.join("panel.html"), b"<h1>outside</h1>").unwrap();

        let hash = "33".repeat(32);
        let mut manifest = manifest_json("escape", "orca-samples", "1.0.0");
        manifest["contributes"] = json!({
            "panels": [{ "id": "p", "title": "P", "entry": "escape/panel.html" }]
        });
        let version_dir =
            install_plugin(&global.install_dir, "orca-samples.escape", &hash, &manifest);
        symlink(&outside, version_dir.join("escape")).unwrap();

        let discovered = discover(&global, &[], &[]);
        let entry = &discovered[0];
        assert!(!entry.is_valid());
        let error = entry.error.as_deref().unwrap();
        assert!(error.contains("invalid declared artifact"));
        assert!(error.contains("resolves outside the plugin directory"));
        assert!(!error.contains(root.path().to_str().unwrap()));
    }

    #[test]
    fn dev_plugin_shadows_installed_plugin_in_global_scope() {
        let root = tempdir().unwrap();
        let global = global_roots(&root.path().join("app-data"));
        let hash = "44".repeat(32);
        install_plugin(
            &global.install_dir,
            "orca-samples.demo",
            &hash,
            &manifest_json("demo", "orca-samples", "1.0.0"),
        );

        let dev_path = root.path().join("dev/demo");
        write_manifest(&dev_path, &manifest_json("demo", "orca-samples", "9.9.9"));

        let discovered = discover(&global, &[], std::slice::from_ref(&dev_path));
        assert_eq!(discovered.len(), 1);
        let entry = &discovered[0];
        assert!(entry.is_valid());
        assert_eq!(entry.key.as_deref(), Some("orca-samples.demo"));
        assert_eq!(entry.scope, PluginScope::Global);
        assert_eq!(entry.source_kind, PluginSourceKind::Dev);
        assert_eq!(entry.install_hash, None);
        assert_eq!(entry.root, dev_path);
        assert_eq!(entry.manifest.as_ref().unwrap().version, "9.9.9");
    }

    #[test]
    fn dev_plugin_with_broken_manifest_does_not_shadow_installed() {
        let root = tempdir().unwrap();
        let global = global_roots(&root.path().join("app-data"));
        let hash = "45".repeat(32);
        install_plugin(
            &global.install_dir,
            "orca-samples.demo",
            &hash,
            &manifest_json("demo", "orca-samples", "1.0.0"),
        );

        let dev_path = root.path().join("dev/broken");
        fs::create_dir_all(&dev_path).unwrap();

        let discovered = discover(&global, &[], std::slice::from_ref(&dev_path));
        assert_eq!(discovered.len(), 2);
        assert!(discovered[0].is_valid());
        assert_eq!(discovered[0].source_kind, PluginSourceKind::Installed);
        assert!(!discovered[1].is_valid());
        assert_eq!(discovered[1].source_kind, PluginSourceKind::Dev);
    }

    #[test]
    fn duplicate_dev_keys_produce_an_invalid_entry() {
        let root = tempdir().unwrap();
        let global = global_roots(&root.path().join("app-data"));
        let hash = "46".repeat(32);
        install_plugin(
            &global.install_dir,
            "orca-samples.demo",
            &hash,
            &manifest_json("demo", "orca-samples", "1.0.0"),
        );

        let first_dev = root.path().join("dev/first");
        let second_dev = root.path().join("dev/second");
        write_manifest(&first_dev, &manifest_json("demo", "orca-samples", "1.0.0"));
        write_manifest(&second_dev, &manifest_json("demo", "orca-samples", "2.0.0"));

        let discovered = discover(&global, &[], &[first_dev.clone(), second_dev.clone()]);
        assert_eq!(discovered.len(), 2);
        assert!(discovered[0].is_valid());
        assert_eq!(discovered[0].root, first_dev);
        assert_eq!(discovered[0].manifest.as_ref().unwrap().version, "1.0.0");
        assert!(!discovered[1].is_valid());
        assert_eq!(discovered[1].key, None);
        let error = discovered[1].error.as_deref().unwrap();
        assert!(error.contains("duplicate plugin key \"orca-samples.demo\""));
        assert!(!error.contains(second_dev.to_str().unwrap()));
    }

    #[test]
    fn invalid_dev_entries_get_fallback_projection_keys_without_paths() {
        let root = tempdir().unwrap();
        let global = global_roots(&root.path().join("app-data"));
        let dev_path = root.path().join("dev/broken");
        fs::create_dir_all(&dev_path).unwrap();

        let discovered = discover(&global, &[], std::slice::from_ref(&dev_path));
        let projections = DiscoveryProjection::project(&discovered, None);
        assert_eq!(projections.len(), 1);
        assert_eq!(projections[0].plugin_key, "invalid-development-plugin-1");
        assert!(projections[0].is_dev);
        assert!(projections[0].manifest.is_none());
        assert!(projections[0].error.is_some());

        let json = serde_json::to_string(&projections).unwrap();
        assert!(!json.contains(dev_path.to_str().unwrap()));
        assert!(!json.contains(root.path().to_str().unwrap()));
    }

    #[test]
    fn valid_dev_projection_never_serializes_the_dev_root() {
        let root = tempdir().unwrap();
        let global = global_roots(&root.path().join("app-data"));
        let dev_path = root.path().join("dev/demo");
        write_manifest(&dev_path, &manifest_json("demo", "orca-samples", "1.0.0"));

        let discovered = discover(&global, &[], std::slice::from_ref(&dev_path));
        assert_eq!(discovered[0].root, dev_path);
        let json = serde_json::to_string(&DiscoveryProjection::project(&discovered, None)).unwrap();
        assert!(!json.contains(dev_path.to_str().unwrap()));
    }

    #[test]
    fn project_plugins_mask_globals_in_their_own_repo_context_only() {
        let root = tempdir().unwrap();
        let global = global_roots(&root.path().join("app-data"));
        let repo_one = root.path().join("repo-one");
        let repo_two = root.path().join("repo-two");
        let hash = "55".repeat(32);

        for name in ["alpha", "beta", "gamma"] {
            install_plugin(
                &global.install_dir,
                &format!("orca-samples.{name}"),
                &hash,
                &manifest_json(name, "orca-samples", "1.0.0"),
            );
        }
        for name in ["alpha", "delta"] {
            install_plugin(
                &project_roots(&repo_one).install_dir,
                &format!("orca-samples.{name}"),
                &hash,
                &manifest_json(name, "orca-samples", "2.0.0"),
            );
        }
        install_plugin(
            &project_roots(&repo_two).install_dir,
            "orca-samples.beta",
            &hash,
            &manifest_json("beta", "orca-samples", "3.0.0"),
        );

        let discovered = discover(
            &global,
            &[
                ("repo-1".to_string(), repo_one),
                ("repo-2".to_string(), repo_two),
            ],
            &[],
        );

        let projected = |repo: Option<&str>| {
            DiscoveryProjection::project(&discovered, repo)
                .iter()
                .map(|entry| {
                    (
                        summary(entry).0.to_string(),
                        summary(entry).1,
                        summary(entry).2.map(str::to_string),
                    )
                })
                .collect::<Vec<_>>()
        };

        assert_eq!(
            projected(None),
            vec![
                ("orca-samples.alpha".to_string(), PluginScope::Global, None),
                ("orca-samples.beta".to_string(), PluginScope::Global, None),
                ("orca-samples.gamma".to_string(), PluginScope::Global, None),
            ]
        );
        assert_eq!(
            projected(Some("repo-1")),
            vec![
                ("orca-samples.beta".to_string(), PluginScope::Global, None),
                ("orca-samples.gamma".to_string(), PluginScope::Global, None),
                (
                    "orca-samples.alpha".to_string(),
                    PluginScope::Project,
                    Some("repo-1".to_string())
                ),
                (
                    "orca-samples.delta".to_string(),
                    PluginScope::Project,
                    Some("repo-1".to_string())
                ),
            ]
        );
        assert_eq!(
            projected(Some("repo-2")),
            vec![
                ("orca-samples.alpha".to_string(), PluginScope::Global, None),
                ("orca-samples.gamma".to_string(), PluginScope::Global, None),
                (
                    "orca-samples.beta".to_string(),
                    PluginScope::Project,
                    Some("repo-2".to_string())
                ),
            ]
        );
        assert_eq!(
            projected(Some("unknown-repo")),
            vec![
                ("orca-samples.alpha".to_string(), PluginScope::Global, None),
                ("orca-samples.beta".to_string(), PluginScope::Global, None),
                ("orca-samples.gamma".to_string(), PluginScope::Global, None),
            ]
        );
    }

    #[test]
    fn broken_project_install_still_shadows_the_global_plugin() {
        let root = tempdir().unwrap();
        let global = global_roots(&root.path().join("app-data"));
        let repo = root.path().join("repo");
        let hash = "66".repeat(32);
        install_plugin(
            &global.install_dir,
            "orca-samples.demo",
            &hash,
            &manifest_json("demo", "orca-samples", "1.0.0"),
        );
        let broken_dir = project_roots(&repo).install_dir.join("orca-samples.demo");
        fs::create_dir_all(&broken_dir).unwrap();

        let discovered = discover(&global, &[("repo-1".to_string(), repo)], &[]);
        let global_view = DiscoveryProjection::project(&discovered, None);
        assert_eq!(global_view.len(), 1);
        assert!(!global_view[0].is_dev);
        assert!(global_view[0].manifest.is_some());

        let repo_view = DiscoveryProjection::project(&discovered, Some("repo-1"));
        assert_eq!(repo_view.len(), 1);
        assert_eq!(repo_view[0].scope, PluginScope::Project);
        assert_eq!(repo_view[0].plugin_key, "orca-samples.demo");
        assert!(repo_view[0].error.is_some());
    }

    #[test]
    fn missing_or_non_plugin_directories_discover_nothing() {
        let root = tempdir().unwrap();
        let global = global_roots(&root.path().join("missing-app-data"));
        let repo = root.path().join("missing-repo");

        assert!(discover(&global, &[("repo-1".to_string(), repo)], &[]).is_empty());

        let app_data = root.path().join("missing-app-data");
        fs::create_dir_all(global.install_dir.join("not-a-key")).unwrap();
        fs::create_dir_all(global.install_dir.join("Bad.Key")).unwrap();
        fs::write(global.install_dir.join("orca-samples.file"), b"not a dir").unwrap();
        assert!(discover(&global, &[], &[]).is_empty());
        assert!(app_data.exists());
    }

    #[test]
    fn qualified_plugin_key_predicate_matches_oracle() {
        assert!(is_qualified_plugin_key("orca-samples.demo"));
        assert!(is_qualified_plugin_key("a.b"));
        assert!(!is_qualified_plugin_key("demo"));
        assert!(!is_qualified_plugin_key("a.b.c"));
        assert!(!is_qualified_plugin_key("Bad.demo"));
        assert!(!is_qualified_plugin_key("a.Bad"));
        assert!(!is_qualified_plugin_key("a..b"));
        assert!(!is_qualified_plugin_key(".a"));
        assert!(!is_qualified_plugin_key("a."));
        assert!(!is_qualified_plugin_key("__proto__.demo"));
        assert!(!is_qualified_plugin_key(""));
    }

    #[test]
    fn content_hash_pattern_accepts_32_and_64_lowercase_hex() {
        assert!(is_plugin_content_hash(&"a".repeat(32)));
        assert!(is_plugin_content_hash(&"0123456789abcdef".repeat(4)));
        assert!(!is_plugin_content_hash(&"a".repeat(31)));
        assert!(!is_plugin_content_hash(&"a".repeat(63)));
        assert!(!is_plugin_content_hash(&"A".repeat(64)));
        assert!(!is_plugin_content_hash(&"g".repeat(64)));
        assert!(!is_plugin_content_hash(""));
    }
}
