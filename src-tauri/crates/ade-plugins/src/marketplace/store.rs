//! Marketplace persistence: registered sources (`sources.json`) and one cached
//! snapshot per source (`snapshots/<sourceId>.json`).
//!
//! Ported from `orca:src/main/plugins/plugin-marketplace-store.ts`:
//!
//! - Source ids are the first 32 hex chars of
//!   `sha256("orca-plugin-marketplace-source-v1\0<url>\0<ref>")`, so identity
//!   is content-addressed and duplicates collapse.
//! - `orca-marketplace.json` is a strict index: `name`, `owner`, and `plugins`
//!   entries `{id, source{kind,url,ref}, description?, categories}`.
//! - Reads are capped (2 MiB sources, 16 MiB snapshots); a corrupt snapshot is
//!   ignored by [`MarketplaceStore::read_snapshot`] so one bad cache cannot
//!   brick the catalog.

use std::collections::HashSet;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::discovery::is_qualified_plugin_key;
use crate::git::is_allowed_git_url;
use crate::install::atomic_write::write_atomic;
use crate::install::lockfile::is_plugin_commit;
use crate::types::{PluginError, INVALID_MANIFEST, INVALID_PARAMS, IO_ERROR};

pub const MARKETPLACE_FILENAME: &str = "orca-marketplace.json";
pub const MARKETPLACE_SOURCE_LIMIT: usize = 64;
pub const MARKETPLACE_ENTRY_LIMIT: usize = 2_048;
pub const MARKETPLACE_CATEGORY_LIMIT: usize = 16;
pub const OFFICIAL_MARKETPLACE_URL: &str = "https://github.com/stablyai/orca-plugins.git";
pub const OFFICIAL_MARKETPLACE_REF: &str = "main";
pub const OFFICIAL_MARKETPLACE_OWNER: &str = "stablyai";
pub const OFFICIAL_PLUGIN_PUBLISHER: &str = "stablyai";
pub const OFFICIAL_PLUGIN_ID_PREFIX: &str = "orca-";

pub(crate) const SOURCES_FILENAME: &str = "sources.json";
const SOURCES_MAX_BYTES: u64 = 2 * 1024 * 1024;
const SNAPSHOT_MAX_BYTES: u64 = 16 * 1024 * 1024;
const MAX_SOURCE_URL_LENGTH: usize = 32 * 1024;
const MAX_REF_LENGTH: usize = 4_096;
const MAX_MARKETPLACE_NAME_LENGTH: usize = 256;
const MAX_OWNER_LENGTH: usize = 128;
const MAX_DESCRIPTION_LENGTH: usize = 4_096;
const MAX_CATEGORY_LENGTH: usize = 64;

/// One Git marketplace source; mirrors the oracle `pluginMarketplaceGitSourceSchema`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MarketplaceSource {
    pub kind: String,
    pub url: String,
    #[serde(rename = "ref")]
    pub ref_: String,
}

impl MarketplaceSource {
    pub fn git(url: impl Into<String>, ref_: impl Into<String>) -> Self {
        Self {
            kind: "git".to_string(),
            url: url.into(),
            ref_: ref_.into(),
        }
    }

    pub fn official() -> Self {
        Self::git(OFFICIAL_MARKETPLACE_URL, OFFICIAL_MARKETPLACE_REF)
    }

    pub fn validate(&self) -> Result<(), PluginError> {
        if self.kind != "git" {
            return Err(PluginError::new(
                INVALID_MANIFEST,
                "marketplace source kind must be git",
            ));
        }
        if self.url.chars().count() > MAX_SOURCE_URL_LENGTH || !is_allowed_git_url(&self.url) {
            return Err(PluginError::new(
                INVALID_MANIFEST,
                "marketplace Git URL must use HTTPS or SSH",
            ));
        }
        if self.ref_.trim().is_empty() || self.ref_.chars().count() > MAX_REF_LENGTH {
            return Err(PluginError::new(
                INVALID_MANIFEST,
                "marketplace source ref must not be empty",
            ));
        }
        Ok(())
    }
}

/// `sha256("orca-plugin-marketplace-source-v1\0<url>\0<ref>")[..32]`.
pub fn marketplace_source_id(source: &MarketplaceSource) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"orca-plugin-marketplace-source-v1\0");
    hasher.update(source.url.as_bytes());
    hasher.update(b"\0");
    hasher.update(source.ref_.as_bytes());
    hex::encode(hasher.finalize())[..32].to_string()
}

fn is_marketplace_source_id(value: &str) -> bool {
    value.len() == 32
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RegisteredSource {
    pub id: String,
    pub source: MarketplaceSource,
    pub added_at: u64,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SourcesFile {
    schema_version: u32,
    sources: Vec<RegisteredSource>,
}

/// Parsed `orca-marketplace.json`; mirrors the oracle `pluginMarketplaceSchema`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MarketplaceIndex {
    pub name: String,
    pub owner: String,
    pub plugins: Vec<MarketplaceEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MarketplaceEntry {
    pub id: String,
    pub source: MarketplaceSource,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default)]
    pub categories: Vec<String>,
}

impl MarketplaceIndex {
    pub fn validate(&self) -> Result<(), PluginError> {
        if self.name.is_empty() || self.name.chars().count() > MAX_MARKETPLACE_NAME_LENGTH {
            return Err(PluginError::new(
                INVALID_MANIFEST,
                "invalid marketplace name",
            ));
        }
        if !is_marketplace_owner(&self.owner) {
            return Err(PluginError::new(
                INVALID_MANIFEST,
                "invalid marketplace owner",
            ));
        }
        if self.plugins.len() > MARKETPLACE_ENTRY_LIMIT {
            return Err(PluginError::new(
                INVALID_MANIFEST,
                format!("marketplace exceeds the {MARKETPLACE_ENTRY_LIMIT}-entry limit"),
            ));
        }
        let mut seen = HashSet::new();
        for entry in &self.plugins {
            entry.validate()?;
            if !seen.insert(entry.id.as_str()) {
                return Err(PluginError::new(
                    INVALID_MANIFEST,
                    format!("duplicate plugin id: {}", entry.id),
                ));
            }
        }
        Ok(())
    }
}

impl MarketplaceEntry {
    pub fn validate(&self) -> Result<(), PluginError> {
        if !is_qualified_plugin_key(&self.id) {
            return Err(PluginError::new(
                INVALID_MANIFEST,
                format!("invalid qualified plugin key: {}", self.id),
            ));
        }
        self.source.validate()?;
        if let Some(description) = &self.description {
            if description.is_empty() || description.chars().count() > MAX_DESCRIPTION_LENGTH {
                return Err(PluginError::new(
                    INVALID_MANIFEST,
                    "invalid marketplace entry description",
                ));
            }
        }
        if self.categories.len() > MARKETPLACE_CATEGORY_LIMIT {
            return Err(PluginError::new(
                INVALID_MANIFEST,
                format!(
                    "marketplace entry exceeds the {MARKETPLACE_CATEGORY_LIMIT}-category limit"
                ),
            ));
        }
        let mut seen = HashSet::new();
        for category in &self.categories {
            if !is_category_slug(category) {
                return Err(PluginError::new(
                    INVALID_MANIFEST,
                    format!("invalid marketplace category: {category}"),
                ));
            }
            if !seen.insert(category.as_str()) {
                return Err(PluginError::new(
                    INVALID_MANIFEST,
                    format!("duplicate category: {category}"),
                ));
            }
        }
        Ok(())
    }
}

fn is_marketplace_owner(owner: &str) -> bool {
    let length = owner.chars().count();
    if length == 0 || length > MAX_OWNER_LENGTH {
        return false;
    }
    if !owner
        .chars()
        .all(|character| character.is_ascii_alphanumeric() || matches!(character, '.' | '_' | '-'))
    {
        return false;
    }
    let first = owner.chars().next().unwrap_or(' ');
    let last = owner.chars().last().unwrap_or(' ');
    first.is_ascii_alphanumeric() && last.is_ascii_alphanumeric()
}

fn is_category_slug(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_CATEGORY_LENGTH
        && !value.starts_with('-')
        && !value.ends_with('-')
        && !value.contains("--")
        && value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
}

/// Cached marketplace fetch: the commit the source resolved to plus the parsed
/// index. Mirrors the oracle `cachedSnapshotSchema`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MarketplaceSnapshot {
    pub schema_version: u32,
    pub source_id: String,
    pub source: MarketplaceSource,
    pub marketplace_commit: String,
    pub fetched_at: u64,
    pub marketplace: MarketplaceIndex,
}

/// `sources.json` + `snapshots/` under `<data_dir>/marketplaces/`.
#[derive(Debug, Clone)]
pub struct MarketplaceStore {
    root: PathBuf,
}

impl MarketplaceStore {
    pub fn new(data_dir: &Path) -> Self {
        Self {
            root: data_dir.join("marketplaces"),
        }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    fn sources_path(&self) -> PathBuf {
        self.root.join(SOURCES_FILENAME)
    }

    fn snapshot_path(&self, source_id: &str) -> PathBuf {
        self.root
            .join("snapshots")
            .join(format!("{source_id}.json"))
    }

    /// Registered sources in insertion order. Missing state reads as empty; a
    /// corrupt file is an error (the caller decides whether to recover).
    pub fn list_sources(&self) -> Result<Vec<RegisteredSource>, PluginError> {
        let path = self.sources_path();
        if !path.exists() {
            return Ok(Vec::new());
        }
        let bytes = read_bounded(&path, SOURCES_MAX_BYTES)?;
        let file: SourcesFile = serde_json::from_slice(&bytes).map_err(|error| {
            PluginError::new(
                INVALID_MANIFEST,
                format!("marketplace sources are invalid: {error}"),
            )
        })?;
        if file.schema_version != 1 {
            return Err(PluginError::new(
                INVALID_MANIFEST,
                "marketplace sources are invalid: unsupported schema version",
            ));
        }
        if file.sources.len() > MARKETPLACE_SOURCE_LIMIT {
            return Err(PluginError::new(
                INVALID_MANIFEST,
                "marketplace sources are invalid: source limit exceeded",
            ));
        }
        let mut seen = HashSet::new();
        for source in &file.sources {
            source.source.validate().map_err(|error| {
                PluginError::new(
                    INVALID_MANIFEST,
                    format!("marketplace sources are invalid: {}", error.message()),
                )
            })?;
            if !is_marketplace_source_id(&source.id)
                || marketplace_source_id(&source.source) != source.id
                || !seen.insert(source.id.as_str())
            {
                return Err(PluginError::new(
                    INVALID_MANIFEST,
                    "marketplace sources are invalid: source identity is inconsistent or duplicated",
                ));
            }
        }
        Ok(file.sources)
    }

    /// Adds a source once; re-adding the same `{url, ref}` returns the
    /// existing registration.
    pub fn add_source(
        &self,
        source: &MarketplaceSource,
        added_at: u64,
    ) -> Result<RegisteredSource, PluginError> {
        source.validate()?;
        let mut sources = self.list_sources()?;
        let id = marketplace_source_id(source);
        if let Some(existing) = sources.iter().find(|candidate| candidate.id == id) {
            return Ok(existing.clone());
        }
        if sources.len() >= MARKETPLACE_SOURCE_LIMIT {
            return Err(PluginError::new(
                INVALID_PARAMS,
                format!("marketplace source limit ({MARKETPLACE_SOURCE_LIMIT}) reached"),
            ));
        }
        let registered = RegisteredSource {
            id,
            source: source.clone(),
            added_at,
        };
        sources.push(registered.clone());
        self.write_sources(&sources)?;
        Ok(registered)
    }

    /// Removes a source and its cached snapshot. Returns whether it existed.
    pub fn remove_source(&self, source_id: &str) -> Result<bool, PluginError> {
        let mut sources = self.list_sources()?;
        let before = sources.len();
        sources.retain(|candidate| candidate.id != source_id);
        if sources.len() == before {
            return Ok(false);
        }
        self.write_sources(&sources)?;
        let _ = fs::remove_file(self.snapshot_path(source_id));
        Ok(true)
    }

    pub fn write_snapshot(
        &self,
        source: &RegisteredSource,
        marketplace_commit: &str,
        fetched_at: u64,
        marketplace: &MarketplaceIndex,
    ) -> Result<MarketplaceSnapshot, PluginError> {
        source.source.validate()?;
        if marketplace_source_id(&source.source) != source.id {
            return Err(PluginError::new(
                INVALID_MANIFEST,
                "marketplace snapshot source identity does not match its registration",
            ));
        }
        if !is_plugin_commit(marketplace_commit) {
            return Err(PluginError::new(
                INVALID_MANIFEST,
                "invalid marketplace commit",
            ));
        }
        marketplace.validate()?;
        let snapshot = MarketplaceSnapshot {
            schema_version: 1,
            source_id: source.id.clone(),
            source: source.source.clone(),
            marketplace_commit: marketplace_commit.to_string(),
            fetched_at,
            marketplace: marketplace.clone(),
        };
        let path = self.snapshot_path(&source.id);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|error| {
                PluginError::new(
                    IO_ERROR,
                    format!("failed to create the marketplace snapshot directory: {error}"),
                )
            })?;
        }
        let bytes = serde_json::to_vec_pretty(&snapshot).map_err(|error| {
            PluginError::new(
                IO_ERROR,
                format!("failed to serialize the snapshot: {error}"),
            )
        })?;
        write_atomic(&path, &bytes, None)?;
        Ok(snapshot)
    }

    /// Reads one cached snapshot; missing, corrupt, or inconsistent snapshots
    /// are ignored (`None`).
    pub fn read_snapshot(&self, source_id: &str) -> Option<MarketplaceSnapshot> {
        if !is_marketplace_source_id(source_id) {
            return None;
        }
        let bytes = read_bounded(&self.snapshot_path(source_id), SNAPSHOT_MAX_BYTES).ok()?;
        let snapshot: MarketplaceSnapshot = serde_json::from_slice(&bytes).ok()?;
        if snapshot.schema_version != 1
            || snapshot.source_id != source_id
            || marketplace_source_id(&snapshot.source) != source_id
            || !is_plugin_commit(&snapshot.marketplace_commit)
            || snapshot.marketplace.validate().is_err()
        {
            return None;
        }
        Some(snapshot)
    }

    fn write_sources(&self, sources: &[RegisteredSource]) -> Result<(), PluginError> {
        fs::create_dir_all(&self.root).map_err(|error| {
            PluginError::new(
                IO_ERROR,
                format!("failed to create the marketplace directory: {error}"),
            )
        })?;
        let bytes = serde_json::to_vec_pretty(&SourcesFile {
            schema_version: 1,
            sources: sources.to_vec(),
        })
        .map_err(|error| {
            PluginError::new(IO_ERROR, format!("failed to serialize sources: {error}"))
        })?;
        write_atomic(&self.sources_path(), &bytes, None)
    }
}

/// Reads a file through a byte cap; `IO_ERROR` carries the OS detail.
pub(crate) fn read_bounded(path: &Path, max_bytes: u64) -> Result<Vec<u8>, PluginError> {
    let file = fs::File::open(path).map_err(|error| {
        PluginError::new(
            IO_ERROR,
            format!("failed to read {}: {error}", path.display()),
        )
    })?;
    let mut bytes = Vec::new();
    file.take(max_bytes + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| {
            PluginError::new(
                IO_ERROR,
                format!("failed to read {}: {error}", path.display()),
            )
        })?;
    if bytes.len() as u64 > max_bytes {
        return Err(PluginError::new(
            IO_ERROR,
            format!("{} exceeds its {max_bytes}-byte limit", path.display()),
        ));
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tempfile::tempdir;

    const COMMIT: &str = "abababababababababababababababababababab";

    fn index() -> MarketplaceIndex {
        serde_json::from_value(json!({
            "name": "Local Marketplace",
            "owner": "local-owner",
            "plugins": [{
                "id": "acme.demo",
                "source": { "kind": "git", "url": "https://github.com/acme/demo.git", "ref": "main" },
                "description": "A demo",
                "categories": ["demo"]
            }]
        }))
        .unwrap()
    }

    #[test]
    fn adds_lists_and_removes_sources() {
        let temp = tempdir().unwrap();
        let store = MarketplaceStore::new(&temp.path().join("plugins-data"));
        assert!(store.list_sources().unwrap().is_empty());

        let source = MarketplaceSource::git("https://github.com/acme/marketplace.git", "main");
        let registered = store.add_source(&source, 7).unwrap();
        assert_eq!(registered.id, marketplace_source_id(&source));
        assert_eq!(registered.id.len(), 32);
        assert_eq!(registered.added_at, 7);

        let again = store.add_source(&source, 9).unwrap();
        assert_eq!(again.added_at, 7, "re-adding must be idempotent");
        assert_eq!(store.list_sources().unwrap().len(), 1);

        let second = MarketplaceSource::git("git@github.com:acme/other.git", "release");
        store.add_source(&second, 8).unwrap();
        assert_eq!(store.list_sources().unwrap().len(), 2);

        assert!(store.remove_source(&registered.id).unwrap());
        assert_eq!(store.list_sources().unwrap().len(), 1);
        assert!(!store.remove_source(&registered.id).unwrap());
    }

    #[test]
    fn snapshots_round_trip_and_corrupt_snapshots_are_ignored() {
        let temp = tempdir().unwrap();
        let data = temp.path().join("plugins-data");
        let store = MarketplaceStore::new(&data);
        let source = MarketplaceSource::git("https://github.com/acme/marketplace.git", "main");
        let registered = store.add_source(&source, 1).unwrap();

        assert!(store.read_snapshot(&registered.id).is_none());
        let written = store
            .write_snapshot(&registered, COMMIT, 11, &index())
            .unwrap();
        assert_eq!(written.source_id, registered.id);

        let read = store.read_snapshot(&registered.id).unwrap();
        assert_eq!(read, written);
        assert_eq!(read.marketplace.plugins[0].id, "acme.demo");

        let raw: serde_json::Value =
            serde_json::from_slice(&fs::read(store.snapshot_path(&registered.id)).unwrap())
                .unwrap();
        assert_eq!(raw["schemaVersion"], 1);
        assert_eq!(raw["sourceId"], registered.id);
        assert_eq!(raw["marketplaceCommit"], COMMIT);

        fs::write(store.snapshot_path(&registered.id), b"{ not json").unwrap();
        assert!(store.read_snapshot(&registered.id).is_none());

        let mut mismatched = written.clone();
        mismatched.marketplace_commit = "not-a-commit".to_string();
        fs::write(
            store.snapshot_path(&registered.id),
            serde_json::to_vec(&mismatched).unwrap(),
        )
        .unwrap();
        assert!(store.read_snapshot(&registered.id).is_none());

        store.remove_source(&registered.id).unwrap();
        assert!(store.read_snapshot(&registered.id).is_none());
    }

    #[test]
    fn rejects_non_git_sources_and_invalid_indexes() {
        let temp = tempdir().unwrap();
        let store = MarketplaceStore::new(&temp.path().join("plugins-data"));
        let error = store
            .add_source(
                &MarketplaceSource::git("http://github.com/acme/marketplace.git", "main"),
                1,
            )
            .unwrap_err();
        assert_eq!(error.message(), "marketplace Git URL must use HTTPS or SSH");

        let source = MarketplaceSource::git("https://github.com/acme/marketplace.git", "main");
        let registered = store.add_source(&source, 1).unwrap();
        let invalid_commit = store
            .write_snapshot(&registered, "short", 1, &index())
            .unwrap_err();
        assert_eq!(invalid_commit.message(), "invalid marketplace commit");

        let mut invalid = index();
        invalid.plugins[0].id = "not-qualified".to_string();
        assert!(store
            .write_snapshot(&registered, COMMIT, 1, &invalid)
            .is_err());

        let mut duplicate = index();
        duplicate.plugins.push(duplicate.plugins[0].clone());
        assert!(store
            .write_snapshot(&registered, COMMIT, 1, &duplicate)
            .is_err());

        let mut duplicate_categories = index();
        duplicate_categories.plugins[0]
            .categories
            .push("demo".to_string());
        assert!(store
            .write_snapshot(&registered, COMMIT, 1, &duplicate_categories)
            .is_err());
    }

    #[test]
    fn source_id_matches_the_oracle_hash_recipe() {
        let source = MarketplaceSource::official();
        let mut hasher = Sha256::new();
        hasher.update(b"orca-plugin-marketplace-source-v1\0");
        hasher.update(OFFICIAL_MARKETPLACE_URL.as_bytes());
        hasher.update(b"\0");
        hasher.update(OFFICIAL_MARKETPLACE_REF.as_bytes());
        let expected = hex::encode(hasher.finalize())[..32].to_string();
        assert_eq!(marketplace_source_id(&source), expected);
    }
}
