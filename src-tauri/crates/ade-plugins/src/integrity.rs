//! Lazy content-integrity verification for installed, hash-addressed plugin
//! payloads.
//!
//! Ported from `orca:src/main/plugins/plugin-content-integrity.ts`: the
//! immutable `<install>/<key>/<hash>` directory name is the integrity anchor,
//! so before a worker starts or a panel/tab entry is read the host re-hashes
//! the tree and refuses any tree whose bytes no longer match the pointer that
//! selected it. Dev trees are intentionally mutable and are not verified here.
//!
//! The early-P0 128-bit directory-name prefix is honored for installs that
//! predate full digests; all new installs use the full sha256 hex digest.

use std::collections::HashSet;
use std::path::Path;

use crate::discovery::{is_plugin_content_hash, is_qualified_plugin_key};
use crate::install::pointer::read_pointer;
use crate::install::staging::reparse_manifest;
use crate::manifest::Manifest;
use crate::types::{PluginError, INTEGRITY_MISMATCH, IO_ERROR, MANIFEST_MISMATCH};

/// Recomputes the tree hash beneath `install_root` and requires it to match
/// the `current`-pointer install hash and the expected plugin identity.
///
/// `install_root` is the scope's `plugins/` directory; the active version is
/// resolved through `<install_root>/<manifest qualified key>/current` exactly
/// like discovery does, so an install whose pointer or directory was swapped
/// cannot be verified by accident.
pub fn verify_installed(install_root: &Path, manifest: &Manifest) -> Result<(), PluginError> {
    let plugin_key = manifest.qualified_key();
    if !is_qualified_plugin_key(&plugin_key) {
        return Err(PluginError::new(
            MANIFEST_MISMATCH,
            "manifest identity is not a qualified plugin key",
        ));
    }
    let plugin_dir = install_root.join(&plugin_key);
    if !plugin_dir.is_dir() {
        return Err(PluginError::new(
            IO_ERROR,
            format!("plugin {plugin_key} is not installed"),
        ));
    }
    let install_hash = read_pointer(&plugin_dir)?.filter(|hash| is_plugin_content_hash(hash));
    let Some(install_hash) = install_hash else {
        return Err(PluginError::new(
            INTEGRITY_MISMATCH,
            format!("plugin {plugin_key} has no current version"),
        ));
    };
    let version_dir = plugin_dir.join(&install_hash);

    let installed_manifest = reparse_manifest(&version_dir)?;
    if installed_manifest.qualified_key() != plugin_key {
        return Err(PluginError::new(
            MANIFEST_MISMATCH,
            format!(
                "installed manifest identity \"{}\" does not match expected \"{plugin_key}\"",
                installed_manifest.qualified_key()
            ),
        ));
    }

    let actual_hash = crate::content_hash::hash_plugin_tree(&version_dir)?;
    let matches_current = actual_hash == install_hash;
    // Early P0 installs used a 128-bit SHA-256 prefix as the directory name.
    let matches_legacy_prefix = install_hash.len() == 32 && actual_hash.starts_with(&install_hash);
    if !matches_current && !matches_legacy_prefix {
        return Err(PluginError::new(
            INTEGRITY_MISMATCH,
            format!("content hash mismatch (expected {install_hash}, got {actual_hash})"),
        ));
    }
    Ok(())
}

/// Content hash of one immutable installed tree. Injected so the verifier's
/// caching behavior is testable without the real filesystem.
pub trait ContentHasher {
    fn hash_tree(&self, root: &Path) -> Result<String, PluginError>;
}

/// Real sha256 tree hasher (the same routine that names the install
/// directory).
#[derive(Debug, Clone, Copy, Default)]
pub struct FsContentHasher;

impl ContentHasher for FsContentHasher {
    fn hash_tree(&self, root: &Path) -> Result<String, PluginError> {
        crate::content_hash::hash_plugin_tree(root)
    }
}

/// Lazy verifier that deduplicates the first verification of each immutable
/// install identity for the lifetime of one host process/refresh.
///
/// Mirrors the oracle's `PluginContentVerifier`: nothing is hashed until the
/// first `verify` (worker start / panel read), and a refresh that replaces a
/// same-key install while the old tree is still being verified cannot leak a
/// stale success because the cache key is the immutable
/// `(plugin_key, version_dir, content_hash)` triple, never the key alone.
pub struct PluginContentVerifier<H: ContentHasher> {
    hasher: H,
    verified: HashSet<(String, String, String)>,
}

impl<H: ContentHasher> PluginContentVerifier<H> {
    pub fn new(hasher: H) -> Self {
        Self {
            hasher,
            verified: HashSet::new(),
        }
    }

    /// Verifies the tree beneath `version_dir` against `content_hash`. Each
    /// identity is verified once; failures are not cached so a tampered tree
    /// keeps failing on every call.
    pub fn verify(
        &mut self,
        plugin_key: &str,
        version_dir: &Path,
        content_hash: &str,
    ) -> Result<(), PluginError> {
        let identity = (
            plugin_key.to_string(),
            version_dir.to_string_lossy().into_owned(),
            content_hash.to_string(),
        );
        if self.verified.contains(&identity) {
            return Ok(());
        }
        // A failed verification is never recorded, so a tampered tree keeps
        // failing on every call instead of being masked by a cache entry.
        let actual = self.hasher.hash_tree(version_dir)?;
        let matches_current = actual == content_hash;
        let matches_legacy_prefix = content_hash.len() == 32 && actual.starts_with(content_hash);
        if !matches_current && !matches_legacy_prefix {
            return Err(PluginError::new(
                INTEGRITY_MISMATCH,
                format!(
                    "plugin {plugin_key} failed integrity verification: content hash mismatch (expected {content_hash}, got {actual})"
                ),
            ));
        }
        self.verified.insert(identity);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::install::{install_from_local, InstallContext};
    use crate::manifest::parse_manifest;
    use crate::paths::global_roots;
    use crate::types::{PluginScope, CONTENT_CHANGED, INVALID_MANIFEST, IO_ERROR};
    use serde_json::json;
    use std::cell::RefCell;
    use std::fs;
    use std::path::PathBuf;
    use tempfile::tempdir;

    fn manifest_json(capabilities: serde_json::Value, main: Option<&str>) -> Vec<u8> {
        let mut value = json!({
            "manifestVersion": 1,
            "id": "demo",
            "publisher": "orca-samples",
            "name": "Demo",
            "version": "1.0.0",
            "engines": { "orca": ">=1.0.0" },
            "pluginApi": 1,
            "capabilities": capabilities
        });
        if let Some(main) = main {
            value["main"] = json!(main);
        }
        serde_json::to_vec(&value).unwrap()
    }

    fn parse(capabilities: serde_json::Value, main: Option<&str>) -> Manifest {
        parse_manifest(&manifest_json(capabilities, main)).unwrap()
    }

    fn write_install(root: &Path, version: &str, files: &[(&str, &str)]) {
        fs::create_dir_all(root).unwrap();
        fs::write(
            root.join("ade-plugin.json"),
            serde_json::to_vec(&json!({
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
            }))
            .unwrap(),
        )
        .unwrap();
        for (relative, content) in files {
            let path = root.join(relative);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).unwrap();
            }
            fs::write(path, content).unwrap();
        }
    }

    #[test]
    fn verifies_a_fresh_install_and_accepts_the_current_pointer() {
        let temp = tempdir().unwrap();
        let source = temp.path().join("source");
        let app_data = temp.path().join("app-data");
        write_install(&source, "1.0.0", &[("panel.html", "<h1>one</h1>")]);
        let roots = global_roots(&app_data);
        install_from_local(
            &source,
            &roots,
            &InstallContext {
                scope: PluginScope::Global,
                repo_id: None,
                host_version: "1.4.0".to_string(),
                consent_fingerprint: "sha256-one".to_string(),
            },
        )
        .unwrap();

        verify_installed(&roots.install_dir, &parse(json!([]), None)).unwrap();
    }

    #[test]
    fn detects_tampering_in_the_installed_payload() {
        let temp = tempdir().unwrap();
        let source = temp.path().join("source");
        let app_data = temp.path().join("app-data");
        write_install(&source, "1.0.0", &[("panel.html", "<h1>one</h1>")]);
        let roots = global_roots(&app_data);
        let result = install_from_local(
            &source,
            &roots,
            &InstallContext {
                scope: PluginScope::Global,
                repo_id: None,
                host_version: "1.4.0".to_string(),
                consent_fingerprint: "sha256-one".to_string(),
            },
        )
        .unwrap();
        fs::write(
            roots
                .install_dir
                .join("orca-samples.demo")
                .join(&result.content_hash)
                .join("panel.html"),
            "<h1>tampered</h1>",
        )
        .unwrap();

        let error = verify_installed(&roots.install_dir, &parse(json!([]), None)).unwrap_err();
        assert_eq!(error.code(), INTEGRITY_MISMATCH);
        assert!(error.message().contains("content hash mismatch"));
        assert!(error.message().contains(&result.content_hash));
    }

    #[test]
    fn rejects_a_missing_install_or_pointer() {
        let temp = tempdir().unwrap();
        let app_data = temp.path().join("app-data");
        let roots = global_roots(&app_data);

        let missing = verify_installed(&roots.install_dir, &parse(json!([]), None)).unwrap_err();
        assert_eq!(missing.code(), IO_ERROR);

        fs::create_dir_all(roots.install_dir.join("orca-samples.demo")).unwrap();
        let no_pointer = verify_installed(&roots.install_dir, &parse(json!([]), None)).unwrap_err();
        assert_eq!(no_pointer.code(), INTEGRITY_MISMATCH);
        assert!(no_pointer.message().contains("no current version"));
    }

    #[test]
    fn rejects_a_corrupt_pointer_as_unverified() {
        let temp = tempdir().unwrap();
        let roots = global_roots(&temp.path().join("app-data"));
        let plugin_dir = roots.install_dir.join("orca-samples.demo");
        fs::create_dir_all(&plugin_dir).unwrap();
        fs::write(plugin_dir.join("current"), "not-a-hash").unwrap();

        let error = verify_installed(&roots.install_dir, &parse(json!([]), None)).unwrap_err();
        assert_eq!(error.code(), INTEGRITY_MISMATCH);
    }

    #[test]
    fn rejects_an_installed_manifest_identity_mismatch() {
        let temp = tempdir().unwrap();
        let roots = global_roots(&temp.path().join("app-data"));
        let version_dir = roots
            .install_dir
            .join("orca-samples.demo")
            .join("ab".repeat(32));
        fs::create_dir_all(&version_dir).unwrap();
        fs::write(
            version_dir.join("ade-plugin.json"),
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
        fs::write(
            roots.install_dir.join("orca-samples.demo").join("current"),
            "ab".repeat(32),
        )
        .unwrap();

        let error = verify_installed(&roots.install_dir, &parse(json!([]), None)).unwrap_err();
        assert_eq!(error.code(), MANIFEST_MISMATCH);
    }

    #[test]
    fn accepts_a_legacy_128_bit_prefix_directory_name() {
        let temp = tempdir().unwrap();
        let roots = global_roots(&temp.path().join("app-data"));
        let version_dir = roots.install_dir.join("orca-samples.demo").join("pending");
        write_install(&version_dir, "1.0.0", &[("panel.html", "<h1>one</h1>")]);
        let actual = crate::content_hash::hash_plugin_tree(&version_dir).unwrap();
        fs::rename(
            &version_dir,
            roots
                .install_dir
                .join("orca-samples.demo")
                .join(&actual[..32]),
        )
        .unwrap();
        fs::write(
            roots.install_dir.join("orca-samples.demo").join("current"),
            &actual[..32],
        )
        .unwrap();

        verify_installed(&roots.install_dir, &parse(json!([]), None)).unwrap();
    }

    #[test]
    fn rejects_an_unqualified_manifest_identity() {
        let temp = tempdir().unwrap();
        let roots = global_roots(&temp.path().join("app-data"));
        let mut manifest = parse(json!([]), None);
        manifest.publisher = "Bad Publisher".to_string();

        let error = verify_installed(&roots.install_dir, &manifest).unwrap_err();
        assert_eq!(error.code(), MANIFEST_MISMATCH);
    }

    #[test]
    fn rejects_an_unreadable_manifest_file() {
        let temp = tempdir().unwrap();
        let roots = global_roots(&temp.path().join("app-data"));
        let version_dir = roots
            .install_dir
            .join("orca-samples.demo")
            .join("ab".repeat(32));
        fs::create_dir_all(&version_dir).unwrap();
        fs::write(
            roots.install_dir.join("orca-samples.demo").join("current"),
            "ab".repeat(32),
        )
        .unwrap();

        let error = verify_installed(&roots.install_dir, &parse(json!([]), None)).unwrap_err();
        assert_eq!(error.code(), INVALID_MANIFEST);
    }

    #[derive(Default)]
    struct CountingHasher {
        calls: RefCell<Vec<PathBuf>>,
    }

    impl ContentHasher for CountingHasher {
        fn hash_tree(&self, root: &Path) -> Result<String, PluginError> {
            self.calls.borrow_mut().push(root.to_path_buf());
            crate::content_hash::hash_plugin_tree(root)
        }
    }

    #[test]
    fn verifier_hashes_each_immutable_identity_once() {
        let temp = tempdir().unwrap();
        let version_dir = temp.path().join("version");
        write_install(&version_dir, "1.0.0", &[("panel.html", "<h1>one</h1>")]);
        let content_hash = crate::content_hash::hash_plugin_tree(&version_dir).unwrap();
        let mut verifier = PluginContentVerifier::new(CountingHasher::default());

        verifier
            .verify("orca-samples.demo", &version_dir, &content_hash)
            .unwrap();
        verifier
            .verify("orca-samples.demo", &version_dir, &content_hash)
            .unwrap();

        assert_eq!(verifier.hasher.calls.borrow().len(), 1);
    }

    #[test]
    fn verifier_caches_per_identity_and_detects_tampering_under_a_new_identity() {
        let temp = tempdir().unwrap();
        let version_dir = temp.path().join("version");
        write_install(&version_dir, "1.0.0", &[("panel.html", "<h1>one</h1>")]);
        let content_hash = crate::content_hash::hash_plugin_tree(&version_dir).unwrap();
        let mut verifier = PluginContentVerifier::new(FsContentHasher);
        verifier
            .verify("orca-samples.demo", &version_dir, &content_hash)
            .unwrap();

        fs::write(version_dir.join("panel.html"), "<h1>tampered</h1>").unwrap();
        // The immutable identity was already proven; the cache stands for it.
        verifier
            .verify("orca-samples.demo", &version_dir, &content_hash)
            .unwrap();
        // A pointer move to new content must re-hash and fail closed.
        let error = verifier
            .verify("orca-samples.demo", &version_dir, &"f".repeat(64))
            .unwrap_err();
        assert_eq!(error.code(), INTEGRITY_MISMATCH);
        assert!(error.message().contains("failed integrity verification"));
    }

    #[test]
    fn verifier_accepts_the_legacy_128_bit_prefix() {
        let temp = tempdir().unwrap();
        let version_dir = temp.path().join("version");
        write_install(&version_dir, "1.0.0", &[("panel.html", "<h1>one</h1>")]);
        let content_hash = crate::content_hash::hash_plugin_tree(&version_dir).unwrap();
        let mut verifier = PluginContentVerifier::new(FsContentHasher);

        verifier
            .verify("orca-samples.demo", &version_dir, &content_hash[..32])
            .unwrap();
    }

    #[test]
    fn verifier_does_not_leak_a_stale_cache_key_for_a_same_key_reinstall() {
        let temp = tempdir().unwrap();
        let first = temp.path().join("first");
        let second = temp.path().join("second");
        write_install(&first, "1.0.0", &[("panel.html", "<h1>one</h1>")]);
        write_install(&second, "1.0.0", &[("panel.html", "<h1>two</h1>")]);
        let first_hash = crate::content_hash::hash_plugin_tree(&first).unwrap();
        let second_hash = crate::content_hash::hash_plugin_tree(&second).unwrap();
        let mut verifier = PluginContentVerifier::new(CountingHasher::default());

        verifier
            .verify("orca-samples.demo", &first, &first_hash)
            .unwrap();
        verifier
            .verify("orca-samples.demo", &second, &second_hash)
            .unwrap();
        verifier
            .verify("orca-samples.demo", &first, &first_hash)
            .unwrap();

        assert_eq!(verifier.hasher.calls.borrow().len(), 2);
    }

    #[test]
    fn verifier_failure_is_not_cached() {
        let temp = tempdir().unwrap();
        let version_dir = temp.path().join("version");
        write_install(&version_dir, "1.0.0", &[("panel.html", "<h1>one</h1>")]);
        let content_hash = crate::content_hash::hash_plugin_tree(&version_dir).unwrap();
        let mut verifier = PluginContentVerifier::new(CountingHasher::default());
        assert_ne!(content_hash, "f".repeat(64));

        assert!(verifier
            .verify("orca-samples.demo", &version_dir, &"f".repeat(64))
            .is_err());
        verifier
            .verify("orca-samples.demo", &version_dir, &content_hash)
            .unwrap();

        assert_eq!(verifier.hasher.calls.borrow().len(), 2);
    }

    #[test]
    fn hashing_errors_surface_with_their_own_code() {
        let temp = tempdir().unwrap();
        let roots = global_roots(&temp.path().join("app-data"));
        let version_dir = roots
            .install_dir
            .join("orca-samples.demo")
            .join("ab".repeat(32));
        write_install(&version_dir, "1.0.0", &[("panel.html", "<h1>one</h1>")]);
        std::os::unix::fs::symlink(temp.path().join("outside"), version_dir.join("escape"))
            .unwrap();
        fs::write(
            roots.install_dir.join("orca-samples.demo").join("current"),
            "ab".repeat(32),
        )
        .unwrap();

        let error = verify_installed(&roots.install_dir, &parse(json!([]), None)).unwrap_err();
        assert_eq!(error.code(), crate::types::SYMLINK_NOT_ALLOWED);
        assert_ne!(error.code(), CONTENT_CHANGED);
    }
}
