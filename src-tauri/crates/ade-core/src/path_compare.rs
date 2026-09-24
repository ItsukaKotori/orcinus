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
}
