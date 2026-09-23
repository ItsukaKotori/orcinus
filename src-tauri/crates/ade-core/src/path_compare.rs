use unicode_normalization::UnicodeNormalization;

/// Normalize a path for equality comparison: NFC, fold backslashes, lowercase on Windows.
pub fn normalize_for_comparison(path: &str) -> String {
    let normalized: String = path.nfc().collect();
    let folded = normalized.replace('\\', "/");
    if cfg!(windows) {
        folded.to_lowercase()
    } else {
        folded
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
}
