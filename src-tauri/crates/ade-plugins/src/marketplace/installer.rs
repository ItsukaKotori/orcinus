//! Marketplace install pipeline: preview a pinned commit, install it through
//! the shared staging/publication path, and roll back like any other install.
//!
//! Ported from `orca:src/main/plugins/plugin-marketplace-installer.ts`:
//!
//! - `preview` checks out the listed ref into a temporary workspace, resolves
//!   the exact commit, and returns the content hash, consent fingerprint, and
//!   manifest projection the UI shows before approval.
//! - `install` re-resolves the listing and refuses when either the marketplace
//!   commit or the plugin commit changed after preview; only then does it
//!   stage the bytes and publish a lock entry whose source records the
//!   marketplace resolved commit.
//! - `rollback` reuses [`crate::install::rollback`] (previous immutable
//!   version, previous consent fingerprint).

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::artifact::validate_declared_artifacts;
use crate::consent::fingerprint;
use crate::content_hash::hash_plugin_tree;
use crate::install::lockfile::{LockEntry, LockSource, MarketplacePluginRef, MarketplaceSourceRef};
use crate::install::publication::publish_install;
use crate::install::staging::{read_install_manifest, stage_plugin_tree};
use crate::install::{
    ensure_project_gitignore, rollback as install_rollback, InstallContext, InstallResult,
};
use crate::manifest::Manifest;
use crate::marketplace::service::MarketplaceService;
use crate::marketplace::store::MarketplaceSource;
use crate::marketplace::{now_millis, TempWorkspace};
use crate::paths::ScopeRoots;
use crate::types::{PluginError, PluginScope, CONTENT_CHANGED, INVALID_MANIFEST, INVALID_PARAMS};

/// Consent-visible projection of a pinned marketplace candidate.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MarketplacePreview {
    pub marketplace_source_id: String,
    pub marketplace_name: String,
    pub marketplace_owner: String,
    /// The registered marketplace source (`{url, ref}`).
    pub marketplace: MarketplaceSource,
    pub marketplace_commit: String,
    pub plugin_key: String,
    /// The listed plugin source (`{url, ref}`).
    pub source: MarketplaceSource,
    pub resolved_commit: String,
    pub content_hash: String,
    pub consent_fingerprint: String,
    pub manifest: Manifest,
    pub official: bool,
}

/// Preview/install/rollback for listings served by a [`MarketplaceService`].
#[derive(Debug, Clone)]
pub struct MarketplaceInstaller {
    service: MarketplaceService,
    host_version: String,
}

impl MarketplaceInstaller {
    pub fn new(service: MarketplaceService, host_version: impl Into<String>) -> Self {
        Self {
            service,
            host_version: host_version.into(),
        }
    }

    pub fn service(&self) -> &MarketplaceService {
        &self.service
    }

    /// Checks out the listed ref at its current commit and returns the pinned
    /// identity (commit, content hash, consent fingerprint, manifest).
    pub fn preview(
        &self,
        source_id: &str,
        plugin_key: &str,
    ) -> Result<MarketplacePreview, PluginError> {
        let listing = self.require_listing(source_id, plugin_key)?;
        let workspace = TempWorkspace::create("ade-plugin-marketplace-preview-")?;
        let destination = workspace.path().join("plugin");
        let resolved_commit = self.service.runner().clone_depth1(
            &listing.source.url,
            Some(&listing.source.ref_),
            &destination,
        )?;
        let (manifest, content_hash, consent_fingerprint) =
            inspect_plugin_tree(&destination, plugin_key, &self.host_version)?;
        Ok(MarketplacePreview {
            marketplace_source_id: listing.marketplace_source_id.clone(),
            marketplace_name: listing.marketplace_name.clone(),
            marketplace_owner: listing.marketplace_owner.clone(),
            marketplace: listing.marketplace.clone(),
            marketplace_commit: listing.marketplace_commit.clone(),
            plugin_key: listing.plugin_key.clone(),
            source: listing.source.clone(),
            resolved_commit,
            content_hash,
            consent_fingerprint,
            manifest,
            official: listing.official,
        })
    }

    /// Re-validates the pinned identity and installs through the shared
    /// staging/publication pipeline. Refuses when the marketplace commit or
    /// the plugin commit no longer matches the preview.
    pub fn install(
        &self,
        preview: &MarketplacePreview,
        roots: &ScopeRoots,
        context: &InstallContext,
    ) -> Result<InstallResult, PluginError> {
        let listing = self.require_listing(&preview.marketplace_source_id, &preview.plugin_key)?;
        if listing.marketplace_commit != preview.marketplace_commit {
            return Err(PluginError::new(
                CONTENT_CHANGED,
                "marketplace changed after preview; review the plugin again",
            ));
        }
        let workspace = TempWorkspace::create("ade-plugin-marketplace-install-")?;
        let destination = workspace.path().join("plugin");
        let resolved_commit = self.service.runner().clone_depth1(
            &listing.source.url,
            Some(&listing.source.ref_),
            &destination,
        )?;
        if resolved_commit != preview.resolved_commit {
            return Err(PluginError::new(
                CONTENT_CHANGED,
                "plugin source changed after preview; review the update again",
            ));
        }
        let (manifest, content_hash, _) =
            inspect_plugin_tree(&destination, &preview.plugin_key, &context.host_version)?;
        if content_hash != preview.content_hash {
            return Err(PluginError::new(
                CONTENT_CHANGED,
                "plugin content changed after preview; review the plugin again",
            ));
        }
        if context.scope == PluginScope::Project {
            ensure_project_gitignore(roots)?;
        }
        let plugin_dir = roots.install_dir.join(&preview.plugin_key);
        let (installed_hash, version_dir) =
            stage_plugin_tree(&destination, &plugin_dir, &manifest)?;
        let staged_manifest = read_install_manifest(&version_dir, &context.host_version)?;
        if staged_manifest.qualified_key() != preview.plugin_key {
            return Err(PluginError::new(
                INVALID_MANIFEST,
                "staged plugin manifest identity does not match its directory",
            ));
        }
        let entry = LockEntry {
            plugin_key: preview.plugin_key.clone(),
            version: staged_manifest.version.clone(),
            source: LockSource::Marketplace {
                marketplace: MarketplaceSourceRef {
                    url: preview.marketplace.url.clone(),
                    ref_: preview.marketplace.ref_.clone(),
                    resolved_commit: preview.marketplace_commit.clone(),
                },
                plugin: MarketplacePluginRef {
                    url: preview.source.url.clone(),
                    ref_: preview.source.ref_.clone(),
                },
            },
            resolved_commit: Some(preview.resolved_commit.clone()),
            content_hash: installed_hash.clone(),
            consent_fingerprint: context.consent_fingerprint.clone(),
            installed_at: now_millis(),
        };
        publish_install(
            roots,
            &preview.plugin_key,
            &installed_hash,
            &entry,
            &roots.lockfile_path,
        )?;
        Ok(InstallResult {
            plugin_key: preview.plugin_key.clone(),
            version: staged_manifest.version,
            content_hash: installed_hash.clone(),
            consent_fingerprint: context.consent_fingerprint.clone(),
            install_hash: installed_hash,
            scope: context.scope,
            resolved_commit: Some(preview.resolved_commit.clone()),
        })
    }

    /// Restores the previous immutable version; the previous consent
    /// fingerprint becomes current again.
    pub fn rollback(
        &self,
        plugin_key: &str,
        roots: &ScopeRoots,
        scope: PluginScope,
    ) -> Result<InstallResult, PluginError> {
        install_rollback(plugin_key, roots, &self.host_version, scope)
    }

    fn require_listing(
        &self,
        source_id: &str,
        plugin_key: &str,
    ) -> Result<crate::marketplace::service::MarketplaceListing, PluginError> {
        self.service
            .find_plugin(source_id, plugin_key)?
            .ok_or_else(|| {
                PluginError::new(
                    INVALID_PARAMS,
                    format!("plugin {plugin_key} is not listed by marketplace {source_id}"),
                )
            })
    }
}

fn inspect_plugin_tree(
    destination: &Path,
    expected_plugin_key: &str,
    host_version: &str,
) -> Result<(Manifest, String, String), PluginError> {
    let manifest = read_install_manifest(destination, host_version)?;
    if manifest.qualified_key() != expected_plugin_key {
        return Err(PluginError::new(
            INVALID_MANIFEST,
            format!(
                "plugin manifest identity {} does not match the marketplace listing {expected_plugin_key}",
                manifest.qualified_key()
            ),
        ));
    }
    validate_declared_artifacts(destination, &manifest)?;
    let content_hash = hash_plugin_tree(destination)?;
    let consent_fingerprint = fingerprint(&manifest, &content_hash);
    Ok((manifest, content_hash, consent_fingerprint))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::install::lockfile::read_lockfile;
    use crate::marketplace::test_support::{
        commit_all, init_repo, write_marketplace_index, write_plugin_tree,
    };
    use crate::paths::global_roots;
    use serde_json::json;
    use tempfile::tempdir;

    fn write_index(repo: &Path, plugin_url: &str, description: &str) {
        write_marketplace_index(
            repo,
            &json!({
                "name": "Test Marketplace",
                "owner": "acme",
                "plugins": [{
                    "id": "acme.demo",
                    "source": { "kind": "git", "url": plugin_url, "ref": "main" },
                    "description": description,
                    "categories": ["demo"]
                }]
            }),
        );
    }

    fn install_context(preview: &MarketplacePreview, scope: PluginScope) -> InstallContext {
        InstallContext {
            scope,
            repo_id: None,
            host_version: "1.4.0".to_string(),
            consent_fingerprint: preview.consent_fingerprint.clone(),
        }
    }

    #[test]
    fn preview_install_and_rollback_use_the_marketplace_source() {
        let temp = tempdir().unwrap();
        let plugin_dir = temp.path().join("plugin");
        let plugin_repo = init_repo(&plugin_dir);
        write_plugin_tree(&plugin_dir, "acme", "demo", "<h1>one</h1>");
        let first_commit = commit_all(&plugin_repo, "v1");
        let plugin_url = plugin_dir.to_string_lossy().into_owned();

        let marketplace_dir = temp.path().join("marketplace");
        let marketplace_repo = init_repo(&marketplace_dir);
        write_index(&marketplace_dir, &plugin_url, "v1");
        let marketplace_commit = commit_all(&marketplace_repo, "m1");

        let service = MarketplaceService::with_runner(
            &temp.path().join("plugins-data"),
            crate::git::GitRunner::local_test(),
        );
        let state = service
            .add_source(
                &MarketplaceSource::git(marketplace_dir.to_string_lossy().into_owned(), "main"),
                1,
            )
            .unwrap();
        let installer = MarketplaceInstaller::new(service, "1.4.0");

        let preview = installer.preview(&state.id, "acme.demo").unwrap();
        assert_eq!(preview.resolved_commit, first_commit);
        assert_eq!(preview.marketplace_commit, marketplace_commit);
        assert_eq!(preview.manifest.qualified_key(), "acme.demo");
        assert_eq!(preview.content_hash.len(), 64);
        assert!(preview.consent_fingerprint.starts_with("sha256-"));
        assert!(!preview.official);
        assert_eq!(preview.marketplace.url, marketplace_dir.to_string_lossy());

        let roots = global_roots(&temp.path().join("app-data"));
        let result = installer
            .install(
                &preview,
                &roots,
                &install_context(&preview, PluginScope::Global),
            )
            .unwrap();
        assert_eq!(result.content_hash, preview.content_hash);
        assert_eq!(result.consent_fingerprint, preview.consent_fingerprint);
        assert_eq!(result.scope, PluginScope::Global);

        let raw: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&roots.lockfile_path).unwrap()).unwrap();
        let entry = &raw["plugins"]["acme.demo"];
        assert_eq!(entry["source"]["kind"], "marketplace");
        assert_eq!(
            entry["source"]["marketplace"]["url"],
            marketplace_dir.to_string_lossy().as_ref()
        );
        assert_eq!(entry["source"]["plugin"]["url"], plugin_url);
        assert_eq!(entry["source"]["plugin"]["ref"], "main");
        assert_eq!(
            entry["source"]["marketplace"]["resolvedCommit"],
            marketplace_commit
        );
        assert_eq!(entry["resolvedCommit"], first_commit);
        assert_eq!(entry["capabilityHash"], preview.consent_fingerprint);

        let managed = read_lockfile(&roots.lockfile_path);
        let lock_entry = managed.entry("acme.demo").expect("lock entry");
        assert_eq!(
            lock_entry.source,
            LockSource::Marketplace {
                marketplace: MarketplaceSourceRef {
                    url: marketplace_dir.to_string_lossy().into_owned(),
                    ref_: "main".to_string(),
                    resolved_commit: marketplace_commit.clone(),
                },
                plugin: MarketplacePluginRef {
                    url: plugin_url.clone(),
                    ref_: "main".to_string(),
                },
            }
        );
        assert_eq!(
            lock_entry.resolved_commit.as_deref(),
            Some(first_commit.as_str())
        );

        std::fs::write(plugin_dir.join("panel.html"), b"<h1>two</h1>").unwrap();
        let second_commit = commit_all(&plugin_repo, "v2");
        let preview2 = installer.preview(&state.id, "acme.demo").unwrap();
        assert_eq!(preview2.resolved_commit, second_commit);
        let result2 = installer
            .install(
                &preview2,
                &roots,
                &install_context(&preview2, PluginScope::Global),
            )
            .unwrap();
        assert_ne!(result2.content_hash, result.content_hash);

        let rolled = installer
            .rollback("acme.demo", &roots, PluginScope::Global)
            .unwrap();
        assert_eq!(rolled.content_hash, preview.content_hash);
        assert_eq!(rolled.consent_fingerprint, preview.consent_fingerprint);
        let lock = read_lockfile(&roots.lockfile_path);
        assert_eq!(
            lock.entry("acme.demo").unwrap().content_hash,
            preview.content_hash
        );
    }

    #[test]
    fn install_refuses_after_plugin_or_marketplace_commit_drift() {
        let temp = tempdir().unwrap();
        let plugin_dir = temp.path().join("plugin");
        let plugin_repo = init_repo(&plugin_dir);
        write_plugin_tree(&plugin_dir, "acme", "demo", "<h1>one</h1>");
        commit_all(&plugin_repo, "v1");
        let plugin_url = plugin_dir.to_string_lossy().into_owned();

        let marketplace_dir = temp.path().join("marketplace");
        let marketplace_repo = init_repo(&marketplace_dir);
        write_index(&marketplace_dir, &plugin_url, "v1");
        commit_all(&marketplace_repo, "m1");

        let service = MarketplaceService::with_runner(
            &temp.path().join("plugins-data"),
            crate::git::GitRunner::local_test(),
        );
        let state = service
            .add_source(
                &MarketplaceSource::git(marketplace_dir.to_string_lossy().into_owned(), "main"),
                1,
            )
            .unwrap();
        let installer = MarketplaceInstaller::new(service, "1.4.0");
        let roots = global_roots(&temp.path().join("app-data"));

        let preview = installer.preview(&state.id, "acme.demo").unwrap();

        std::fs::write(plugin_dir.join("panel.html"), b"<h1>two</h1>").unwrap();
        commit_all(&plugin_repo, "v2");
        let error = installer
            .install(
                &preview,
                &roots,
                &install_context(&preview, PluginScope::Global),
            )
            .unwrap_err();
        assert_eq!(
            error.message(),
            "plugin source changed after preview; review the update again"
        );
        assert!(
            !roots.install_dir.exists(),
            "refused install must not stage"
        );

        let preview2 = installer.preview(&state.id, "acme.demo").unwrap();
        write_index(&marketplace_dir, &plugin_url, "v2");
        commit_all(&marketplace_repo, "m2");
        installer.service().refresh(Some(&state.id)).unwrap();

        let error = installer
            .install(
                &preview,
                &roots,
                &install_context(&preview, PluginScope::Global),
            )
            .unwrap_err();
        assert_eq!(
            error.message(),
            "marketplace changed after preview; review the plugin again"
        );
        assert!(!roots.install_dir.exists());
        let error = installer
            .install(
                &preview2,
                &roots,
                &install_context(&preview2, PluginScope::Global),
            )
            .unwrap_err();
        assert_eq!(
            error.message(),
            "marketplace changed after preview; review the plugin again"
        );

        let preview3 = installer.preview(&state.id, "acme.demo").unwrap();
        let installed = installer
            .install(
                &preview3,
                &roots,
                &install_context(&preview3, PluginScope::Global),
            )
            .unwrap();
        assert_eq!(installed.content_hash, preview3.content_hash);
    }

    #[test]
    fn preview_rejects_manifest_identity_mismatches() {
        let temp = tempdir().unwrap();
        let plugin_dir = temp.path().join("plugin");
        let plugin_repo = init_repo(&plugin_dir);
        write_plugin_tree(&plugin_dir, "acme", "demo", "<h1>one</h1>");
        commit_all(&plugin_repo, "v1");
        let plugin_url = plugin_dir.to_string_lossy().into_owned();

        let marketplace_dir = temp.path().join("marketplace");
        let marketplace_repo = init_repo(&marketplace_dir);
        write_marketplace_index(
            &marketplace_dir,
            &json!({
                "name": "Test Marketplace",
                "owner": "acme",
                "plugins": [{
                    "id": "other.demo",
                    "source": { "kind": "git", "url": plugin_url, "ref": "main" }
                }]
            }),
        );
        commit_all(&marketplace_repo, "m1");

        let service = MarketplaceService::with_runner(
            &temp.path().join("plugins-data"),
            crate::git::GitRunner::local_test(),
        );
        let state = service
            .add_source(
                &MarketplaceSource::git(marketplace_dir.to_string_lossy().into_owned(), "main"),
                1,
            )
            .unwrap();
        let installer = MarketplaceInstaller::new(service, "1.4.0");

        let error = installer.preview(&state.id, "other.demo").unwrap_err();
        assert_eq!(
            error.message(),
            "plugin manifest identity acme.demo does not match the marketplace listing other.demo"
        );
        let error = installer.preview(&state.id, "missing.plugin").unwrap_err();
        assert!(error.message().contains("is not listed by marketplace"));
    }
}
