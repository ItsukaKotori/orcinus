//! Helper app path resolution (spec §3.1): env override → packaged resource →
//! development build output. Every candidate is only accepted when it exists.

use std::path::{Path, PathBuf};

/// Bundle-relative executable name inside
/// `Orcinus Computer Use.app/Contents/MacOS/`.
pub const HELPER_EXECUTABLE_NAME: &str = "orcinus-computer-use-macos";
/// Packaged helper directory name under the Tauri resource directory.
pub const HELPER_RESOURCE_SUBDIR: &str = "computer-use";
/// Helper `.app` bundle directory name.
pub const HELPER_APP_NAME: &str = "Orcinus Computer Use.app";
/// Environment override honored before any other resolution strategy.
pub const HELPER_APP_PATH_ENV: &str = "ORCINUS_COMPUTER_MACOS_HELPER_APP_PATH";
/// Default bundle identifier baked into the generated Info.plist.
pub const DEFAULT_HELPER_BUNDLE_ID: &str = "dev.itsuka.orcinus.computer-use";

/// Resolve the helper `.app` path. `env_override` takes precedence (used
/// verbatim when it exists), then `resource_dir/computer-use/Orcinus Computer
/// Use.app`, then the development build output under
/// `native/computer-use-macos/.build/release/`. Non-existent candidates are
/// skipped; returns `None` when nothing is available.
pub fn helper_app_path(env_override: Option<&str>, resource_dir: Option<&Path>) -> Option<PathBuf> {
    if let Some(candidate) = env_override.filter(|value| !value.is_empty()) {
        let path = PathBuf::from(candidate);
        return path.exists().then_some(path);
    }
    if let Some(dir) = resource_dir {
        let candidate = dir.join(HELPER_RESOURCE_SUBDIR).join(HELPER_APP_NAME);
        if candidate.exists() {
            return Some(candidate);
        }
    }
    dev_helper_app_path()
}

fn dev_helper_app_path() -> Option<PathBuf> {
    let candidate = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../native/computer-use-macos/.build/release")
        .join(HELPER_APP_NAME);
    candidate.exists().then_some(candidate)
}

/// Path to the helper executable inside an `.app` bundle.
pub fn helper_executable(app: &Path) -> PathBuf {
    app.join("Contents/MacOS").join(HELPER_EXECUTABLE_NAME)
}

/// Extract `CFBundleIdentifier` from `<app>/Contents/Info.plist` with a plain
/// text scan (the permission module uses this for `tccutil`). Returns `None`
/// when the plist is missing or has no identifier.
pub fn helper_bundle_id(app: &Path) -> Option<String> {
    let contents = std::fs::read_to_string(app.join("Contents/Info.plist")).ok()?;
    extract_plist_string(&contents, "CFBundleIdentifier")
}

fn extract_plist_string(plist: &str, key: &str) -> Option<String> {
    let key_marker = format!("<key>{key}</key>");
    let remainder = &plist[plist.find(&key_marker)? + key_marker.len()..];
    let open = remainder.find("<string>")? + "<string>".len();
    let close = remainder[open..].find("</string>")? + open;
    let value = remainder[open..close].trim();
    (!value.is_empty()).then(|| value.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::atomic::{AtomicU64, Ordering};

    static DIR_COUNTER: AtomicU64 = AtomicU64::new(0);

    struct TempDir {
        path: PathBuf,
    }

    impl TempDir {
        fn new() -> Self {
            let unique = format!(
                "ade-computer-paths-{}-{}",
                std::process::id(),
                DIR_COUNTER.fetch_add(1, Ordering::Relaxed)
            );
            let path = std::env::temp_dir().join(unique);
            fs::create_dir_all(&path).expect("create temp dir");
            Self { path }
        }

        fn create_helper_app(&self) -> PathBuf {
            let app = self.path.join(HELPER_APP_NAME);
            fs::create_dir_all(app.join("Contents/MacOS")).expect("create app bundle");
            app
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    #[test]
    fn env_override_wins_when_it_exists() {
        let temp = TempDir::new();
        let env_app = temp.create_helper_app();
        let resource_app = temp
            .path
            .join("resource/computer-use/Orcinus Computer Use.app");
        fs::create_dir_all(&resource_app).unwrap();
        let resolved = helper_app_path(
            Some(env_app.to_str().unwrap()),
            Some(&temp.path.join("resource")),
        );
        assert_eq!(resolved, Some(env_app));
    }

    #[test]
    fn env_override_missing_does_not_fall_back() {
        let temp = TempDir::new();
        let resource_app = temp
            .path
            .join("resource/computer-use/Orcinus Computer Use.app");
        fs::create_dir_all(&resource_app).unwrap();
        let missing = temp.path.join("does-not-exist.app");
        assert_eq!(
            helper_app_path(
                Some(missing.to_str().unwrap()),
                Some(&temp.path.join("resource"))
            ),
            None
        );
    }

    #[test]
    fn packaged_resource_is_used_when_no_override() {
        let temp = TempDir::new();
        let resource_dir = temp.path.join("resource");
        let packaged = resource_dir.join("computer-use/Orcinus Computer Use.app");
        fs::create_dir_all(&packaged).unwrap();
        assert_eq!(helper_app_path(None, Some(&resource_dir)), Some(packaged));
    }

    fn dev_helper_app() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../native/computer-use-macos/.build/release/Orcinus Computer Use.app")
    }

    #[test]
    fn missing_resource_dir_falls_back_to_dev_only_when_dev_helper_exists() {
        let temp = TempDir::new();
        let resource_dir = temp.path.join("empty-resource");
        fs::create_dir_all(&resource_dir).unwrap();
        let expected = dev_helper_app();
        assert_eq!(
            helper_app_path(None, Some(&resource_dir)),
            expected.exists().then_some(expected)
        );
    }

    #[test]
    fn dev_fallback_resolution_is_consistent_with_expected_location() {
        let expected = dev_helper_app();
        assert_eq!(dev_helper_app_path(), expected.exists().then_some(expected));
    }

    #[test]
    fn helper_executable_lives_under_contents_macos() {
        let app = Path::new("/Applications/Orcinus Computer Use.app");
        assert_eq!(
            helper_executable(app),
            Path::new(
                "/Applications/Orcinus Computer Use.app/Contents/MacOS/orcinus-computer-use-macos"
            )
        );
    }

    #[test]
    fn helper_bundle_id_reads_identifier_from_info_plist() {
        let temp = TempDir::new();
        let app = temp.create_helper_app();
        fs::write(
            app.join("Contents/Info.plist"),
            r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
	<key>CFBundleName</key>
	<string>Orcinus Computer Use</string>
	<key>CFBundleIdentifier</key>
	<string>dev.itsuka.orcinus.computer-use</string>
	<key>LSUIElement</key>
	<true/>
</dict>
</plist>
"#,
        )
        .unwrap();
        assert_eq!(
            helper_bundle_id(&app),
            Some(DEFAULT_HELPER_BUNDLE_ID.to_string())
        );
    }

    #[test]
    fn helper_bundle_id_absent_when_plist_or_key_missing() {
        let temp = TempDir::new();
        let app = temp.create_helper_app();
        assert_eq!(helper_bundle_id(&app), None);
        fs::write(
            app.join("Contents/Info.plist"),
            "<plist version=\"1.0\"><dict><key>CFBundleName</key><string>x</string></dict></plist>",
        )
        .unwrap();
        assert_eq!(helper_bundle_id(&app), None);
    }
}
