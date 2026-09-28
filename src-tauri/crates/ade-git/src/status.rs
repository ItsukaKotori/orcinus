use serde::Serialize;

/// One changed path as reported by `git status --porcelain=v2`.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "lowercase")]
pub enum GitFileStatus {
    Modified,
    Added,
    Deleted,
    Renamed,
    Untracked,
    Copied,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "lowercase")]
pub enum GitStagingArea {
    Staged,
    Unstaged,
    Untracked,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "snake_case")]
pub enum GitConflictKind {
    BothModified,
    BothAdded,
    BothDeleted,
    AddedByUs,
    AddedByThem,
    DeletedByUs,
    DeletedByThem,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "kebab-case")]
pub enum GitConflictOperation {
    Merge,
    Rebase,
    CherryPick,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "snake_case")]
pub enum GitConflictResolutionStatus {
    Unresolved,
    ResolvedLocally,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "lowercase")]
pub enum GitConflictStatusSource {
    Git,
    Session,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct GitSubmoduleStatus {
    pub commit_changed: bool,
    pub tracked_changes: bool,
    pub untracked_changes: bool,
}

/// One added/removed pair in a [`GitBranchLineTotal`] bucket.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct LineStat {
    pub added: u64,
    pub removed: u64,
}

/// One row of the Source Control list. Mirrors the renderer contract
/// (`src/shared/git-status-types.ts`); absent optionals are omitted on the wire.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct GitStatusEntry {
    pub path: String,
    pub status: GitFileStatus,
    pub area: GitStagingArea,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub old_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub conflict_kind: Option<GitConflictKind>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub conflict_status: Option<GitConflictResolutionStatus>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub conflict_status_source: Option<GitConflictStatusSource>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub submodule: Option<GitSubmoduleStatus>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub submodule_root: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub added: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub removed: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct GitUpstreamStatus {
    pub has_upstream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub upstream_name: Option<String>,
    pub ahead: u64,
    pub behind: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub has_configured_push_target: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub behind_commits_are_patch_equivalent: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct GitBranchLineTotal {
    pub added: u64,
    pub removed: u64,
    pub merge_base: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub test: Option<LineStat>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub generated: Option<LineStat>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct GitStatusResult {
    pub entries: Vec<GitStatusEntry>,
    pub conflict_operation: GitConflictOperation,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub head: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub upstream_status: Option<GitUpstreamStatus>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ignored_paths: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub did_hit_limit: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status_length: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub branch_line_total: Option<GitBranchLineTotal>,
}

/// One changed row in Git's output order. Unmerged (`u`) rows defer their
/// worktree lookups, so the caller resolves them later; keeping them in this
/// ordered table lets a limit truncate the same rows Git emitted first.
#[derive(Debug, Clone, PartialEq)]
pub enum StatusRecord {
    Entry(GitStatusEntry),
    Unmerged(String),
}

/// Parser output before the caller resolves unmerged rows and merges branch
/// metadata into a [`GitStatusResult`].
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ParsedStatus {
    pub entries: Vec<GitStatusEntry>,
    pub records: Vec<StatusRecord>,
    pub ignored_paths: Vec<String>,
    pub unmerged_lines: Vec<String>,
    pub head: Option<String>,
    pub branch: Option<String>,
    pub upstream_name: Option<String>,
    pub ahead_behind: Option<(u64, u64)>,
    pub changed_count: u64,
}

/// Incremental parser for `git status --porcelain=v2 --branch -z` output.
///
/// Why incremental: a repo with an enormous un-ignored folder can emit a
/// status listing too large to buffer. Feeding chunks as they arrive lets the
/// caller stop git the moment the changed-entry count crosses `limit`, so
/// memory stays bounded. Records are NUL-delimited; a partial trailing record
/// is carried across chunks.
///
/// Sync record types (`1`/`2`/`?`/`!`) are parsed here. Unmerged (`u`) records
/// need per-file worktree lookups, so their raw lines are collected for the
/// caller (but still count toward the limit). Type-2 rename records in `-z`
/// form put the original path in the next NUL fragment with no prefix of its
/// own; `pending_rename` holds how many trailing entries that fragment must
/// stamp once it arrives.
///
/// Once `update` reports the limit crossed the parser is *stopped*: no further
/// chunk is parsed and `finish` is a no-op, because whatever sits in the carry
/// is the tail of a record Git never finished writing (it was killed). Without
/// that, flushing the carry would emit one garbage row.
#[derive(Debug, Default)]
pub struct StatusParser {
    carry: Vec<u8>,
    pending_rename: Option<usize>,
    stopped: bool,
    entries: Vec<GitStatusEntry>,
    records: Vec<StatusRecord>,
    ignored_paths: Vec<String>,
    unmerged_lines: Vec<String>,
    head: Option<String>,
    branch: Option<String>,
    upstream_name: Option<String>,
    ahead_behind: Option<(u64, u64)>,
    changed_count: u64,
}

impl StatusParser {
    pub fn new() -> Self {
        Self::default()
    }

    /// Feed one chunk. Returns true once the accumulated changed-entry count
    /// exceeds `limit` (limit 0 disables the cap), signaling the caller to stop
    /// git. Complete records are parsed; an incomplete trailing record is
    /// carried, and already-parsed results are kept. The check runs after the
    /// whole chunk, so with a caller that buffers output this counts every
    /// changed row rather than stopping one row past the cap.
    pub fn update(&mut self, chunk: &[u8], limit: usize) -> bool {
        if self.stopped {
            return true;
        }
        self.carry.extend_from_slice(chunk);
        while let Some(nul) = self.carry.iter().position(|byte| *byte == 0) {
            let tail = self.carry.split_off(nul + 1);
            let mut record = std::mem::replace(&mut self.carry, tail);
            record.pop();
            self.parse_record(&record);
        }
        if limit != 0 && self.changed_count > limit as u64 {
            self.stopped = true;
            return true;
        }
        false
    }

    /// Flush a final record with no trailing NUL (e.g. when git exits). A
    /// stopped parser flushes nothing: the carry belongs to a killed Git.
    pub fn finish(&mut self) {
        if self.stopped || self.carry.is_empty() {
            return;
        }
        let record = std::mem::take(&mut self.carry);
        self.parse_record(&record);
    }

    pub fn into_parsed(self) -> ParsedStatus {
        ParsedStatus {
            entries: self.entries,
            records: self.records,
            ignored_paths: self.ignored_paths,
            unmerged_lines: self.unmerged_lines,
            head: self.head,
            branch: self.branch,
            upstream_name: self.upstream_name,
            ahead_behind: self.ahead_behind,
            changed_count: self.changed_count,
        }
    }

    fn parse_record(&mut self, record: &[u8]) {
        if record.is_empty() {
            return;
        }
        if let Some(pushed) = self.pending_rename.take() {
            let old_path = text(record);
            let start = self.entries.len().saturating_sub(pushed);
            for entry in &mut self.entries[start..] {
                entry.old_path = Some(old_path.clone());
            }
            let record_start = self.records.len().saturating_sub(pushed);
            for record in &mut self.records[record_start..] {
                if let StatusRecord::Entry(entry) = record {
                    entry.old_path = Some(old_path.clone());
                }
            }
            return;
        }
        if let Some(rest) = record.strip_prefix(b"# branch.oid ") {
            self.head = Some(text(rest).trim().to_string());
            return;
        }
        if let Some(rest) = record.strip_prefix(b"# branch.head ") {
            let value = text(rest);
            let value = value.trim();
            self.branch = if value.is_empty() || value == "(detached)" {
                None
            } else {
                Some(format!("refs/heads/{value}"))
            };
            return;
        }
        if let Some(rest) = record.strip_prefix(b"# branch.upstream ") {
            let value = text(rest);
            let value = value.trim();
            self.upstream_name = if value.is_empty() {
                None
            } else {
                Some(value.to_string())
            };
            return;
        }
        if let Some(rest) = record.strip_prefix(b"# branch.ab ") {
            if let Some(ahead_behind) = parse_ahead_behind(rest) {
                self.ahead_behind = Some(ahead_behind);
            }
            return;
        }
        if record.starts_with(b"1 ") {
            self.parse_changed_entry(record);
            return;
        }
        if record.starts_with(b"2 ") {
            self.parse_rename_entry(record);
            return;
        }
        if let Some(rest) = record.strip_prefix(b"? ") {
            self.push(new_entry(
                text(rest),
                GitFileStatus::Untracked,
                GitStagingArea::Untracked,
            ));
            return;
        }
        if let Some(rest) = record.strip_prefix(b"! ") {
            self.ignored_paths.push(text(rest));
            return;
        }
        if record.starts_with(b"u ") {
            let line = text(record);
            self.changed_count += 1;
            self.records.push(StatusRecord::Unmerged(line.clone()));
            self.unmerged_lines.push(line);
        }
    }

    /// `1 XY sub mH mI mW hH hI path` — index/worktree chars each emit a row.
    fn parse_changed_entry(&mut self, record: &[u8]) {
        let line = text(record);
        let parts: Vec<&str> = line.split(' ').collect();
        if parts.len() < 9 {
            return;
        }
        let xy = parts[1].as_bytes();
        if xy.len() != 2 {
            return;
        }
        let path = parts[8..].join(" ");
        let submodule_field = parts.get(2).copied();
        for (index, area) in [(0, GitStagingArea::Staged), (1, GitStagingArea::Unstaged)] {
            if xy[index] == b'.' {
                continue;
            }
            let status_char = xy[index] as char;
            let mut entry = new_entry(path.clone(), parse_status_char(status_char), area);
            entry.submodule = parse_submodule_status(submodule_field, status_char);
            self.push(entry);
        }
    }

    /// `2 XY sub mH mI mW hH X<score> path`, then the original path in the
    /// next NUL fragment (no `2 ` prefix).
    fn parse_rename_entry(&mut self, record: &[u8]) {
        let line = text(record);
        let parts: Vec<&str> = line.split(' ').collect();
        if parts.len() < 10 {
            return;
        }
        let xy = parts[1].as_bytes();
        if xy.len() != 2 {
            return;
        }
        let path = parts[9..].join(" ");
        let submodule_field = parts.get(2).copied();
        let mut pushed = 0;
        for (index, area) in [(0, GitStagingArea::Staged), (1, GitStagingArea::Unstaged)] {
            if xy[index] == b'.' {
                continue;
            }
            let status_char = xy[index] as char;
            let mut entry = new_entry(path.clone(), parse_status_char(status_char), area);
            entry.submodule = parse_submodule_status(submodule_field, status_char);
            self.push(entry);
            pushed += 1;
        }
        if pushed > 0 {
            self.pending_rename = Some(pushed);
        }
    }

    fn push(&mut self, entry: GitStatusEntry) {
        self.changed_count += 1;
        self.records.push(StatusRecord::Entry(entry.clone()));
        self.entries.push(entry);
    }
}

fn new_entry(path: String, status: GitFileStatus, area: GitStagingArea) -> GitStatusEntry {
    GitStatusEntry {
        path,
        status,
        area,
        old_path: None,
        conflict_kind: None,
        conflict_status: None,
        conflict_status_source: None,
        submodule: None,
        submodule_root: None,
        added: None,
        removed: None,
    }
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

fn parse_status_char(char: char) -> GitFileStatus {
    match char {
        'A' => GitFileStatus::Added,
        'D' => GitFileStatus::Deleted,
        'R' => GitFileStatus::Renamed,
        'C' => GitFileStatus::Copied,
        _ => GitFileStatus::Modified,
    }
}

fn parse_submodule_status(
    submodule_field: Option<&str>,
    status_char: char,
) -> Option<GitSubmoduleStatus> {
    let field = submodule_field?;
    if !field.starts_with('S') {
        return None;
    }
    let bytes = field.as_bytes();
    let char_at = |index: usize| bytes.get(index).map(|byte| *byte as char);
    Some(GitSubmoduleStatus {
        commit_changed: char_at(1) == Some('C') || (field == "S..." && status_char == 'M'),
        tracked_changes: char_at(2) == Some('M'),
        untracked_changes: char_at(3) == Some('U'),
    })
}

fn parse_ahead_behind(rest: &[u8]) -> Option<(u64, u64)> {
    let line = text(rest);
    let mut parts = line.split(' ');
    let ahead = parts.next()?.strip_prefix('+')?.parse().ok()?;
    let behind = parts.next()?.strip_prefix('-')?.parse().ok()?;
    if parts.next().is_some() {
        return None;
    }
    Some((ahead, behind))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn feed(records: &[&str], limit: usize) -> (StatusParser, bool) {
        let mut parser = StatusParser::new();
        let mut bytes = Vec::new();
        for record in records {
            bytes.extend_from_slice(record.as_bytes());
            bytes.push(0);
        }
        let stopped = parser.update(&bytes, limit);
        parser.finish();
        (parser, stopped)
    }

    #[test]
    fn parses_branch_headers_and_entries() {
        let (parser, stopped) = feed(
            &[
                "# branch.oid efbccd00b747859625ba07b4b6d4322cbe07b37",
                "# branch.head main",
                "# branch.upstream origin/main",
                "# branch.ab +2 -1",
                "1 M. N... 100644 100644 100644 61780798228d17af2d34fce4cfbdf35556832472 61780798228d17af2d34fce4cfbdf35556832472 src/app.ts",
                "1 .M N... 100644 100644 100644 1111111111111111111111111111111111111111 1111111111111111111111111111111111111111 src/other.ts",
                "? notes.txt",
            ],
            1000,
        );
        assert!(!stopped);
        let parsed = parser.into_parsed();
        assert_eq!(
            parsed.head.as_deref(),
            Some("efbccd00b747859625ba07b4b6d4322cbe07b37")
        );
        assert_eq!(parsed.branch.as_deref(), Some("refs/heads/main"));
        assert_eq!(parsed.upstream_name.as_deref(), Some("origin/main"));
        assert_eq!(parsed.ahead_behind, Some((2, 1)));
        assert_eq!(parsed.entries.len(), 3);
        assert_eq!(parsed.entries[0].area, GitStagingArea::Staged);
        assert_eq!(parsed.entries[0].status, GitFileStatus::Modified);
        assert_eq!(parsed.entries[1].area, GitStagingArea::Unstaged);
        assert_eq!(parsed.entries[2].status, GitFileStatus::Untracked);
    }

    #[test]
    fn detached_head_yields_no_branch() {
        let (parser, _) = feed(&["# branch.oid abc", "# branch.head (detached)"], 1000);
        assert_eq!(parser.into_parsed().branch, None);
    }

    #[test]
    fn type2_rename_z_takes_orig_path_from_next_chunk() {
        // `-z` 形态：type-2 记录的旧路径是紧随其后的独立 NUL 分片（见计划 Task 2 Step 3）
        let (parser, _) = feed(
            &[
                "2 R. N... 100644 100644 100644 61780798228d17af2d34fce4cfbdf35556832472 61780798228d17af2d34fce4cfbdf35556832472 R100 new name.txt",
                "has space.txt",
            ],
            1000,
        );
        let parsed = parser.into_parsed();
        assert_eq!(parsed.entries.len(), 1);
        assert_eq!(parsed.entries[0].path, "new name.txt");
        assert_eq!(parsed.entries[0].old_path.as_deref(), Some("has space.txt"));
        // 有序记录表同样带上旧路径，行的输出序不变。
        assert_eq!(
            parsed.records,
            vec![StatusRecord::Entry(parsed.entries[0].clone())]
        );
    }

    #[test]
    fn record_split_across_chunks_is_carried() {
        let mut parser = StatusParser::new();
        assert!(!parser.update(b"1 M. N... 100644 100644 100644 a a spl", 1000));
        assert!(!parser.update(b"it.txt\0? untracked.txt\0", 1000));
        parser.finish();

        let parsed = parser.into_parsed();
        assert_eq!(parsed.entries.len(), 2);
        assert_eq!(parsed.entries[0].path, "split.txt");
        assert_eq!(parsed.entries[1].path, "untracked.txt");
    }

    #[test]
    fn records_preserve_git_output_order() {
        let (parser, _) = feed(
            &[
                "1 .M N... 100644 100644 100644 a a first.txt",
                "u UU N... 100644 100644 100644 100644 a b c conflict.txt",
                "? last.txt",
            ],
            1000,
        );
        let parsed = parser.into_parsed();
        assert_eq!(parsed.records.len(), 3);
        match &parsed.records[0] {
            StatusRecord::Entry(entry) => assert_eq!(entry.path, "first.txt"),
            other => panic!("expected entry record, got {other:?}"),
        }
        match &parsed.records[1] {
            StatusRecord::Unmerged(line) => assert!(line.contains("conflict.txt")),
            other => panic!("expected unmerged record, got {other:?}"),
        }
        match &parsed.records[2] {
            StatusRecord::Entry(entry) => assert_eq!(entry.path, "last.txt"),
            other => panic!("expected entry record, got {other:?}"),
        }
    }

    #[test]
    fn limit_stop_discards_unfinished_carry_and_finish_is_noop() {
        let mut parser = StatusParser::new();

        let mut first = Vec::new();
        first.extend_from_slice(b"1 M. N... 100644 100644 100644 a a one\0");
        first.extend_from_slice(b"1 M. N... 100644 100644 100644 a a tw");
        assert!(!parser.update(&first, 2));

        let mut second = Vec::new();
        second.extend_from_slice(b"o\0");
        second.extend_from_slice(b"1 M. N... 100644 100644 100644 a a three\0");
        second.extend_from_slice(b"1 M. N... 100644 100644 100644 a a fo");
        assert!(parser.update(&second, 2));
        // 调用方在 stop 后不得把 carry 当成完整记录冲刷。
        assert!(parser.update(b"ur\0", 2));
        parser.finish();

        let parsed = parser.into_parsed();
        assert_eq!(parsed.changed_count, 3);
        assert_eq!(parsed.entries.len(), 3);
        assert!(parsed.entries.iter().all(|entry| entry.path != "fo"));
    }

    #[test]
    fn stops_when_changed_count_exceeds_limit() {
        let (parser, stopped) = feed(
            &[
                "1 M. N... 100644 100644 100644 a a one",
                "1 M. N... 100644 100644 100644 a a two",
                "1 M. N... 100644 100644 100644 a a three",
            ],
            2,
        );
        assert!(stopped);
        let parsed = parser.into_parsed();
        assert_eq!(parsed.changed_count, 3); // statusLength 含越限计数
        assert_eq!(parsed.entries.len(), 3);
    }

    #[test]
    fn collects_ignored_and_unmerged_without_parsing() {
        let (parser, _) = feed(
            &[
                "! dist/",
                "u UU N... 100644 100644 100644 100644 a b c conflict.txt",
            ],
            1000,
        );
        let parsed = parser.into_parsed();
        assert_eq!(parsed.ignored_paths, vec!["dist/".to_string()]);
        assert_eq!(parsed.unmerged_lines.len(), 1);
        assert_eq!(parsed.changed_count, 1);
    }
}
