//! Declared-artifact validation at the plugin root.
//!
//! Every path a manifest declares (icon, worker entry, panel/tab entries,
//! language packs, VM recipes, agent profiles) is resolved through symlinks
//! and must land inside the plugin root as a regular file under its per-kind
//! size cap. Ported from
//! `orca:src/main/plugins/plugin-artifact-validation.ts`; the fork adds the
//! `tabs` contribution kind at the same 10 MiB cap as panels.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use crate::manifest::Manifest;
use crate::paths::validate_containment;
use crate::types::{PluginError, CONTENT_TOO_LARGE, IO_ERROR};

pub const PLUGIN_ICON_MAX_BYTES: u64 = 2 * 1024 * 1024;
pub const PLUGIN_WORKER_ENTRY_MAX_BYTES: u64 = 50 * 1024 * 1024;
pub const PLUGIN_PANEL_ENTRY_MAX_BYTES: u64 = 10 * 1024 * 1024;
/// Fork extension: main-area tab entries share the panel cap.
pub const PLUGIN_TAB_ENTRY_MAX_BYTES: u64 = 10 * 1024 * 1024;
pub const PLUGIN_LANGUAGE_PACK_MAX_BYTES: u64 = 5 * 1024 * 1024;
pub const PLUGIN_VM_RECIPE_MAX_BYTES: u64 = 256 * 1024;
pub const PLUGIN_AGENT_PROFILE_MAX_BYTES: u64 = 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PanelEntryArtifact {
    pub id: String,
    pub path: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LanguagePackArtifact {
    pub locale: String,
    pub path: PathBuf,
}

/// Absolute (symlink-resolved) paths of every declared artifact.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ArtifactSet {
    pub icon: Option<PathBuf>,
    pub worker_entry: Option<PathBuf>,
    pub panels: Vec<PanelEntryArtifact>,
    pub tabs: Vec<PanelEntryArtifact>,
    pub language_packs: Vec<LanguagePackArtifact>,
    pub vm_recipes: Vec<PathBuf>,
    pub agents: Vec<PathBuf>,
}

impl ArtifactSet {
    pub fn is_empty(&self) -> bool {
        self.icon.is_none()
            && self.worker_entry.is_none()
            && self.panels.is_empty()
            && self.tabs.is_empty()
            && self.language_packs.is_empty()
            && self.vm_recipes.is_empty()
            && self.agents.is_empty()
    }

    pub fn panel(&self, id: &str) -> Option<&Path> {
        self.panels
            .iter()
            .find(|artifact| artifact.id == id)
            .map(|artifact| artifact.path.as_path())
    }

    pub fn tab(&self, id: &str) -> Option<&Path> {
        self.tabs
            .iter()
            .find(|artifact| artifact.id == id)
            .map(|artifact| artifact.path.as_path())
    }
}

/// Resolves one declared artifact against `root` with the oracle's containment
/// and file checks. Errors carry the bare reason (`not found`, `is not a
/// regular file`, `exceeds the N-byte artifact limit`, containment) without a
/// label or absolute path; [`validate_declared_artifacts`] prefixes the label.
pub fn resolve_declared_artifact(
    root: &Path,
    relative_path: &str,
    max_bytes: u64,
) -> Result<PathBuf, PluginError> {
    let root_real = fs::canonicalize(root).map_err(|error| {
        PluginError::new(
            IO_ERROR,
            format!("failed to resolve the plugin directory: {error}"),
        )
    })?;
    resolve_from_real_root(&root_real, root, relative_path, max_bytes)
}

fn resolve_from_real_root(
    root_real: &Path,
    root: &Path,
    relative_path: &str,
    max_bytes: u64,
) -> Result<PathBuf, PluginError> {
    let mut candidate = root.to_path_buf();
    for segment in relative_path.split(['/', '\\']) {
        candidate.push(segment);
    }
    let resolved = validate_containment(root_real, &candidate)?;
    let metadata = fs::metadata(&resolved)
        .map_err(|error| PluginError::new(IO_ERROR, format!("not accessible: {error}")))?;
    if !metadata.is_file() {
        return Err(PluginError::new(IO_ERROR, "is not a regular file"));
    }
    if metadata.len() > max_bytes {
        return Err(PluginError::new(
            CONTENT_TOO_LARGE,
            format!("exceeds the {max_bytes}-byte artifact limit"),
        ));
    }
    Ok(resolved)
}

pub fn validate_declared_artifacts(
    root: &Path,
    manifest: &Manifest,
) -> Result<ArtifactSet, PluginError> {
    let has_declared = manifest.icon.is_some()
        || manifest.main.is_some()
        || !manifest.contributes.panels.is_empty()
        || !manifest.contributes.tabs.is_empty()
        || !manifest.contributes.language_packs.is_empty()
        || !manifest.contributes.vm_recipes.is_empty()
        || !manifest.contributes.agents.is_empty();
    if !has_declared {
        return Ok(ArtifactSet::default());
    }

    let root_real = fs::canonicalize(root).map_err(|error| {
        PluginError::new(
            IO_ERROR,
            format!("failed to resolve the plugin directory: {error}"),
        )
    })?;
    let mut resolver = ArtifactResolver {
        root,
        root_real,
        cache: HashMap::new(),
    };
    let mut set = ArtifactSet::default();

    if let Some(icon) = &manifest.icon {
        set.icon = Some(resolver.resolve("icon", icon, PLUGIN_ICON_MAX_BYTES)?);
    }
    if let Some(main) = &manifest.main {
        set.worker_entry =
            Some(resolver.resolve("worker entry", main, PLUGIN_WORKER_ENTRY_MAX_BYTES)?);
    }
    for panel in &manifest.contributes.panels {
        let label = format!("panel \"{}\" entry", panel.id);
        let path = resolver.resolve(&label, &panel.entry, PLUGIN_PANEL_ENTRY_MAX_BYTES)?;
        set.panels.push(PanelEntryArtifact {
            id: panel.id.clone(),
            path,
        });
    }
    for tab in &manifest.contributes.tabs {
        let label = format!("tab \"{}\" entry", tab.id);
        let path = resolver.resolve(&label, &tab.entry, PLUGIN_TAB_ENTRY_MAX_BYTES)?;
        set.tabs.push(PanelEntryArtifact {
            id: tab.id.clone(),
            path,
        });
    }
    for pack in &manifest.contributes.language_packs {
        let label = format!("language pack \"{}\"", pack.locale);
        let path = resolver.resolve(&label, &pack.path, PLUGIN_LANGUAGE_PACK_MAX_BYTES)?;
        set.language_packs.push(LanguagePackArtifact {
            locale: pack.locale.clone(),
            path,
        });
    }
    for recipe in &manifest.contributes.vm_recipes {
        set.vm_recipes.push(resolver.resolve(
            "VM recipe",
            &recipe.path,
            PLUGIN_VM_RECIPE_MAX_BYTES,
        )?);
    }
    for agent in &manifest.contributes.agents {
        set.agents.push(resolver.resolve(
            "agent profile",
            &agent.path,
            PLUGIN_AGENT_PROFILE_MAX_BYTES,
        )?);
    }
    Ok(set)
}

struct ArtifactResolver<'a> {
    root: &'a Path,
    root_real: PathBuf,
    /// The oracle skips duplicate declared paths; cache keeps resolve work and
    /// error attribution identical (first label wins).
    cache: HashMap<String, PathBuf>,
}

impl ArtifactResolver<'_> {
    fn resolve(
        &mut self,
        label: &str,
        relative_path: &str,
        max_bytes: u64,
    ) -> Result<PathBuf, PluginError> {
        if let Some(path) = self.cache.get(relative_path) {
            return Ok(path.clone());
        }
        let path = resolve_from_real_root(&self.root_real, self.root, relative_path, max_bytes)
            .map_err(|error| {
                PluginError::new(
                    error.code(),
                    format!("{label} {relative_path}: {}", error.message()),
                )
            })?;
        self.cache.insert(relative_path.to_string(), path.clone());
        Ok(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::parse_manifest;
    use serde_json::{json, Value};
    use std::os::unix::fs::symlink;
    use tempfile::tempdir;

    fn base_manifest() -> Value {
        json!({
            "manifestVersion": 1,
            "id": "demo",
            "publisher": "orca-samples",
            "name": "Demo",
            "version": "1.0.0",
            "engines": { "orca": ">=1.0.0" },
            "pluginApi": 1
        })
    }

    fn built(overrides: Value) -> Manifest {
        let mut value = base_manifest();
        for (key, item) in overrides.as_object().expect("overrides object") {
            value[key.as_str()] = item.clone();
        }
        parse_manifest(&serde_json::to_vec(&value).unwrap()).expect("valid manifest")
    }

    fn expected(root: &Path, relative: &str) -> PathBuf {
        fs::canonicalize(root.join(relative)).unwrap()
    }

    #[test]
    fn cap_constants_match_spec() {
        assert_eq!(PLUGIN_WORKER_ENTRY_MAX_BYTES, 50 * 1024 * 1024);
        assert_eq!(PLUGIN_PANEL_ENTRY_MAX_BYTES, 10 * 1024 * 1024);
        assert_eq!(PLUGIN_TAB_ENTRY_MAX_BYTES, 10 * 1024 * 1024);
        assert_eq!(PLUGIN_ICON_MAX_BYTES, 2 * 1024 * 1024);
        assert_eq!(PLUGIN_LANGUAGE_PACK_MAX_BYTES, 5 * 1024 * 1024);
        assert_eq!(PLUGIN_VM_RECIPE_MAX_BYTES, 256 * 1024);
        assert_eq!(PLUGIN_AGENT_PROFILE_MAX_BYTES, 1024 * 1024);
    }

    #[test]
    fn resolves_every_declared_artifact_kind() {
        let root = tempdir().unwrap();
        for (dir, file) in [
            ("panels", "p.html"),
            ("tabs", "t.html"),
            ("locales", "en-US.json"),
            ("vm", "r.json"),
            ("agents", "a.json"),
        ] {
            fs::create_dir_all(root.path().join(dir)).unwrap();
            fs::write(root.path().join(dir).join(file), b"{}").unwrap();
        }
        fs::write(root.path().join("icon.svg"), b"<svg/>").unwrap();
        fs::write(root.path().join("main.mjs"), b"export default {}").unwrap();

        let manifest = built(json!({
            "icon": "icon.svg",
            "main": "main.mjs",
            "contributes": {
                "panels": [{ "id": "dashboard", "title": "Dashboard", "entry": "panels/p.html" }],
                "tabs": [{ "id": "console", "title": "Console", "entry": "tabs/t.html" }],
                "languagePacks": [{ "locale": "en-US", "path": "locales/en-US.json" }],
                "vmRecipes": [{ "path": "vm/r.json" }],
                "agents": [{ "path": "agents/a.json" }]
            }
        }));

        let set = validate_declared_artifacts(root.path(), &manifest).unwrap();
        assert!(!set.is_empty());
        assert_eq!(set.icon, Some(expected(root.path(), "icon.svg")));
        assert_eq!(set.worker_entry, Some(expected(root.path(), "main.mjs")));
        assert_eq!(
            set.panel("dashboard"),
            Some(expected(root.path(), "panels/p.html").as_path())
        );
        assert_eq!(
            set.tab("console"),
            Some(expected(root.path(), "tabs/t.html").as_path())
        );
        assert_eq!(set.panels.len(), 1);
        assert_eq!(set.tabs.len(), 1);
        assert_eq!(set.language_packs[0].locale, "en-US");
        assert_eq!(
            set.language_packs[0].path,
            expected(root.path(), "locales/en-US.json")
        );
        assert_eq!(set.vm_recipes, vec![expected(root.path(), "vm/r.json")]);
        assert_eq!(set.agents, vec![expected(root.path(), "agents/a.json")]);
    }

    #[test]
    fn manifests_without_declared_artifacts_skip_the_filesystem() {
        let root = tempdir().unwrap();
        let missing = root.path().join("does-not-exist");
        let set = validate_declared_artifacts(&missing, &built(json!({}))).unwrap();
        assert!(set.is_empty());
        assert!(set.panel("anything").is_none());
        assert!(set.tab("anything").is_none());
    }

    #[test]
    fn missing_declared_file_names_the_artifact_and_relative_path() {
        let root = tempdir().unwrap();
        let manifest = built(json!({ "main": "main.mjs" }));
        let error = validate_declared_artifacts(root.path(), &manifest).unwrap_err();
        assert_eq!(error.code(), IO_ERROR);
        assert_eq!(error.message(), "worker entry main.mjs: not found");
    }

    #[test]
    fn directory_in_place_of_file_is_rejected() {
        let root = tempdir().unwrap();
        fs::create_dir_all(root.path().join("assets")).unwrap();
        let manifest = built(json!({
            "contributes": {
                "panels": [{ "id": "dashboard", "title": "Dashboard", "entry": "assets" }]
            }
        }));
        let error = validate_declared_artifacts(root.path(), &manifest).unwrap_err();
        assert_eq!(error.code(), IO_ERROR);
        assert_eq!(
            error.message(),
            "panel \"dashboard\" entry assets: is not a regular file"
        );
    }

    #[test]
    fn per_kind_size_caps_accept_the_boundary_and_reject_oversize() {
        let cases: [(Value, &str, u64); 7] = [
            (
                json!({ "icon": "icon.bin" }),
                "icon.bin",
                PLUGIN_ICON_MAX_BYTES,
            ),
            (
                json!({ "main": "main.bin" }),
                "main.bin",
                PLUGIN_WORKER_ENTRY_MAX_BYTES,
            ),
            (
                json!({ "contributes": { "panels": [{ "id": "p", "title": "P", "entry": "panel.bin" }] } }),
                "panel.bin",
                PLUGIN_PANEL_ENTRY_MAX_BYTES,
            ),
            (
                json!({ "contributes": { "tabs": [{ "id": "t", "title": "T", "entry": "tab.bin" }] } }),
                "tab.bin",
                PLUGIN_TAB_ENTRY_MAX_BYTES,
            ),
            (
                json!({ "contributes": { "languagePacks": [{ "locale": "en-US", "path": "pack.bin" }] } }),
                "pack.bin",
                PLUGIN_LANGUAGE_PACK_MAX_BYTES,
            ),
            (
                json!({ "contributes": { "vmRecipes": [{ "path": "recipe.bin" }] } }),
                "recipe.bin",
                PLUGIN_VM_RECIPE_MAX_BYTES,
            ),
            (
                json!({ "contributes": { "agents": [{ "path": "agent.bin" }] } }),
                "agent.bin",
                PLUGIN_AGENT_PROFILE_MAX_BYTES,
            ),
        ];

        for (overrides, relative, cap) in cases {
            let root = tempdir().unwrap();
            let path = root.path().join(relative);
            fs::write(&path, b"").unwrap();
            fs::OpenOptions::new()
                .write(true)
                .open(&path)
                .unwrap()
                .set_len(cap)
                .unwrap();
            let manifest = built(overrides);

            assert!(
                validate_declared_artifacts(root.path(), &manifest).is_ok(),
                "{relative} at exactly {cap} bytes must pass"
            );

            fs::OpenOptions::new()
                .write(true)
                .open(&path)
                .unwrap()
                .set_len(cap + 1)
                .unwrap();
            let error = validate_declared_artifacts(root.path(), &manifest).unwrap_err();
            assert_eq!(error.code(), CONTENT_TOO_LARGE, "{relative}");
            assert!(
                error
                    .message()
                    .contains(&format!("exceeds the {cap}-byte artifact limit")),
                "{relative}: {}",
                error.message()
            );
        }
    }

    #[test]
    fn symlinked_directory_escape_is_rejected() {
        let root = tempdir().unwrap();
        let outside = tempdir().unwrap();
        fs::write(outside.path().join("panel.html"), b"<h1>outside</h1>").unwrap();
        symlink(outside.path(), root.path().join("escape")).unwrap();

        let manifest = built(json!({
            "contributes": {
                "panels": [{ "id": "p", "title": "P", "entry": "escape/panel.html" }]
            }
        }));
        let error = validate_declared_artifacts(root.path(), &manifest).unwrap_err();
        assert_eq!(error.code(), crate::types::UNSAFE_PATH);
        assert!(error
            .message()
            .contains("resolves outside the plugin directory"));
        assert!(error
            .message()
            .starts_with("panel \"p\" entry escape/panel.html: "));
    }

    #[test]
    fn symlinks_resolving_inside_the_root_are_allowed() {
        let root = tempdir().unwrap();
        let real = root.path().join("real");
        fs::create_dir_all(&real).unwrap();
        fs::write(real.join("panel.html"), b"<h1>inside</h1>").unwrap();
        symlink(&real, root.path().join("alias")).unwrap();

        let manifest = built(json!({
            "contributes": {
                "panels": [{ "id": "p", "title": "P", "entry": "alias/panel.html" }]
            }
        }));
        let set = validate_declared_artifacts(root.path(), &manifest).unwrap();
        assert_eq!(
            set.panel("p"),
            Some(expected(root.path(), "real/panel.html").as_path())
        );
    }

    #[test]
    fn duplicate_declared_paths_are_validated_once() {
        let root = tempdir().unwrap();
        let manifest = built(json!({
            "contributes": {
                "panels": [
                    { "id": "one", "title": "One", "entry": "shared.html" },
                    { "id": "two", "title": "Two", "entry": "shared.html" }
                ]
            }
        }));
        let error = validate_declared_artifacts(root.path(), &manifest).unwrap_err();
        assert_eq!(
            error.message(),
            "panel \"one\" entry shared.html: not found"
        );

        fs::write(root.path().join("shared.html"), b"<h1/>").unwrap();
        let set = validate_declared_artifacts(root.path(), &manifest).unwrap();
        assert_eq!(set.panels.len(), 2);
        assert_eq!(set.panels[0].path, set.panels[1].path);
    }

    #[test]
    fn resolve_declared_artifact_reports_path_free_errors() {
        let root = tempdir().unwrap();
        let error =
            resolve_declared_artifact(root.path(), "main.mjs", PLUGIN_WORKER_ENTRY_MAX_BYTES)
                .unwrap_err();
        assert_eq!(error.message(), "not found");
        assert!(!error.message().contains(root.path().to_str().unwrap()));
    }
}
