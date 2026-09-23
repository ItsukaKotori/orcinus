use uuid::Uuid;

pub fn new_uuid() -> String {
    Uuid::new_v4().to_string()
}

pub fn worktree_id(repo_id: &str, path: &str) -> String {
    format!("{repo_id}::{path}")
}

pub fn folder_workspace_root_id(repo_id: &str, path: &str) -> String {
    worktree_id(repo_id, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn worktree_id_uses_double_colon_separator() {
        assert_eq!(worktree_id("repo-1", "/tmp/proj"), "repo-1::/tmp/proj");
    }

    #[test]
    fn new_uuid_is_non_empty_and_unique() {
        let first = new_uuid();
        let second = new_uuid();
        assert!(!first.is_empty());
        assert_ne!(first, second);
    }

    #[test]
    fn folder_workspace_root_id_matches_worktree_id_format() {
        assert_eq!(folder_workspace_root_id("repo-1", "/tmp/proj"), "repo-1::/tmp/proj");
    }
}
