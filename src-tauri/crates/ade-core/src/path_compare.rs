use unicode_normalization::UnicodeNormalization;

/// Normalize a path for equality comparison: NFC, fold backslashes, trim trailing
/// separators, lowercase on Windows.
pub fn normalize_for_comparison(path: &str) -> String {
    let normalized: String = path.nfc().collect();
    let folded = normalized.replace('\\', "/");
    let trimmed = trim_trailing_separators(&folded);
    if cfg!(windows) {
        trimmed.to_lowercase()
    } else {
        trimmed.to_string()
    }
}

/// Whether `candidate` is `root` itself or sits below it at a segment boundary
/// (oracle `isPathInsideOrEqual`): `/root-other` is not inside `/root`.
pub fn is_path_inside_or_equal(root: &str, candidate: &str) -> bool {
    let root = normalize_for_comparison(root);
    let candidate = normalize_for_comparison(candidate);
    if candidate == root {
        return true;
    }
    let root_with_boundary = if root == "/" || is_windows_drive_root(&root) {
        root
    } else {
        format!("{root}/")
    };
    candidate.starts_with(&root_with_boundary)
}

fn is_windows_drive_root(path: &str) -> bool {
    let bytes = path.as_bytes();
    let drive = bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':';
    match bytes.len() {
        2 => drive,
        3 => drive && bytes[2] == b'/',
        _ => false,
    }
}

/// Trailing separators are dropped so `/repo/` and `/repo` compare equal; the
/// POSIX root `/` and Windows drive roots (`C:/`) are preserved (oracle
/// `trimRuntimePathTrailingSlash`).
fn trim_trailing_separators(path: &str) -> &str {
    if !path.ends_with('/') || path == "/" {
        return path;
    }
    let trimmed = path.trim_end_matches('/');
    if trimmed.len() == 2
        && trimmed.as_bytes()[0].is_ascii_alphabetic()
        && trimmed.as_bytes()[1] == b':'
    {
        return path;
    }
    if trimmed.is_empty() {
        "/"
    } else {
        trimmed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folds_backslashes_and_normalizes_unicode() {
        assert_eq!(normalize_for_comparison("C:\\Repo\\A"), if cfg!(windows) { "c:/repo/a".to_string() } else { "C:/Repo/A".to_string() });
        assert_eq!(normalize_for_comparison("e\u{0301}"), "é");
    }

    #[test]
    fn trims_trailing_separators_so_a_slash_spelling_dedupes() {
        assert_eq!(
            normalize_for_comparison("/repo/"),
            normalize_for_comparison("/repo")
        );
        assert_eq!(
            normalize_for_comparison("/repo//"),
            normalize_for_comparison("/repo")
        );
        assert_eq!(
            normalize_for_comparison("C:\\Repo\\"),
            normalize_for_comparison("C:/Repo")
        );
    }

    #[test]
    fn preserves_root_spellings() {
        assert_eq!(normalize_for_comparison("/"), "/");
        assert_eq!(normalize_for_comparison("///"), "/");
        assert_eq!(normalize_for_comparison("C:/"), if cfg!(windows) { "c:/" } else { "C:/" });
    }

    #[test]
    fn path_inside_requires_a_segment_boundary() {
        assert!(is_path_inside_or_equal("/root", "/root"));
        assert!(is_path_inside_or_equal("/root/", "/root"));
        assert!(is_path_inside_or_equal("/root", "/root/a/b"));
        assert!(!is_path_inside_or_equal("/root", "/root-other"));
        assert!(!is_path_inside_or_equal("/root", "/other"));
        assert!(is_path_inside_or_equal("/", "/anything"));
        assert!(!is_path_inside_or_equal("/root", "/"));
    }
}
