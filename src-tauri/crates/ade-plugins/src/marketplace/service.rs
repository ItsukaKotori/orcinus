//! Marketplace service: source registration, git refresh into cached
//! snapshots, merged listings, and host-derived official trust.
//!
//! Ported from `orca:src/main/plugins/plugin-marketplace-service.ts` +
//! `plugin-marketplace-provenance.ts` + `plugin-marketplace-projection.ts`:
//!
//! - `refresh(source_id?)` clones the source repository through [`GitRunner`],
//!   parses `orca-marketplace.json`, validates provenance, and writes the
//!   snapshot. No HTTP client is involved.
//! - Official trust is host-projected: the source URL must be the official
//!   marketplace (`github.com/stablyai/orca-plugins`), the index owner must be
//!   `stablyai`, the entry identity `stablyai.orca-*`, and the plugin URL must
//!   live under the `stablyai` organization.
//! - Reserved identities (`stablyai.*` or `*.orca-*`) may only resolve to
//!   `github.com/stablyai`, otherwise the snapshot is refused.

use std::path::Path;

use serde::Serialize;

use crate::discovery::is_qualified_plugin_key;
use crate::git::GitRunner;
use crate::marketplace::store::{
    marketplace_source_id, read_bounded, MarketplaceEntry, MarketplaceIndex, MarketplaceSnapshot,
    MarketplaceSource, MarketplaceStore, RegisteredSource, MARKETPLACE_FILENAME,
    OFFICIAL_MARKETPLACE_OWNER, OFFICIAL_PLUGIN_ID_PREFIX, OFFICIAL_PLUGIN_PUBLISHER,
};
use crate::marketplace::{now_millis, TempWorkspace};
use crate::types::{PluginError, INVALID_MANIFEST, INVALID_PARAMS};

const MARKETPLACE_INDEX_MAX_BYTES: u64 = 16 * 1024 * 1024;

/// Official seed: `https://github.com/stablyai/orca-plugins.git#main`.
pub fn official_marketplace_source() -> MarketplaceSource {
    MarketplaceSource::official()
}

/// Registers, refreshes, and projects Git marketplace sources.
#[derive(Debug, Clone)]
pub struct MarketplaceService {
    store: MarketplaceStore,
    runner: GitRunner,
}

/// Snapshot metadata projection attached to a registered source.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MarketplaceProjection {
    pub name: String,
    pub owner: String,
    pub resolved_commit: String,
    pub fetched_at: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceState {
    pub id: String,
    pub source: MarketplaceSource,
    pub added_at: u64,
    pub marketplace: Option<MarketplaceProjection>,
    pub official: bool,
}

/// One merged catalog entry with its host-derived trust projection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MarketplaceListing {
    pub marketplace_source_id: String,
    pub marketplace_name: String,
    pub marketplace_owner: String,
    pub marketplace: MarketplaceSource,
    pub marketplace_commit: String,
    pub plugin_key: String,
    pub source: MarketplaceSource,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub categories: Vec<String>,
    pub official: bool,
    pub bundled: bool,
}

impl MarketplaceService {
    pub fn new(data_dir: &Path) -> Self {
        Self::with_runner(data_dir, GitRunner::default())
    }

    pub fn with_runner(data_dir: &Path, runner: GitRunner) -> Self {
        Self {
            store: MarketplaceStore::new(data_dir),
            runner,
        }
    }

    pub fn store(&self) -> &MarketplaceStore {
        &self.store
    }

    pub fn runner(&self) -> &GitRunner {
        &self.runner
    }

    /// Registered sources with their cached snapshot projections.
    pub fn list_sources(&self) -> Result<Vec<SourceState>, PluginError> {
        self.store
            .list_sources()?
            .into_iter()
            .map(|source| {
                let snapshot = self.store.read_snapshot(&source.id);
                Ok(state_from_snapshot(&source, snapshot.as_ref()))
            })
            .collect()
    }

    /// Registers `source` (idempotent by `{url, ref}`) and fetches its first
    /// snapshot; a failed first fetch rolls the registration back.
    pub fn add_source(
        &self,
        source: &MarketplaceSource,
        added_at: u64,
    ) -> Result<SourceState, PluginError> {
        source.validate()?;
        let existing = self
            .store
            .list_sources()?
            .into_iter()
            .find(|candidate| candidate.id == marketplace_source_id(source));
        let was_new = existing.is_none();
        let registered = match existing {
            Some(registered) => registered,
            None => self.store.add_source(source, added_at)?,
        };
        match self.fetch_source(&registered) {
            Ok(snapshot) => Ok(state_from_snapshot(&registered, Some(&snapshot))),
            Err(error) => {
                if was_new {
                    let _ = self.store.remove_source(&registered.id);
                }
                Err(error)
            }
        }
    }

    /// The official seed cannot be removed; other sources drop with their
    /// snapshot.
    pub fn remove_source(&self, source_id: &str) -> Result<bool, PluginError> {
        if let Some(source) = self
            .store
            .list_sources()?
            .into_iter()
            .find(|candidate| candidate.id == source_id)
        {
            if is_official_marketplace_git_source(&source.source.url) {
                return Err(PluginError::new(
                    INVALID_PARAMS,
                    "the official marketplace is managed by Orca and cannot be removed",
                ));
            }
        }
        self.store.remove_source(source_id)
    }

    /// Ensures the official source is registered, seeded, and fetched. An
    /// existing valid snapshot is served without another fetch.
    pub fn seed_official(&self, added_at: u64) -> Result<SourceState, PluginError> {
        let existing = self
            .store
            .list_sources()?
            .into_iter()
            .find(|candidate| is_official_marketplace_git_source(&candidate.source.url));
        let registered = match existing {
            Some(registered) => registered,
            None => self
                .store
                .add_source(&official_marketplace_source(), added_at)?,
        };
        if let Some(snapshot) = self.store.read_snapshot(&registered.id) {
            return Ok(state_from_snapshot(&registered, Some(&snapshot)));
        }
        let snapshot = self.fetch_source(&registered)?;
        Ok(state_from_snapshot(&registered, Some(&snapshot)))
    }

    /// Refreshes one source or every registered source (clone/fetch + parse +
    /// snapshot write). Unknown source ids are rejected.
    pub fn refresh(&self, source_id: Option<&str>) -> Result<Vec<SourceState>, PluginError> {
        let sources = self.store.list_sources()?;
        let selected: Vec<RegisteredSource> = match source_id {
            Some(source_id) => sources
                .into_iter()
                .filter(|source| source.id == source_id)
                .collect(),
            None => sources,
        };
        if let Some(source_id) = source_id {
            if selected.is_empty() {
                return Err(PluginError::new(
                    INVALID_PARAMS,
                    format!("unknown marketplace source: {source_id}"),
                ));
            }
        }
        let mut states = Vec::with_capacity(selected.len());
        for source in selected {
            let snapshot = self.fetch_source(&source)?;
            states.push(state_from_snapshot(&source, Some(&snapshot)));
        }
        Ok(states)
    }

    /// Merged listings across all cached snapshots, sorted by plugin key then
    /// source id.
    pub fn list_plugins(&self) -> Result<Vec<MarketplaceListing>, PluginError> {
        let mut listings = Vec::new();
        for source in self.store.list_sources()? {
            let Some(snapshot) = self.store.read_snapshot(&source.id) else {
                continue;
            };
            for entry in &snapshot.marketplace.plugins {
                listings.push(listing_from_entry(&source, &snapshot, entry));
            }
        }
        listings.sort_by(|left, right| {
            format!("{}\0{}", left.plugin_key, left.marketplace_source_id).cmp(&format!(
                "{}\0{}",
                right.plugin_key, right.marketplace_source_id
            ))
        });
        Ok(listings)
    }

    pub fn find_plugin(
        &self,
        source_id: &str,
        plugin_key: &str,
    ) -> Result<Option<MarketplaceListing>, PluginError> {
        let Some(source) = self
            .store
            .list_sources()?
            .into_iter()
            .find(|candidate| candidate.id == source_id)
        else {
            return Ok(None);
        };
        let Some(snapshot) = self.store.read_snapshot(&source.id) else {
            return Ok(None);
        };
        Ok(snapshot
            .marketplace
            .plugins
            .iter()
            .find(|entry| entry.id == plugin_key)
            .map(|entry| listing_from_entry(&source, &snapshot, entry)))
    }

    fn fetch_source(&self, source: &RegisteredSource) -> Result<MarketplaceSnapshot, PluginError> {
        let workspace = TempWorkspace::create("ade-plugin-marketplace-")?;
        let destination = workspace.path().join("repo");
        let commit = self.runner.clone_depth1(
            &source.source.url,
            Some(&source.source.ref_),
            &destination,
        )?;
        let index = read_marketplace_index(&destination)?;
        validate_marketplace_provenance(&source.source, &index)?;
        self.store
            .write_snapshot(source, &commit, now_millis(), &index)
    }
}

/// Reads and validates `orca-marketplace.json` from a checked-out marketplace
/// tree.
pub fn read_marketplace_index(root: &Path) -> Result<MarketplaceIndex, PluginError> {
    let path = root.join(MARKETPLACE_FILENAME);
    let bytes = read_bounded(&path, MARKETPLACE_INDEX_MAX_BYTES).map_err(|error| {
        PluginError::new(
            INVALID_MANIFEST,
            format!("missing {MARKETPLACE_FILENAME}: {}", error.message()),
        )
    })?;
    let index: MarketplaceIndex = serde_json::from_slice(&bytes).map_err(|error| {
        PluginError::new(
            INVALID_MANIFEST,
            format!("invalid {MARKETPLACE_FILENAME}: {error}"),
        )
    })?;
    index.validate()?;
    Ok(index)
}

fn state_from_snapshot(
    source: &RegisteredSource,
    snapshot: Option<&MarketplaceSnapshot>,
) -> SourceState {
    SourceState {
        id: source.id.clone(),
        source: source.source.clone(),
        added_at: source.added_at,
        marketplace: snapshot.map(|snapshot| MarketplaceProjection {
            name: snapshot.marketplace.name.clone(),
            owner: snapshot.marketplace.owner.clone(),
            resolved_commit: snapshot.marketplace_commit.clone(),
            fetched_at: snapshot.fetched_at,
        }),
        official: is_official_marketplace_git_source(&source.source.url),
    }
}

fn listing_from_entry(
    source: &RegisteredSource,
    snapshot: &MarketplaceSnapshot,
    entry: &MarketplaceEntry,
) -> MarketplaceListing {
    let official = is_official_marketplace_git_source(&source.source.url)
        && snapshot
            .marketplace
            .owner
            .eq_ignore_ascii_case(OFFICIAL_MARKETPLACE_OWNER)
        && is_official_plugin_identity(&entry.id)
        && is_official_organization_git_source(&entry.source.url);
    MarketplaceListing {
        marketplace_source_id: source.id.clone(),
        marketplace_name: snapshot.marketplace.name.clone(),
        marketplace_owner: snapshot.marketplace.owner.clone(),
        marketplace: source.source.clone(),
        marketplace_commit: snapshot.marketplace_commit.clone(),
        plugin_key: entry.id.clone(),
        source: entry.source.clone(),
        description: entry.description.clone(),
        categories: entry.categories.clone(),
        official,
        bundled: false,
    }
}

/// `github.com/stablyai/orca-plugins`.
pub const OFFICIAL_MARKETPLACE_REPOSITORY: &str = "orca-plugins";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitRepositoryIdentity {
    pub host: String,
    pub owner: String,
    pub repository: String,
}

/// Extracts host + first owner segment from the HTTPS/SSH forms the installer
/// accepts; a provenance parser only.
pub fn parse_git_repository_identity(url: &str) -> Option<GitRepositoryIdentity> {
    let trimmed = url.trim();
    if let Some(identity) = parse_scp_repository_identity(trimmed) {
        return Some(identity);
    }
    let (scheme, rest) = trimmed.split_once("://")?;
    if scheme != "https" && scheme != "ssh" {
        return None;
    }
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    let host = match authority.rsplit_once('@') {
        Some((_, host)) => host,
        None => authority,
    };
    repository_identity(host, &rest[authority.len()..])
}

fn parse_scp_repository_identity(url: &str) -> Option<GitRepositoryIdentity> {
    let (user_host, path) = url.split_once(':')?;
    let (user, host) = user_host.split_once('@')?;
    if user.is_empty() {
        return None;
    }
    if host.is_empty()
        || !host
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'.' || byte == b'-')
    {
        return None;
    }
    repository_identity(host, path)
}

fn repository_identity(host: &str, repository_path: &str) -> Option<GitRepositoryIdentity> {
    let segments: Vec<&str> = repository_path
        .split('/')
        .filter(|segment| !segment.is_empty())
        .collect();
    if segments.len() < 2 || host.is_empty() {
        return None;
    }
    let last = segments.last()?;
    let repository = last
        .strip_suffix(".git")
        .or_else(|| last.strip_suffix(".GIT"))
        .unwrap_or(last);
    if repository.is_empty() {
        return None;
    }
    Some(GitRepositoryIdentity {
        host: host.to_ascii_lowercase(),
        owner: segments[0].to_string(),
        repository: repository.to_string(),
    })
}

pub fn is_official_marketplace_git_source(url: &str) -> bool {
    let Some(source) = parse_git_repository_identity(url) else {
        return false;
    };
    source.host == "github.com"
        && source
            .owner
            .eq_ignore_ascii_case(OFFICIAL_MARKETPLACE_OWNER)
        && source
            .repository
            .eq_ignore_ascii_case(OFFICIAL_MARKETPLACE_REPOSITORY)
}

pub fn is_official_organization_git_source(url: &str) -> bool {
    parse_git_repository_identity(url).is_some_and(|source| {
        source.host == "github.com" && source.owner.eq_ignore_ascii_case(OFFICIAL_PLUGIN_PUBLISHER)
    })
}

/// `stablyai.*` or `*.orca-*`.
pub fn is_reserved_plugin_identity(plugin_key: &str) -> bool {
    let Some((publisher, id)) = split_qualified_plugin_key(plugin_key) else {
        return false;
    };
    publisher == OFFICIAL_PLUGIN_PUBLISHER || id.starts_with(OFFICIAL_PLUGIN_ID_PREFIX)
}

pub fn is_official_plugin_identity(plugin_key: &str) -> bool {
    let Some((publisher, id)) = split_qualified_plugin_key(plugin_key) else {
        return false;
    };
    publisher == OFFICIAL_PLUGIN_PUBLISHER && id.starts_with(OFFICIAL_PLUGIN_ID_PREFIX)
}

fn split_qualified_plugin_key(plugin_key: &str) -> Option<(&str, &str)> {
    if !is_qualified_plugin_key(plugin_key) {
        return None;
    }
    plugin_key.split_once('.')
}

/// Refuses snapshots whose owner or reserved identities contradict their
/// source, mirroring `validateMarketplaceProvenance`.
pub fn validate_marketplace_provenance(
    source: &MarketplaceSource,
    index: &MarketplaceIndex,
) -> Result<(), PluginError> {
    if is_official_marketplace_git_source(&source.url)
        && !index.owner.eq_ignore_ascii_case(OFFICIAL_MARKETPLACE_OWNER)
    {
        return Err(PluginError::new(
            INVALID_MANIFEST,
            "official marketplace metadata has an unexpected owner",
        ));
    }
    for entry in &index.plugins {
        if is_reserved_plugin_identity(&entry.id)
            && !is_official_organization_git_source(&entry.source.url)
        {
            return Err(PluginError::new(
                INVALID_MANIFEST,
                format!(
                    "reserved plugin identity {} must resolve to the stablyai organization",
                    entry.id
                ),
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::marketplace::store::{OFFICIAL_MARKETPLACE_OWNER, OFFICIAL_MARKETPLACE_URL};
    use crate::marketplace::test_support::{commit_all, init_repo, write_marketplace_index};
    use serde_json::json;
    use tempfile::tempdir;

    const COMMIT: &str = "cccccccccccccccccccccccccccccccccccccccc";

    fn local_source(path: &Path) -> MarketplaceSource {
        MarketplaceSource::git(path.to_string_lossy().into_owned(), "main")
    }

    fn service(data_dir: &Path) -> MarketplaceService {
        MarketplaceService::with_runner(data_dir, GitRunner::local_test())
    }

    fn local_marketplace_repo(root: &Path) -> (std::path::PathBuf, String) {
        let repo = init_repo(root);
        write_marketplace_index(
            &repo,
            &json!({
                "name": "Local Marketplace",
                "owner": "local-owner",
                "plugins": [{
                    "id": "acme.demo",
                    "source": { "kind": "git", "url": "https://github.com/acme/demo.git", "ref": "main" },
                    "description": "A demo",
                    "categories": ["demo"]
                }]
            }),
        );
        let commit = commit_all(&repo, "marketplace");
        (repo, commit)
    }

    #[test]
    fn refresh_clones_parses_and_writes_a_snapshot() {
        let temp = tempdir().unwrap();
        let (repo, commit) = local_marketplace_repo(&temp.path().join("marketplace"));
        let service = service(&temp.path().join("plugins-data"));
        let source = local_source(&repo);

        let state = service.add_source(&source, 7).unwrap();
        assert_eq!(state.added_at, 7);
        assert!(!state.official);
        let projection = state.marketplace.as_ref().unwrap();
        assert_eq!(projection.name, "Local Marketplace");
        assert_eq!(projection.resolved_commit, commit);
        assert_eq!(projection.owner, "local-owner");

        let listings = service.list_plugins().unwrap();
        assert_eq!(listings.len(), 1);
        assert_eq!(listings[0].plugin_key, "acme.demo");
        assert_eq!(listings[0].marketplace_source_id, state.id);
        assert_eq!(listings[0].marketplace_commit, commit);
        assert_eq!(listings[0].description.as_deref(), Some("A demo"));
        assert!(!listings[0].official);

        let found = service
            .find_plugin(&state.id, "acme.demo")
            .unwrap()
            .unwrap();
        assert_eq!(found.plugin_key, "acme.demo");
        assert!(service
            .find_plugin(&state.id, "acme.missing")
            .unwrap()
            .is_none());

        let refreshed = service.refresh(Some(&state.id)).unwrap();
        assert_eq!(refreshed.len(), 1);
        assert_eq!(service.refresh(None).unwrap().len(), 1);
        let unknown = service.refresh(Some("00000000000000000000000000000000"));
        assert!(unknown
            .unwrap_err()
            .message()
            .contains("unknown marketplace source"));

        let sources = service.list_sources().unwrap();
        assert_eq!(sources.len(), 1);
        assert_eq!(sources[0].id, state.id);
    }

    #[test]
    fn refresh_rejects_broken_marketplace_content() {
        let temp = tempdir().unwrap();
        let repo = init_repo(&temp.path().join("marketplace"));
        std::fs::write(repo.join(MARKETPLACE_FILENAME), b"not json").unwrap();
        commit_all(&repo, "broken");
        let service = service(&temp.path().join("plugins-data"));

        let error = service.add_source(&local_source(&repo), 1).unwrap_err();
        assert!(error.message().contains("invalid orca-marketplace.json"));
        assert!(
            service.list_sources().unwrap().is_empty(),
            "failed seed must roll back"
        );
    }

    #[test]
    fn official_trust_requires_source_owner_identity_and_prefix() {
        let temp = tempdir().unwrap();
        let data = temp.path().join("plugins-data");
        let service = service(&data);
        let official = official_marketplace_source();
        assert_eq!(official.url, OFFICIAL_MARKETPLACE_URL);
        let registered = service.store().add_source(&official, 1).unwrap();

        let index = |owner: &str, id: &str, url: &str| {
            serde_json::from_value::<MarketplaceIndex>(json!({
                "name": "Orca Plugins",
                "owner": owner,
                "plugins": [{
                    "id": id,
                    "source": { "kind": "git", "url": url, "ref": "main" }
                }]
            }))
            .unwrap()
        };

        service
            .store()
            .write_snapshot(
                &registered,
                COMMIT,
                1,
                &index(
                    "stablyai",
                    "stablyai.orca-demo",
                    "https://github.com/stablyai/demo.git",
                ),
            )
            .unwrap();
        let listings = service.list_plugins().unwrap();
        assert!(
            listings[0].official,
            "official trust must project positively"
        );

        for (owner, id, url) in [
            (
                "evil",
                "stablyai.orca-demo",
                "https://github.com/stablyai/demo.git",
            ),
            (
                OFFICIAL_MARKETPLACE_OWNER,
                "stablyai.demo",
                "https://github.com/stablyai/demo.git",
            ),
            (
                OFFICIAL_MARKETPLACE_OWNER,
                "stablyai.orca-demo",
                "https://github.com/evil/demo.git",
            ),
            (
                OFFICIAL_MARKETPLACE_OWNER,
                "stablyai.orca-demo",
                "https://gitlab.com/stablyai/demo.git",
            ),
            (
                OFFICIAL_MARKETPLACE_OWNER,
                "acme.orca-demo",
                "https://github.com/acme/demo.git",
            ),
        ] {
            service
                .store()
                .write_snapshot(&registered, COMMIT, 1, &index(owner, id, url))
                .unwrap();
            let listings = service.list_plugins().unwrap();
            assert!(
                !listings[0].official,
                "expected non-official for owner={owner} id={id} url={url}"
            );
        }

        let official_source = official_marketplace_source();
        let wrong_owner = index("evil", "acme.demo", "https://github.com/acme/demo.git");
        let error = validate_marketplace_provenance(&official_source, &wrong_owner).unwrap_err();
        assert_eq!(
            error.message(),
            "official marketplace metadata has an unexpected owner"
        );

        let reserved = index("acme", "acme.orca-demo", "https://github.com/acme/demo.git");
        let non_official_source =
            MarketplaceSource::git("https://github.com/acme/marketplace.git", "main");
        let error = validate_marketplace_provenance(&non_official_source, &reserved).unwrap_err();
        assert_eq!(
            error.message(),
            "reserved plugin identity acme.orca-demo must resolve to the stablyai organization"
        );

        let official_seed = service.store().add_source(&official, 2).unwrap();
        let error = service.remove_source(&official_seed.id).unwrap_err();
        assert_eq!(
            error.message(),
            "the official marketplace is managed by Orca and cannot be removed"
        );
    }

    #[test]
    fn official_seed_requires_the_official_url_shape() {
        let temp = tempdir().unwrap();
        let repo = init_repo(&temp.path().join("marketplace"));
        write_marketplace_index(
            &repo,
            &json!({
                "name": "Local",
                "owner": "stablyai",
                "plugins": []
            }),
        );
        commit_all(&repo, "marketplace");
        let service = service(&temp.path().join("plugins-data"));
        let state = service.add_source(&local_source(&repo), 1).unwrap();
        assert!(!state.official, "a local clone can never be official");
    }

    #[test]
    fn repository_identity_matches_the_oracle_parser() {
        assert!(is_official_marketplace_git_source(
            "https://github.com/stablyai/orca-plugins.git"
        ));
        assert!(is_official_marketplace_git_source(
            "git@github.com:stablyai/orca-plugins.git"
        ));
        assert!(!is_official_marketplace_git_source(
            "https://gitlab.com/stablyai/orca-plugins.git"
        ));
        assert!(!is_official_marketplace_git_source(
            "https://github.com/stablyai-other/orca-plugins.git"
        ));
        assert!(!is_official_marketplace_git_source(
            "https://github.com/stablyai/orca-other.git"
        ));
        assert!(is_official_organization_git_source(
            "https://github.com/stablyai/anything.git"
        ));
        assert!(!is_official_organization_git_source(
            "https://github.com/stablyai-evil/x.git"
        ));

        assert!(is_reserved_plugin_identity("stablyai.anything"));
        assert!(is_reserved_plugin_identity("acme.orca-thing"));
        assert!(!is_reserved_plugin_identity("acme.demo"));
        assert!(is_official_plugin_identity("stablyai.orca-demo"));
        assert!(!is_official_plugin_identity("stablyai.demo"));
        assert!(!is_official_plugin_identity("acme.orca-demo"));

        let identity = parse_git_repository_identity("ssh://git@github.com/acme/demo.git").unwrap();
        assert_eq!(identity.host, "github.com");
        assert_eq!(identity.owner, "acme");
        assert_eq!(identity.repository, "demo");
    }
}
