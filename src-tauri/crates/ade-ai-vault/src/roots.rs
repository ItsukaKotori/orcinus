use std::path::{Path, PathBuf};

pub fn claude_projects_root(home: &Path) -> PathBuf {
    home.join(".claude").join("projects")
}

pub fn opencode_data_dir(home: &Path, xdg: Option<&str>) -> PathBuf {
    match xdg.map(str::trim).filter(|value| !value.is_empty()) {
        Some(xdg) => Path::new(xdg).join("opencode"),
        None => home.join(".local").join("share").join("opencode"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claude_projects_root_joins_home() {
        assert_eq!(
            claude_projects_root(Path::new("/Users/alice")),
            PathBuf::from("/Users/alice/.claude/projects")
        );
    }

    #[test]
    fn opencode_data_dir_defaults_to_the_home_data_dir() {
        let home = Path::new("/Users/alice");
        assert_eq!(
            opencode_data_dir(home, None),
            PathBuf::from("/Users/alice/.local/share/opencode")
        );
        assert_eq!(
            opencode_data_dir(home, Some("")),
            PathBuf::from("/Users/alice/.local/share/opencode")
        );
        assert_eq!(
            opencode_data_dir(home, Some("   ")),
            PathBuf::from("/Users/alice/.local/share/opencode")
        );
    }

    #[test]
    fn opencode_data_dir_prefers_a_non_empty_xdg_data_home() {
        let home = Path::new("/Users/alice");
        assert_eq!(
            opencode_data_dir(home, Some("/xdg/data")),
            PathBuf::from("/xdg/data/opencode")
        );
        assert_eq!(
            opencode_data_dir(home, Some("  /xdg/data  ")),
            PathBuf::from("/xdg/data/opencode")
        );
    }
}
