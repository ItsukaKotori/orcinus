//! Dual-scope path layout and the realpath containment primitive every
//! filesystem-facing plugin surface is built on.
//!
//! Global plugin state lives directly under the Tauri app-data directory;
//! project plugin state lives under `<repo>/.ade` (spec §3.3). Both scopes
//! carry the same three roots:
//!
//! - `<base>/plugins/`      hash-addressed immutable installs
//! - `<base>/plugin-data/`  per-plugin storage/settings/audit
//! - `<base>/plugins.lock.json`  provenance for one-click reinstall
//!
//! `validate_containment` mirrors the oracle's `resolvePathFromRealRoot`
//! containment rule (`orca:src/main/plugins/plugin-artifact-validation.ts`):
//! both paths are resolved through symlinks and the candidate must land
//! strictly inside the base directory.

use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use crate::types::{PluginError, IO_ERROR, UNSAFE_PATH};

pub const PLUGINS_DIR_NAME: &str = "plugins";
pub const PLUGIN_DATA_DIR_NAME: &str = "plugin-data";
pub const PLUGIN_LOCKFILE_FILENAME: &str = "plugins.lock.json";
/// Project-scope state directory inside the repository.
pub const PROJECT_STATE_DIR_NAME: &str = ".ade";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScopeRoots {
    pub install_dir: PathBuf,
    pub data_dir: PathBuf,
    pub lockfile_path: PathBuf,
}

impl ScopeRoots {
    pub fn from_base(base: &Path) -> Self {
        Self {
            install_dir: base.join(PLUGINS_DIR_NAME),
            data_dir: base.join(PLUGIN_DATA_DIR_NAME),
            lockfile_path: base.join(PLUGIN_LOCKFILE_FILENAME),
        }
    }
}

/// Global scope roots: `<app_data_dir>/{plugins,plugin-data,plugins.lock.json}`.
pub fn global_roots(app_data_dir: &Path) -> ScopeRoots {
    ScopeRoots::from_base(app_data_dir)
}

/// Project scope roots: `<repo>/.ade/{plugins,plugin-data,plugins.lock.json}`.
pub fn project_roots(repo_path: &Path) -> ScopeRoots {
    ScopeRoots::from_base(&repo_path.join(PROJECT_STATE_DIR_NAME))
}

/// Resolves `candidate` and requires it to be a strict descendant of the
/// resolved `base`. Returns the resolved (symlink-free) candidate path.
///
/// Error messages deliberately never carry absolute paths so discovery error
/// projections cannot leak a dev directory.
pub fn validate_containment(base: &Path, candidate: &Path) -> Result<PathBuf, PluginError> {
    let base_real = fs::canonicalize(base).map_err(|error| {
        PluginError::new(
            IO_ERROR,
            format!("failed to resolve the plugin directory: {error}"),
        )
    })?;
    let candidate_real = fs::canonicalize(candidate).map_err(|error| {
        if error.kind() == ErrorKind::NotFound {
            PluginError::new(IO_ERROR, "not found")
        } else {
            PluginError::new(
                IO_ERROR,
                format!("failed to resolve the artifact path: {error}"),
            )
        }
    })?;
    match candidate_real.strip_prefix(&base_real) {
        Ok(relative) if !relative.as_os_str().is_empty() => Ok(candidate_real),
        _ => Err(PluginError::new(
            UNSAFE_PATH,
            "resolves outside the plugin directory",
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;
    use tempfile::tempdir;

    #[test]
    fn global_roots_layout_matches_spec() {
        let base = Path::new("/tmp/example-app-data");
        let roots = global_roots(base);
        assert_eq!(roots.install_dir, base.join("plugins"));
        assert_eq!(roots.data_dir, base.join("plugin-data"));
        assert_eq!(roots.lockfile_path, base.join("plugins.lock.json"));
    }

    #[test]
    fn project_roots_layout_matches_spec() {
        let repo = Path::new("/tmp/example-repo");
        let roots = project_roots(repo);
        assert_eq!(roots.install_dir, repo.join(".ade").join("plugins"));
        assert_eq!(roots.data_dir, repo.join(".ade").join("plugin-data"));
        assert_eq!(
            roots.lockfile_path,
            repo.join(".ade").join("plugins.lock.json")
        );
    }

    #[test]
    fn containment_accepts_descendants_and_resolves_symlinked_bases() {
        let root = tempdir().unwrap();
        let base = root.path().join("base");
        fs::create_dir_all(base.join("child")).unwrap();
        fs::write(base.join("child").join("file.txt"), b"hi").unwrap();

        let expected = fs::canonicalize(base.join("child").join("file.txt")).unwrap();
        let resolved = validate_containment(&base, &base.join("child").join("file.txt")).unwrap();
        assert_eq!(resolved, expected);
    }

    #[test]
    fn containment_rejects_paths_outside_the_base() {
        let root = tempdir().unwrap();
        let base = root.path().join("base");
        fs::create_dir_all(&base).unwrap();
        fs::write(root.path().join("outside.txt"), b"nope").unwrap();

        let error = validate_containment(&base, &root.path().join("outside.txt")).unwrap_err();
        assert_eq!(error.code(), UNSAFE_PATH);
        assert_eq!(error.message(), "resolves outside the plugin directory");
    }

    #[test]
    fn containment_is_component_wise_not_string_prefix() {
        let root = tempdir().unwrap();
        let base = root.path().join("base");
        let sibling = root.path().join("base-sibling");
        fs::create_dir_all(&base).unwrap();
        fs::create_dir_all(&sibling).unwrap();
        fs::write(sibling.join("file.txt"), b"nope").unwrap();

        let error = validate_containment(&base, &sibling.join("file.txt")).unwrap_err();
        assert_eq!(error.code(), UNSAFE_PATH);
    }

    #[test]
    fn containment_rejects_escape_through_symlinked_directory() {
        let root = tempdir().unwrap();
        let base = root.path().join("base");
        let outside = root.path().join("outside");
        fs::create_dir_all(&base).unwrap();
        fs::create_dir_all(&outside).unwrap();
        fs::write(outside.join("panel.html"), b"<h1>outside</h1>").unwrap();
        symlink(&outside, base.join("escape")).unwrap();

        let error =
            validate_containment(&base, &base.join("escape").join("panel.html")).unwrap_err();
        assert_eq!(error.code(), UNSAFE_PATH);
        assert_eq!(error.message(), "resolves outside the plugin directory");
    }

    #[test]
    fn containment_rejects_the_base_itself() {
        let root = tempdir().unwrap();
        let error = validate_containment(root.path(), root.path()).unwrap_err();
        assert_eq!(error.code(), UNSAFE_PATH);
    }

    #[test]
    fn containment_accepts_symlinks_resolving_inside_the_base() {
        let root = tempdir().unwrap();
        let real = root.path().join("real");
        fs::create_dir_all(&real).unwrap();
        fs::write(real.join("file.txt"), b"hi").unwrap();
        symlink(real.join("file.txt"), root.path().join("link.txt")).unwrap();

        let expected = fs::canonicalize(real.join("file.txt")).unwrap();
        let resolved = validate_containment(root.path(), &root.path().join("link.txt")).unwrap();
        assert_eq!(resolved, expected);
    }

    #[test]
    fn containment_reports_missing_candidates_as_not_found() {
        let root = tempdir().unwrap();
        let error =
            validate_containment(root.path(), &root.path().join("missing.txt")).unwrap_err();
        assert_eq!(error.code(), IO_ERROR);
        assert_eq!(error.message(), "not found");
    }

    #[test]
    fn containment_reports_missing_bases() {
        let root = tempdir().unwrap();
        let error = validate_containment(&root.path().join("missing"), root.path()).unwrap_err();
        assert_eq!(error.code(), IO_ERROR);
        assert!(error
            .message()
            .contains("failed to resolve the plugin directory"));
    }
}
