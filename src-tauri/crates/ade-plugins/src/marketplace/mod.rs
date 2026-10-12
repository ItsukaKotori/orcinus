//! Git-based plugin marketplace: sources, cached snapshots, the refresh
//! service, and the preview/install/rollback pipeline (spec §3.7).
//!
//! Follows `orca:src/main/plugins/plugin-marketplace-*.ts`: sources and one
//! parsed snapshot per source live under `<data>/marketplaces/`, all network
//! access goes through [`GitRunner`] (git only, no HTTP client), and official
//! trust is host-projected (marketplace JSON can never self-award it).

pub mod installer;
pub mod service;
pub mod store;

pub use installer::{MarketplaceInstaller, MarketplacePreview};
pub use service::{
    is_official_marketplace_git_source, is_official_organization_git_source,
    is_official_plugin_identity, is_reserved_plugin_identity, official_marketplace_source,
    parse_git_repository_identity, read_marketplace_index, validate_marketplace_provenance,
    MarketplaceListing, MarketplaceProjection, MarketplaceService, SourceState,
};
pub use store::{
    marketplace_source_id, MarketplaceEntry, MarketplaceIndex, MarketplaceSnapshot,
    MarketplaceSource, MarketplaceStore, RegisteredSource, MARKETPLACE_FILENAME,
    MARKETPLACE_SOURCE_LIMIT, OFFICIAL_MARKETPLACE_OWNER, OFFICIAL_MARKETPLACE_REF,
    OFFICIAL_MARKETPLACE_URL, OFFICIAL_PLUGIN_ID_PREFIX, OFFICIAL_PLUGIN_PUBLISHER,
};

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use uuid::Uuid;

use crate::types::{PluginError, IO_ERROR};

/// Temporary workspace for one Git checkout. Removed on drop; used instead of
/// the dev-only `tempfile` crate because service code is production code.
pub(crate) struct TempWorkspace {
    path: PathBuf,
}

impl TempWorkspace {
    pub(crate) fn create(prefix: &str) -> Result<Self, PluginError> {
        let path = std::env::temp_dir().join(format!("{prefix}{}", Uuid::new_v4().simple()));
        fs::create_dir_all(&path).map_err(|error| {
            PluginError::new(
                IO_ERROR,
                format!("failed to create a temporary directory: {error}"),
            )
        })?;
        Ok(Self { path })
    }

    pub(crate) fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempWorkspace {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

pub(crate) fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
pub(crate) mod test_support {
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::process::Command;

    pub(crate) fn git(cwd: &Path, args: &[&str]) {
        let status = Command::new("git")
            .args(args)
            .current_dir(cwd)
            .status()
            .expect("git runs");
        assert!(status.success(), "git {args:?} failed");
    }

    pub(crate) fn init_repo(dir: &Path) -> PathBuf {
        fs::create_dir_all(dir).unwrap();
        git(dir, &["init", "--quiet", "-b", "main"]);
        git(dir, &["config", "user.email", "tests@example.com"]);
        git(dir, &["config", "user.name", "Tests"]);
        git(dir, &["config", "commit.gpgsign", "false"]);
        git(dir, &["config", "tag.gpgsign", "false"]);
        git(dir, &["config", "uploadpack.allowAnySHA1InWant", "true"]);
        dir.to_path_buf()
    }

    pub(crate) fn commit_all(dir: &Path, message: &str) -> String {
        git(dir, &["add", "-A"]);
        git(dir, &["commit", "--quiet", "-m", message]);
        let output = Command::new("git")
            .args(["rev-parse", "HEAD"])
            .current_dir(dir)
            .output()
            .unwrap();
        String::from_utf8_lossy(&output.stdout).trim().to_string()
    }

    pub(crate) fn write_marketplace_index(dir: &Path, index: &serde_json::Value) {
        fs::write(
            dir.join(super::store::MARKETPLACE_FILENAME),
            serde_json::to_vec_pretty(index).unwrap(),
        )
        .unwrap();
    }

    pub(crate) fn plugin_manifest(publisher: &str, id: &str, version: &str) -> serde_json::Value {
        serde_json::json!({
            "manifestVersion": 1,
            "id": id,
            "publisher": publisher,
            "name": id,
            "version": version,
            "engines": { "orca": ">=1.0.0" },
            "pluginApi": 1,
            "contributes": {
                "panels": [{ "id": "panel", "title": "Panel", "entry": "panel.html" }]
            }
        })
    }

    pub(crate) fn write_plugin_tree(dir: &Path, publisher: &str, id: &str, panel: &str) {
        fs::create_dir_all(dir).unwrap();
        fs::write(
            dir.join("ade-plugin.json"),
            serde_json::to_vec_pretty(&plugin_manifest(publisher, id, "1.0.0")).unwrap(),
        )
        .unwrap();
        fs::write(dir.join("panel.html"), panel).unwrap();
    }
}
