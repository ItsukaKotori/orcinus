//! Install lockfile v1: where each installed plugin came from, the exact
//! commit it resolved to, the content hash of the installed tree, and the
//! consent fingerprint the user accepted.
//!
//! Ported from `orca:src/shared/plugins/plugin-install-lockfile.ts` and
//! `orca:src/main/plugins/plugin-install-lockfile-store.ts`:
//!
//! - Serialization keeps the v1 on-disk name `capabilityHash` so older hosts
//!   can still read new lockfiles; reads accept either `capabilityHash` or
//!   `consentFingerprint`.
//! - A corrupt, oversized, or invalid file reads as an empty lockfile — the
//!   hash-addressed install directories remain the integrity anchor.
//! - Reads are capped at 5 MiB so a corrupt local file cannot turn a list
//!   refresh into an unbounded allocation.
//! - `is_plugin_git_url` allows HTTPS (no user/password) and SSH sources only
//!   (scp-like `user@host:path` included); system Git remote helpers such as
//!   `ext::` would turn URL parsing into command execution.

use std::collections::BTreeMap;
use std::fs;
use std::io::Read;
use std::path::Path;

use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::discovery::{is_plugin_content_hash, is_qualified_plugin_key};
use crate::git::is_allowed_git_url;
use crate::install::atomic_write::write_atomic;
use crate::types::{PluginError, INVALID_MANIFEST, IO_ERROR};

pub const PLUGIN_LOCKFILE_MAX_BYTES: u64 = 5 * 1024 * 1024;
const PLUGIN_COMMIT_LENGTHS: [usize; 2] = [40, 64];
const MAX_SOURCE_URL_LENGTH: usize = 32 * 1024;
const MAX_REF_LENGTH: usize = 4096;
const MAX_VERSION_LENGTH: usize = 128;
const MAX_FINGERPRINT_LENGTH: usize = 256;

/// Git object id, SHA-1 or SHA-256.
pub fn is_plugin_commit(value: &str) -> bool {
    PLUGIN_COMMIT_LENGTHS.contains(&value.len())
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

/// System Git supports executable remote helpers (`ext::`, custom `foo::`
/// transports). Only network Git over HTTPS or SSH is accepted.
pub fn is_plugin_git_url(value: &str) -> bool {
    let url = value.trim();
    if is_scp_like_git_url(url) {
        return true;
    }
    let Some((scheme, rest)) = url.split_once("://") else {
        return false;
    };
    let scheme = scheme.to_ascii_lowercase();
    if scheme != "https" && scheme != "ssh" {
        return false;
    }
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    if authority.is_empty() {
        return false;
    }
    let (userinfo, host) = match authority.rsplit_once('@') {
        Some((userinfo, host)) => (Some(userinfo), host),
        None => (None, authority),
    };
    if host.is_empty() {
        return false;
    }
    if let Some(userinfo) = userinfo {
        let (username, password) = match userinfo.split_once(':') {
            Some((username, password)) => (username, Some(password)),
            None => (userinfo, None),
        };
        // The oracle checks `parsed.password` truthiness: an empty password is
        // allowed, any password text is not (both schemes).
        if password.is_some_and(|password| !password.is_empty()) {
            return false;
        }
        if scheme == "https" && !username.is_empty() {
            return false;
        }
    }
    true
}

fn is_scp_like_git_url(url: &str) -> bool {
    let Some((user_host, path)) = url.split_once(':') else {
        return false;
    };
    let Some((user, host)) = user_host.split_once('@') else {
        return false;
    };
    if user.is_empty()
        || user
            .chars()
            .any(|character| character.is_whitespace() || matches!(character, '@' | '/' | ':'))
    {
        return false;
    }
    if host.is_empty()
        || !host
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'.' || byte == b'-')
    {
        return false;
    }
    !path.is_empty() && !path.chars().any(char::is_whitespace)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MarketplaceSourceRef {
    pub url: String,
    #[serde(rename = "ref")]
    pub ref_: String,
    pub resolved_commit: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MarketplacePluginRef {
    pub url: String,
    #[serde(rename = "ref")]
    pub ref_: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum LockSource {
    LocalPath {
        path: String,
    },
    Git {
        url: String,
        #[serde(default, rename = "ref")]
        ref_: String,
    },
    Marketplace {
        marketplace: MarketplaceSourceRef,
        plugin: MarketplacePluginRef,
    },
    Bundled {
        #[serde(rename = "bundleId")]
        bundle_id: String,
    },
}

impl LockSource {
    fn validate(&self) -> Result<(), String> {
        match self {
            LockSource::LocalPath { path } => {
                check_string_length("source.path", path, 1, MAX_SOURCE_URL_LENGTH)
            }
            LockSource::Git { url, ref_ } => {
                check_string_length("source.url", url, 1, MAX_SOURCE_URL_LENGTH)?;
                if !is_allowed_git_url(url) {
                    return Err("git URL must use HTTPS or SSH".to_string());
                }
                check_string_length("source.ref", ref_, 0, MAX_REF_LENGTH)
            }
            LockSource::Marketplace {
                marketplace,
                plugin,
            } => {
                check_string_length(
                    "source.marketplace.url",
                    &marketplace.url,
                    1,
                    MAX_SOURCE_URL_LENGTH,
                )?;
                if !is_allowed_git_url(&marketplace.url) {
                    return Err("marketplace Git URL must use HTTPS or SSH".to_string());
                }
                check_string_length(
                    "source.marketplace.ref",
                    &marketplace.ref_,
                    1,
                    MAX_REF_LENGTH,
                )?;
                if !is_plugin_commit(&marketplace.resolved_commit) {
                    return Err("invalid marketplace resolved commit".to_string());
                }
                check_string_length("source.plugin.url", &plugin.url, 1, MAX_SOURCE_URL_LENGTH)?;
                if !is_allowed_git_url(&plugin.url) {
                    return Err("plugin Git URL must use HTTPS or SSH".to_string());
                }
                check_string_length("source.plugin.ref", &plugin.ref_, 1, MAX_REF_LENGTH)
            }
            LockSource::Bundled { bundle_id } => {
                if !is_qualified_plugin_key(bundle_id) {
                    return Err("invalid bundled plugin identity".to_string());
                }
                Ok(())
            }
        }
    }
}

fn check_string_length(path: &str, value: &str, min: usize, max: usize) -> Result<(), String> {
    let length = value.chars().count();
    if length < min {
        return Err(format!("{path} must contain at least {min} character(s)"));
    }
    if length > max {
        return Err(format!("{path} must contain at most {max} character(s)"));
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LockEntry {
    pub plugin_key: String,
    pub version: String,
    pub source: LockSource,
    /// Commit the git source resolved to at install time; `None` for local.
    pub resolved_commit: Option<String>,
    /// Deterministic hash of the installed tree (also the install dir name).
    pub content_hash: String,
    /// Fingerprint the user accepted; serialized as legacy `capabilityHash`.
    pub consent_fingerprint: String,
    pub installed_at: u64,
}

impl LockEntry {
    pub fn validate(&self) -> Result<(), PluginError> {
        self.validate_inner().map_err(|message| {
            PluginError::new(INVALID_MANIFEST, format!("invalid lock entry: {message}"))
        })
    }

    fn validate_inner(&self) -> Result<(), String> {
        if !is_qualified_plugin_key(&self.plugin_key) {
            return Err("invalid qualified plugin key".to_string());
        }
        check_string_length("version", &self.version, 1, MAX_VERSION_LENGTH)?;
        if !is_plugin_content_hash(&self.content_hash) {
            return Err("invalid content hash".to_string());
        }
        check_string_length(
            "consentFingerprint",
            &self.consent_fingerprint,
            1,
            MAX_FINGERPRINT_LENGTH,
        )?;
        if let Some(commit) = &self.resolved_commit {
            if !is_plugin_commit(commit) {
                return Err("invalid resolved commit".to_string());
            }
        }
        self.source.validate()
    }
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LockEntryWire {
    plugin_key: String,
    version: String,
    source: LockSource,
    resolved_commit: Option<String>,
    content_hash: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    consent_fingerprint: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    capability_hash: Option<String>,
    installed_at: u64,
}

impl Serialize for LockEntry {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        LockEntryWire {
            plugin_key: self.plugin_key.clone(),
            version: self.version.clone(),
            source: self.source.clone(),
            resolved_commit: self.resolved_commit.clone(),
            content_hash: self.content_hash.clone(),
            consent_fingerprint: None,
            capability_hash: Some(self.consent_fingerprint.clone()),
            installed_at: self.installed_at,
        }
        .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for LockEntry {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let LockEntryWire {
            plugin_key,
            version,
            source,
            resolved_commit,
            content_hash,
            consent_fingerprint,
            capability_hash,
            installed_at,
        } = LockEntryWire::deserialize(deserializer)?;
        let entry = LockEntry {
            plugin_key,
            version,
            source,
            resolved_commit,
            content_hash,
            consent_fingerprint: consent_fingerprint
                .or(capability_hash)
                .ok_or_else(|| D::Error::custom("consent fingerprint is required"))?,
            installed_at,
        };
        entry.validate_inner().map_err(D::Error::custom)?;
        Ok(entry)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Lockfile {
    pub version: u32,
    pub plugins: BTreeMap<String, LockEntry>,
}

impl Lockfile {
    pub fn empty() -> Self {
        Self {
            version: 1,
            plugins: BTreeMap::new(),
        }
    }

    pub fn entry(&self, plugin_key: &str) -> Option<&LockEntry> {
        self.plugins.get(plugin_key)
    }
}

#[derive(Deserialize)]
struct LockfileWire {
    version: u32,
    plugins: BTreeMap<String, LockEntry>,
}

/// Parses lockfile bytes; anything invalid yields an empty lockfile so a
/// corrupt local file cannot brick installs.
pub fn parse_lockfile_bytes(bytes: &[u8]) -> Lockfile {
    let Ok(wire) = serde_json::from_slice::<LockfileWire>(bytes) else {
        return Lockfile::empty();
    };
    if wire.version != 1 {
        return Lockfile::empty();
    }
    if wire
        .plugins
        .iter()
        .any(|(key, entry)| !is_qualified_plugin_key(key) || &entry.plugin_key != key)
    {
        return Lockfile::empty();
    }
    Lockfile {
        version: 1,
        plugins: wire.plugins,
    }
}

/// Reads and validates a lockfile through the byte cap; missing, oversized,
/// or corrupt files read as empty.
pub fn read_lockfile(path: &Path) -> Lockfile {
    let Ok(file) = fs::File::open(path) else {
        return Lockfile::empty();
    };
    let mut bytes = Vec::new();
    if file
        .take(PLUGIN_LOCKFILE_MAX_BYTES + 1)
        .read_to_end(&mut bytes)
        .is_err()
    {
        return Lockfile::empty();
    }
    if bytes.len() as u64 > PLUGIN_LOCKFILE_MAX_BYTES {
        return Lockfile::empty();
    }
    parse_lockfile_bytes(&bytes)
}

pub fn write_lockfile_atomic(path: &Path, lock: &Lockfile) -> Result<(), PluginError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| {
            PluginError::new(
                IO_ERROR,
                format!("failed to create the lockfile directory: {error}"),
            )
        })?;
    }
    let json = serde_json::to_vec_pretty(lock).map_err(|error| {
        PluginError::new(IO_ERROR, format!("failed to serialize lockfile: {error}"))
    })?;
    write_atomic(path, &json, None)
}

pub fn upsert_entry(lock: &Lockfile, entry: LockEntry) -> Lockfile {
    let mut plugins = lock.plugins.clone();
    plugins.insert(entry.plugin_key.clone(), entry);
    Lockfile {
        version: 1,
        plugins,
    }
}

pub fn remove_entry(lock: &Lockfile, plugin_key: &str) -> Lockfile {
    let mut plugins = lock.plugins.clone();
    plugins.remove(plugin_key);
    Lockfile {
        version: 1,
        plugins,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tempfile::tempdir;

    const HASH: &str = "abababababababababababababababababababababababababababababababab";

    fn local_entry() -> LockEntry {
        LockEntry {
            plugin_key: "orca-samples.demo".to_string(),
            version: "1.0.0".to_string(),
            source: LockSource::LocalPath {
                path: "/tmp/source".to_string(),
            },
            resolved_commit: None,
            content_hash: HASH.to_string(),
            consent_fingerprint: "sha256-abc".to_string(),
            installed_at: 1_700_000_000_000,
        }
    }

    #[test]
    fn serializes_legacy_capability_hash_without_consent_fingerprint() {
        let value = serde_json::to_value(local_entry()).unwrap();
        assert_eq!(value["capabilityHash"], "sha256-abc");
        assert!(value.get("consentFingerprint").is_none());
        assert!(value["resolvedCommit"].is_null());
    }

    #[test]
    fn reads_legacy_capability_hash_entries() {
        let raw = json!({
            "version": 1,
            "plugins": {
                "orca-samples.demo": {
                    "pluginKey": "orca-samples.demo",
                    "version": "1.0.0",
                    "source": { "kind": "local-path", "path": "/tmp/source" },
                    "resolvedCommit": null,
                    "contentHash": HASH,
                    "capabilityHash": "sha256-legacy",
                    "installedAt": 5
                }
            }
        });
        let lock = parse_lockfile_bytes(&serde_json::to_vec(&raw).unwrap());
        assert_eq!(
            lock.plugins["orca-samples.demo"].consent_fingerprint,
            "sha256-legacy"
        );

        let raw = json!({
            "version": 1,
            "plugins": {
                "orca-samples.demo": {
                    "pluginKey": "orca-samples.demo",
                    "version": "1.0.0",
                    "source": { "kind": "local-path", "path": "/tmp/source" },
                    "resolvedCommit": null,
                    "contentHash": HASH,
                    "consentFingerprint": "sha256-new",
                    "installedAt": 5
                }
            }
        });
        let lock = parse_lockfile_bytes(&serde_json::to_vec(&raw).unwrap());
        assert_eq!(
            lock.plugins["orca-samples.demo"].consent_fingerprint,
            "sha256-new"
        );
    }

    #[test]
    fn corrupt_or_invalid_lockfiles_read_empty() {
        for raw in [
            json!(null),
            json!({ "version": 2, "plugins": {} }),
            json!({ "plugins": {} }),
            json!({ "version": 1, "plugins": [] }),
            json!({
                "version": 1,
                "plugins": {
                    "Bad.Key": {
                        "pluginKey": "Bad.Key",
                        "version": "1.0.0",
                        "source": { "kind": "local-path", "path": "/tmp/x" },
                        "resolvedCommit": null,
                        "contentHash": HASH,
                        "capabilityHash": "fp",
                        "installedAt": 1
                    }
                }
            }),
            json!({
                "version": 1,
                "plugins": {
                    "orca-samples.demo": {
                        "pluginKey": "orca-samples.other",
                        "version": "1.0.0",
                        "source": { "kind": "local-path", "path": "/tmp/x" },
                        "resolvedCommit": null,
                        "contentHash": HASH,
                        "capabilityHash": "fp",
                        "installedAt": 1
                    }
                }
            }),
            json!({
                "version": 1,
                "plugins": {
                    "orca-samples.demo": {
                        "pluginKey": "orca-samples.demo",
                        "version": "1.0.0",
                        "source": { "kind": "local-path", "path": "/tmp/x" },
                        "resolvedCommit": null,
                        "contentHash": "not-a-hash",
                        "capabilityHash": "fp",
                        "installedAt": 1
                    }
                }
            }),
            json!({
                "version": 1,
                "plugins": {
                    "orca-samples.demo": {
                        "pluginKey": "orca-samples.demo",
                        "version": "1.0.0",
                        "source": { "kind": "local-path", "path": "/tmp/x" },
                        "resolvedCommit": null,
                        "contentHash": HASH,
                        "installedAt": 1
                    }
                }
            }),
        ] {
            assert_eq!(
                parse_lockfile_bytes(&serde_json::to_vec(&raw).unwrap()),
                Lockfile::empty(),
                "expected empty lockfile for {raw}"
            );
        }
        assert_eq!(parse_lockfile_bytes(b"{ not json"), Lockfile::empty());
    }

    #[test]
    fn read_lockfile_caps_oversized_files_at_five_mib() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("plugins.lock.json");
        fs::write(&path, b"").unwrap();
        let file = fs::OpenOptions::new().write(true).open(&path).unwrap();
        file.set_len(PLUGIN_LOCKFILE_MAX_BYTES + 1).unwrap();
        drop(file);
        assert_eq!(read_lockfile(&path), Lockfile::empty());

        fs::write(&path, b"garbage").unwrap();
        assert_eq!(read_lockfile(&path), Lockfile::empty());
        assert_eq!(
            read_lockfile(&dir.path().join("missing.json")),
            Lockfile::empty()
        );
    }

    #[test]
    fn upsert_and_remove_entries_are_pure() {
        let lock = Lockfile::empty();
        let with_entry = upsert_entry(&lock, local_entry());
        assert!(lock.plugins.is_empty());
        assert_eq!(with_entry.plugins.len(), 1);
        assert_eq!(
            with_entry.entry("orca-samples.demo").unwrap().version,
            "1.0.0"
        );

        let mut updated = local_entry();
        updated.version = "2.0.0".to_string();
        let replaced = upsert_entry(&with_entry, updated);
        assert_eq!(
            replaced.entry("orca-samples.demo").unwrap().version,
            "2.0.0"
        );

        let removed = remove_entry(&replaced, "orca-samples.demo");
        assert!(removed.plugins.is_empty());
        assert_eq!(replaced.plugins.len(), 1);
    }

    #[test]
    fn write_lockfile_atomic_round_trips_pretty_json() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("nested/plugins.lock.json");
        let lock = upsert_entry(&Lockfile::empty(), local_entry());
        write_lockfile_atomic(&path, &lock).unwrap();
        assert_eq!(read_lockfile(&path), lock);
        let text = fs::read_to_string(&path).unwrap();
        assert!(text.contains("\n  \"version\": 1"));
        assert!(text.contains("\"capabilityHash\""));
    }

    #[test]
    fn git_url_whitelist_matches_oracle() {
        for allowed in [
            "https://github.com/example/demo.git",
            "https://github.com/example/demo.git#v1",
            "ssh://git@github.com/example/demo.git",
            "ssh://github.com/example/demo.git",
            "ssh://user:@github.com/example/demo.git",
            "git@github.com:example/demo.git",
            "  https://github.com/example/demo.git  ",
        ] {
            assert!(is_plugin_git_url(allowed), "expected allowed: {allowed}");
        }
        for rejected in [
            "http://github.com/example/demo.git",
            "git://github.com/example/demo.git",
            "file:///tmp/demo",
            "ext::sh -c whoami",
            "fd::/tmp/demo",
            "https://user@github.com/example/demo.git",
            "https://user:pass@github.com/example/demo.git",
            "ssh://user:pass@github.com/example/demo.git",
            "ssh://:pass@github.com/example/demo.git",
            "https://",
            "https:///path",
            "not a url",
            "",
        ] {
            assert!(
                !is_plugin_git_url(rejected),
                "expected rejected: {rejected}"
            );
        }
    }

    #[test]
    fn lock_entry_validation_matches_oracle_shapes() {
        let mut entry = local_entry();
        entry.resolved_commit = Some("a".repeat(40));
        assert!(entry.validate().is_ok());
        entry.resolved_commit = Some("A".repeat(40));
        assert!(entry.validate().is_err());
        entry.resolved_commit = Some("a".repeat(39));
        assert!(entry.validate().is_err());

        let mut entry = local_entry();
        entry.consent_fingerprint = String::new();
        assert!(entry.validate().is_err());

        let mut entry = local_entry();
        entry.source = LockSource::Git {
            url: "http://example.com/demo.git".to_string(),
            ref_: String::new(),
        };
        assert!(entry.validate().is_err());

        let mut entry = local_entry();
        entry.source = LockSource::Git {
            url: "https://example.com/demo.git".to_string(),
            ref_: "main".to_string(),
        };
        assert!(entry.validate().is_ok());

        let mut entry = local_entry();
        entry.source = LockSource::Marketplace {
            marketplace: MarketplaceSourceRef {
                url: "https://example.com/marketplace.git".to_string(),
                ref_: "main".to_string(),
                resolved_commit: "b".repeat(40),
            },
            plugin: MarketplacePluginRef {
                url: "https://example.com/demo.git".to_string(),
                ref_: "v1.0.0".to_string(),
            },
        };
        assert!(entry.validate().is_ok());

        let mut entry = local_entry();
        entry.source = LockSource::Bundled {
            bundle_id: "stablyai.orca-demo".to_string(),
        };
        assert!(entry.validate().is_ok());
        entry.source = LockSource::Bundled {
            bundle_id: "not-qualified".to_string(),
        };
        assert!(entry.validate().is_err());
    }

    #[test]
    fn marketplace_source_round_trips_camel_case_fields() {
        let entry = LockEntry {
            source: LockSource::Marketplace {
                marketplace: MarketplaceSourceRef {
                    url: "https://example.com/marketplace.git".to_string(),
                    ref_: "main".to_string(),
                    resolved_commit: "c".repeat(40),
                },
                plugin: MarketplacePluginRef {
                    url: "https://example.com/demo.git".to_string(),
                    ref_: "main".to_string(),
                },
            },
            ..local_entry()
        };
        let value = serde_json::to_value(&entry).unwrap();
        assert_eq!(value["source"]["kind"], "marketplace");
        assert_eq!(
            value["source"]["marketplace"]["resolvedCommit"],
            "c".repeat(40)
        );
        assert_eq!(value["source"]["plugin"]["ref"], "main");
        let parsed: LockEntry = serde_json::from_value(value).unwrap();
        assert_eq!(parsed, entry);
    }

    #[test]
    fn git_source_ref_defaults_to_empty_string() {
        let raw = json!({
            "version": 1,
            "plugins": {
                "orca-samples.demo": {
                    "pluginKey": "orca-samples.demo",
                    "version": "1.0.0",
                    "source": { "kind": "git", "url": "https://example.com/demo.git" },
                    "resolvedCommit": "d".repeat(40),
                    "contentHash": HASH,
                    "capabilityHash": "fp",
                    "installedAt": 1
                }
            }
        });
        let lock = parse_lockfile_bytes(&serde_json::to_vec(&raw).unwrap());
        assert_eq!(
            lock.plugins["orca-samples.demo"].source,
            LockSource::Git {
                url: "https://example.com/demo.git".to_string(),
                ref_: String::new(),
            }
        );
    }
}
