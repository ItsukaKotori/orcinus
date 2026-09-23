#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitWorktreeEntry {
    pub path: String,
    pub head: String,
    pub branch: Option<String>,
    pub is_bare: bool,
    pub is_main_worktree: bool,
}

/// Parse `git worktree list --porcelain -z` output. The first block is the main worktree.
pub fn parse_worktree_list(bytes: &[u8]) -> Vec<GitWorktreeEntry> {
    let mut entries = Vec::new();
    for (block_index, block) in split_blocks(bytes).iter().enumerate() {
        let mut path: Option<String> = None;
        let mut head = String::new();
        let mut branch: Option<String> = None;
        let mut is_bare = false;
        let mut is_prunable = false;

        for record in block {
            let text = String::from_utf8_lossy(record);
            if let Some(rest) = text.strip_prefix("worktree ") {
                path = Some(rest.to_string());
            } else if let Some(rest) = text.strip_prefix("HEAD ") {
                head = rest.to_string();
            } else if let Some(rest) = text.strip_prefix("branch ") {
                branch = Some(rest.to_string());
            } else if text == "bare" {
                is_bare = true;
            } else if text == "prunable" || text.starts_with("prunable ") {
                is_prunable = true;
            }
        }

        if is_prunable {
            continue;
        }
        let Some(path) = path else {
            continue;
        };
        entries.push(GitWorktreeEntry {
            path,
            head,
            branch,
            is_bare,
            is_main_worktree: block_index == 0,
        });
    }
    entries
}

/// Split NUL-terminated records into blocks; an empty record ends a block.
fn split_blocks(bytes: &[u8]) -> Vec<Vec<&[u8]>> {
    let mut blocks = Vec::new();
    let mut current = Vec::new();
    for record in bytes.split(|byte| *byte == 0) {
        if record.is_empty() {
            if !current.is_empty() {
                blocks.push(std::mem::take(&mut current));
            }
        } else {
            current.push(record);
        }
    }
    if !current.is_empty() {
        blocks.push(current);
    }
    blocks
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(
        path: &str,
        head: &str,
        branch: Option<&str>,
        is_bare: bool,
        is_main_worktree: bool,
    ) -> GitWorktreeEntry {
        GitWorktreeEntry {
            path: path.to_string(),
            head: head.to_string(),
            branch: branch.map(str::to_string),
            is_bare,
            is_main_worktree,
        }
    }

    #[test]
    fn parses_main_and_linked_worktrees_in_output_order() {
        let bytes = b"worktree /repos/demo\0HEAD 1111111111111111111111111111111111111111\0branch refs/heads/main\0\0worktree /repos/demo-wt-feature\0HEAD 2222222222222222222222222222222222222222\0branch refs/heads/feature/login\0\0";

        assert_eq!(
            parse_worktree_list(bytes),
            vec![
                entry(
                    "/repos/demo",
                    "1111111111111111111111111111111111111111",
                    Some("refs/heads/main"),
                    false,
                    true,
                ),
                entry(
                    "/repos/demo-wt-feature",
                    "2222222222222222222222222222222222222222",
                    Some("refs/heads/feature/login"),
                    false,
                    false,
                ),
            ]
        );
    }

    #[test]
    fn detached_head_has_no_branch_field() {
        let bytes = b"worktree /repos/demo-wt-detached\0HEAD 3333333333333333333333333333333333333333\0detached\0\0";

        assert_eq!(
            parse_worktree_list(bytes),
            vec![entry(
                "/repos/demo-wt-detached",
                "3333333333333333333333333333333333333333",
                None,
                false,
                true,
            )]
        );
    }

    #[test]
    fn bare_worktree_has_empty_head_and_no_branch() {
        let bytes = b"worktree /repos/demo.git\0bare\0\0";

        assert_eq!(
            parse_worktree_list(bytes),
            vec![entry("/repos/demo.git", "", None, true, true)]
        );
    }

    #[test]
    fn prunable_block_is_skipped() {
        let bytes = b"worktree /repos/demo\0HEAD 1111111111111111111111111111111111111111\0branch refs/heads/main\0\0worktree /repos/demo-wt-gone\0HEAD 4444444444444444444444444444444444444444\0branch refs/heads/gone\0prunable gitdir file points to non-existent location\0\0";

        assert_eq!(
            parse_worktree_list(bytes),
            vec![entry(
                "/repos/demo",
                "1111111111111111111111111111111111111111",
                Some("refs/heads/main"),
                false,
                true,
            )]
        );
    }

    #[test]
    fn locked_block_is_kept() {
        let bytes = b"worktree /repos/demo-wt-locked\0HEAD 5555555555555555555555555555555555555555\0branch refs/heads/locked\0locked working tree is locked\0\0";

        assert_eq!(
            parse_worktree_list(bytes),
            vec![entry(
                "/repos/demo-wt-locked",
                "5555555555555555555555555555555555555555",
                Some("refs/heads/locked"),
                false,
                true,
            )]
        );
    }

    #[test]
    fn empty_input_yields_no_entries() {
        assert_eq!(parse_worktree_list(b""), Vec::new());
    }
}
