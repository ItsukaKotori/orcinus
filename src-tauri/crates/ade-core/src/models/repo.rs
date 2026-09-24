use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// First entry of the renderer `REPO_COLORS` palette (oracle
/// `DEFAULT_REPO_BADGE_COLOR = REPO_COLORS[0]`).
pub const DEFAULT_REPO_BADGE_COLOR: &str = "#737373";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub enum RepoKind {
    Git,
    Folder,
}

impl RepoKind {
    pub fn as_str(self) -> &'static str {
        match self {
            RepoKind::Git => "git",
            RepoKind::Folder => "folder",
        }
    }

    /// Unknown kinds fall back to `git` (oracle `getRepoKind`).
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "git" => Some(RepoKind::Git),
            "folder" => Some(RepoKind::Folder),
            _ => None,
        }
    }
}

/// Last path segment, ignoring trailing separators (`/repo/` -> `repo`).
pub fn basename(path: &str) -> String {
    let trimmed = path.trim_end_matches(['/', '\\']);
    trimmed
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or_default()
        .to_string()
}

/// `displayName = trim(displayName) || basename(path) without a trailing .git`
/// (oracle `getRepoName`).
pub fn repo_display_name(path: &str, display_name: Option<&str>) -> String {
    let trimmed = display_name.unwrap_or_default().trim();
    if !trimmed.is_empty() {
        return trimmed.to_string();
    }
    let name = basename(path);
    match name.strip_suffix(".git") {
        Some(stripped) => stripped.to_string(),
        None => name,
    }
}

/// Build a new registry repo row (spec §5.2): `id`, `path`, `displayName`,
/// `badgeColor`, `addedAt`, `kind`; git-kind rows also carry
/// `externalWorktreeVisibilityLegacy=false`. Every other optional field is
/// absent.
pub fn new_repo(
    id: &str,
    path: &str,
    display_name: Option<&str>,
    kind: RepoKind,
    added_at_ms: u64,
) -> Value {
    let mut repo = Map::new();
    repo.insert("id".to_string(), Value::String(id.to_string()));
    repo.insert("path".to_string(), Value::String(path.to_string()));
    repo.insert(
        "displayName".to_string(),
        Value::String(repo_display_name(path, display_name)),
    );
    repo.insert(
        "badgeColor".to_string(),
        Value::String(DEFAULT_REPO_BADGE_COLOR.to_string()),
    );
    repo.insert("addedAt".to_string(), Value::from(added_at_ms));
    repo.insert("kind".to_string(), Value::String(kind.as_str().to_string()));
    if kind == RepoKind::Git {
        repo.insert(
            "externalWorktreeVisibilityLegacy".to_string(),
            Value::Bool(false),
        );
    }
    Value::Object(repo)
}

/// Current wall-clock milliseconds since the Unix epoch.
pub fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn kind_parses_lowercase_and_falls_back() {
        assert_eq!(RepoKind::parse("git"), Some(RepoKind::Git));
        assert_eq!(RepoKind::parse("folder"), Some(RepoKind::Folder));
        assert_eq!(RepoKind::parse("Folder"), None);
        assert_eq!(RepoKind::parse(""), None);
        assert_eq!(RepoKind::Git.as_str(), "git");
        assert_eq!(RepoKind::Folder.as_str(), "folder");
    }

    #[test]
    fn basename_ignores_trailing_separators() {
        assert_eq!(basename("/repo/"), "repo");
        assert_eq!(basename("/repo"), "repo");
        assert_eq!(basename("C:\\repo\\"), "repo");
        assert_eq!(basename("/"), "");
    }

    #[test]
    fn display_name_prefers_trimmed_input_then_basename_without_git_suffix() {
        assert_eq!(repo_display_name("/repo", Some("  Nice  ")), "Nice");
        assert_eq!(repo_display_name("/repo", Some("   ")), "repo");
        assert_eq!(repo_display_name("/repo/", None), "repo");
        assert_eq!(repo_display_name("/code/demo.git", None), "demo");
        assert_eq!(repo_display_name("/code/demo.git/", None), "demo");
    }

    #[test]
    fn new_git_repo_matches_spec_fields() {
        let repo = new_repo("r1", "/repo", None, RepoKind::Git, 42);
        assert_eq!(
            repo,
            json!({
                "id": "r1",
                "path": "/repo",
                "displayName": "repo",
                "badgeColor": "#737373",
                "addedAt": 42,
                "kind": "git",
                "externalWorktreeVisibilityLegacy": false
            })
        );
    }

    #[test]
    fn new_folder_repo_omits_git_only_field() {
        let repo = new_repo("r1", "/folder", Some("Folder"), RepoKind::Folder, 7);
        assert_eq!(repo["kind"], "folder");
        assert!(repo.get("externalWorktreeVisibilityLegacy").is_none());
    }
}
