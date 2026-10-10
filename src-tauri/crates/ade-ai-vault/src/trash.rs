//! Move a path to the macOS Trash. No unlink fallback: a trash that fails is
//! reported as a failure, never silently downgraded to a permanent delete.

use std::path::Path;

/// Move one path (file or directory, recursively) to the user's Trash via
/// `NSFileManager.trashItemAtURL:resultingItemURL:error:`. `Err` carries the
/// `NSError`'s localized description.
#[cfg(target_os = "macos")]
pub fn trash_path(path: &Path) -> Result<(), String> {
    use objc2::rc::autoreleasepool;
    use objc2_foundation::{NSFileManager, NSString, NSURL};

    let path = path.to_string_lossy();
    autoreleasepool(|_pool| {
        let url = NSURL::fileURLWithPath(&NSString::from_str(&path));
        NSFileManager::defaultManager()
            .trashItemAtURL_resultingItemURL_error(&url, None)
            .map_err(|error| error.localizedDescription().to_string())
    })
}

/// Non-macOS stub: this port is macOS/POSIX-only, but the crate must still
/// build elsewhere (e.g. CI cross-checks) — trashing simply fails there.
#[cfg(not(target_os = "macos"))]
pub fn trash_path(_path: &Path) -> Result<(), String> {
    Err("trashing is only supported on macOS".to_string())
}

#[cfg(test)]
mod tests {
    use super::trash_path;

    #[test]
    fn moves_a_file_out_of_its_directory() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir
            .path()
            .join(format!("ade-ai-vault-trash-{}.txt", std::process::id()));
        std::fs::write(&file, "trash me").unwrap();

        trash_path(&file).unwrap();

        assert!(!file.exists());
    }

    #[test]
    fn errors_on_a_missing_path() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("already-gone.txt");

        let error = trash_path(&missing).unwrap_err();

        assert!(!error.is_empty());
    }
}
