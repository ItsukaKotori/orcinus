use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use grep_matcher::Matcher;
use grep_regex::{RegexMatcher, RegexMatcherBuilder};
use grep_searcher::{BinaryDetection, Searcher, SearcherBuilder, Sink, SinkMatch};
use ignore::overrides::{Override, OverrideBuilder};
use ignore::WalkBuilder;
use serde::{Deserialize, Serialize};

use crate::walk::{CancelGuard, CancelRegistry};
use crate::{FsError, FsService};

pub const MAX_RESULTS_DEFAULT: usize = 2000;
pub const MAX_RESULTS_CAP: usize = 2000;
pub const PER_FILE_MAX_MATCHES: usize = 100;
pub const MAX_FILE_SIZE: u64 = 5 * 1024 * 1024;
pub const MAX_LINE_CONTENT_CHARS: usize = 500;
pub const SEARCH_TIMEOUT_MS: u64 = 15_000;

const TRUNCATION_MARKER: char = '…';

/// Search request from the renderer. Mirrors the oracle `SearchOptions`
/// contract (`src/shared/code-search-types.ts`) with camelCase serialization.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchOptions {
    pub query: String,
    pub root_path: String,
    #[serde(default)]
    pub case_sensitive: bool,
    #[serde(default)]
    pub whole_word: bool,
    #[serde(default)]
    pub use_regex: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub include_pattern: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exclude_pattern: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_results: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchMatch {
    pub line: usize,
    /// UTF-8 byte offset of the match in the line, plus one (bug-for-bug with
    /// ripgrep's byte submatches consumed by `text-search-match-accumulator`).
    pub column: usize,
    /// Match length in UTF-8 bytes.
    pub match_length: usize,
    pub line_content: String,
    /// Present only when `line_content` was window-truncated; offsets into the
    /// truncated snippet (characters), so the UI can highlight it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_column: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_match_length: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchFileResult {
    pub file_path: String,
    pub relative_path: String,
    pub matches: Vec<SearchMatch>,
    pub match_count: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchResult {
    pub files: Vec<SearchFileResult>,
    pub total_matches: usize,
    pub truncated: bool,
}

impl FsService {
    /// Search an authorized root with embedded ripgrep semantics.
    ///
    /// The `cancel` registry is keyed by the resolved root path: starting a
    /// new search for the same root aborts the previous run, which then
    /// resolves with the partial results collected so far (oracle behavior).
    pub fn search(
        &self,
        options: SearchOptions,
        cancel: CancelRegistry,
    ) -> Result<SearchResult, FsError> {
        let resolved = self.resolve(&options.root_path)?;
        let token = resolved.to_string_lossy().into_owned();
        let guard = CancelGuard::new(cancel, &token);
        search_resolved(
            &resolved,
            &options,
            Some(guard.flag()),
            Duration::from_millis(SEARCH_TIMEOUT_MS),
        )
    }
}

pub fn clamp_max_results(requested: Option<usize>) -> usize {
    requested
        .unwrap_or(MAX_RESULTS_DEFAULT)
        .clamp(1, MAX_RESULTS_CAP)
}

/// Split a comma-separated glob list, preserving `\`-escaped commas as glob
/// input. Mirrors `splitSearchGlobPatterns` in `text-search-glob-patterns.ts`.
pub fn split_search_glob_patterns(patterns: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut escaping = false;
    for character in patterns.chars() {
        if escaping {
            current.push('\\');
            current.push(character);
            escaping = false;
            continue;
        }
        match character {
            '\\' => escaping = true,
            ',' => {
                let trimmed = current.trim();
                if !trimmed.is_empty() {
                    out.push(trimmed.to_string());
                }
                current.clear();
            }
            _ => current.push(character),
        }
    }
    if escaping {
        current.push('\\');
    }
    let trimmed = current.trim();
    if !trimmed.is_empty() {
        out.push(trimmed.to_string());
    }
    out
}

/// Bare globs match recursively, mirroring `toGitGlobPathspec`.
pub fn to_glob_pattern(glob: &str) -> String {
    if glob.contains('/') {
        glob.to_string()
    } else {
        format!("**/{glob}")
    }
}

fn search_resolved(
    root: &Path,
    options: &SearchOptions,
    cancelled: Option<&AtomicBool>,
    timeout: Duration,
) -> Result<SearchResult, FsError> {
    let max_results = clamp_max_results(options.max_results);
    let matcher = build_matcher(options)?;
    let overrides = build_overrides(root, options)?;
    let mut searcher = SearcherBuilder::new()
        .line_number(true)
        .binary_detection(BinaryDetection::quit(0))
        .build();
    let deadline = Instant::now() + timeout;
    let mut sink = SearchSink::new(
        &matcher,
        max_results,
        options.query.is_empty(),
        cancelled,
        deadline,
    );

    let mut builder = WalkBuilder::new(root);
    builder
        // rg `--hidden`: the crate's toggle is inverted (`hidden(true)`
        // ignores hidden entries); the `!.git` override still prunes `.git`.
        .hidden(false)
        .parents(true)
        .follow_links(false)
        .git_ignore(true)
        // Why: the spec enumerates `.gitignore`/`.ignore`/`.rgignore`; keeping
        // the user's global gitignore out makes searches deterministic.
        .git_global(false)
        .git_exclude(true)
        .ignore(true)
        .require_git(true)
        .max_filesize(Some(MAX_FILE_SIZE))
        .overrides(overrides)
        .add_custom_ignore_filename(".rgignore");

    for entry in builder.build() {
        if sink.check_stop() {
            break;
        }
        let Ok(entry) = entry else {
            continue;
        };
        let Some(file_type) = entry.file_type() else {
            continue;
        };
        // Symlinks are neither followed nor searched (rg default).
        if !file_type.is_file() {
            continue;
        }
        let Some(absolute) = entry.path().to_str() else {
            continue;
        };
        let Ok(relative) = entry.path().strip_prefix(root) else {
            continue;
        };
        let relative = relative.to_string_lossy().replace('\\', "/");
        sink.begin_file(absolute.to_string(), relative);
        let _ = searcher.search_path(&matcher, entry.path(), &mut sink);
        sink.end_file();
    }
    Ok(sink.finish())
}

fn build_matcher(options: &SearchOptions) -> Result<RegexMatcher, FsError> {
    RegexMatcherBuilder::new()
        .case_insensitive(!options.case_sensitive)
        .word(options.whole_word)
        .fixed_strings(!options.use_regex)
        .build(&options.query)
        .map_err(|error| FsError::InvalidInput(format!("Invalid search pattern: {error}")))
}

fn build_overrides(root: &Path, options: &SearchOptions) -> Result<Override, FsError> {
    let mut builder = OverrideBuilder::new(root);
    builder.add("!.git").map_err(override_error)?;
    if let Some(include) = &options.include_pattern {
        for pattern in split_search_glob_patterns(include) {
            builder
                .add(&to_glob_pattern(&pattern))
                .map_err(override_error)?;
        }
    }
    if let Some(exclude) = &options.exclude_pattern {
        for pattern in split_search_glob_patterns(exclude) {
            builder
                .add(&format!("!{}", to_glob_pattern(&pattern)))
                .map_err(override_error)?;
        }
    }
    builder.build().map_err(override_error)
}

fn override_error(error: ignore::Error) -> FsError {
    FsError::InvalidInput(format!("Invalid search glob: {error}"))
}

struct CurrentFile {
    absolute: String,
    relative: String,
    result_index: Option<usize>,
    match_count: usize,
}

struct SearchSink<'a> {
    matcher: &'a RegexMatcher,
    max_results: usize,
    empty_query: bool,
    cancelled: Option<&'a AtomicBool>,
    deadline: Instant,
    files: Vec<SearchFileResult>,
    current: Option<CurrentFile>,
    total_matches: usize,
    truncated: bool,
    stopped: bool,
}

impl<'a> SearchSink<'a> {
    fn new(
        matcher: &'a RegexMatcher,
        max_results: usize,
        empty_query: bool,
        cancelled: Option<&'a AtomicBool>,
        deadline: Instant,
    ) -> Self {
        Self {
            matcher,
            max_results,
            empty_query,
            cancelled,
            deadline,
            files: Vec::new(),
            current: None,
            total_matches: 0,
            truncated: false,
            stopped: false,
        }
    }

    fn begin_file(&mut self, absolute: String, relative: String) {
        self.current = Some(CurrentFile {
            absolute,
            relative,
            result_index: None,
            match_count: 0,
        });
    }

    fn end_file(&mut self) {
        self.current = None;
    }

    /// Cancellation stops silently with partial results; the 15s timeout marks
    /// the result truncated. Both latch `stopped` for the traversal loop.
    fn check_stop(&mut self) -> bool {
        if self.stopped {
            return true;
        }
        if self
            .cancelled
            .is_some_and(|flag| flag.load(Ordering::SeqCst))
        {
            self.stopped = true;
            return true;
        }
        if Instant::now() >= self.deadline {
            self.truncated = true;
            self.stopped = true;
            return true;
        }
        false
    }

    fn push_match(
        &mut self,
        line: usize,
        column_start: usize,
        match_length: usize,
        line_content: Option<&str>,
    ) -> bool {
        if self.check_stop() {
            return false;
        }
        let (line_content, display_column, display_match_length) = match line_content {
            Some(text) => clamp_line_context(text, column_start, match_length),
            None => (String::new(), None, None),
        };
        let Some(current) = self.current.as_mut() else {
            return true;
        };
        if current.result_index.is_none() {
            self.files.push(SearchFileResult {
                file_path: current.absolute.clone(),
                relative_path: current.relative.clone(),
                matches: Vec::new(),
                match_count: 0,
            });
            current.result_index = Some(self.files.len() - 1);
        }
        let index = current.result_index.expect("result index set above");
        self.files[index].matches.push(SearchMatch {
            line,
            column: column_start + 1,
            match_length,
            line_content,
            display_column,
            display_match_length,
        });
        self.files[index].match_count += 1;
        current.match_count += 1;
        self.total_matches += 1;
        if self.total_matches >= self.max_results {
            self.truncated = true;
            self.stopped = true;
            return false;
        }
        if current.match_count >= PER_FILE_MAX_MATCHES {
            return false;
        }
        true
    }

    fn finish(self) -> SearchResult {
        SearchResult {
            files: self.files,
            total_matches: self.total_matches,
            truncated: self.truncated,
        }
    }
}

impl Sink for SearchSink<'_> {
    type Error = std::io::Error;

    fn matched(&mut self, _searcher: &Searcher, mat: &SinkMatch<'_>) -> Result<bool, Self::Error> {
        if self.check_stop() {
            return Ok(false);
        }
        let line_number = mat.line_number().unwrap_or(0) as usize;
        let raw = strip_line_terminator(mat.bytes());
        let line_content = std::str::from_utf8(raw).ok();
        let submatches: Vec<(usize, usize)> = if self.empty_query {
            Vec::new()
        } else {
            let mut ranges = Vec::new();
            let _ = self.matcher.find_iter(raw, |m| {
                ranges.push((m.start(), m.end()));
                true
            });
            ranges
        };
        if submatches.is_empty() {
            // Why: rg can report a line without submatch ranges (and an empty
            // query has none); keep a navigable line-level result instead.
            let length = line_content.map_or(0, |text| usize::from(!text.is_empty()));
            return Ok(self.push_match(line_number, 0, length, line_content));
        }
        for (start, end) in submatches {
            if !self.push_match(line_number, start, end - start, line_content) {
                return Ok(false);
            }
        }
        Ok(true)
    }
}

/// Strip one trailing `\n`; a preceding `\r` is left in place (oracle parity).
fn strip_line_terminator(bytes: &[u8]) -> &[u8] {
    match bytes.strip_suffix(b"\n") {
        Some(stripped) => stripped,
        None => bytes,
    }
}

/// Clamp the line context around a match to `MAX_LINE_CONTENT_CHARS`,
/// mirroring `clampLineContext` in `text-search-match-accumulator.ts` with
/// character windows (the primary `column`/`matchLength` stay byte-based).
fn clamp_line_context(
    text: &str,
    match_start: usize,
    match_length: usize,
) -> (String, Option<usize>, Option<usize>) {
    let total_chars = text.chars().count();
    if total_chars <= MAX_LINE_CONTENT_CHARS {
        return (text.to_string(), None, None);
    }
    let match_start = floor_char_boundary(text, match_start);
    let match_end = floor_char_boundary(text, match_start.saturating_add(match_length));
    let match_start_chars = text[..match_start].chars().count();
    let match_length_chars = text[match_start..match_end].chars().count();
    let clamped_match_length = match_length_chars.min(MAX_LINE_CONTENT_CHARS);
    let remaining = MAX_LINE_CONTENT_CHARS - clamped_match_length;
    let left_budget = remaining / 2;
    let mut window_start = match_start_chars.saturating_sub(left_budget);
    let window_end = (window_start + MAX_LINE_CONTENT_CHARS).min(total_chars);
    window_start = window_end.saturating_sub(MAX_LINE_CONTENT_CHARS);

    let mut snippet: String = text
        .chars()
        .skip(window_start)
        .take(window_end - window_start)
        .collect();
    let mut display_column = match_start_chars - window_start + 1;
    if window_start > 0 {
        snippet.insert(0, TRUNCATION_MARKER);
        display_column += 1;
    }
    if window_end < total_chars {
        snippet.push(TRUNCATION_MARKER);
    }
    (snippet, Some(display_column), Some(clamped_match_length))
}

fn floor_char_boundary(text: &str, mut offset: usize) -> usize {
    offset = offset.min(text.len());
    while offset > 0 && !text.is_char_boundary(offset) {
        offset -= 1;
    }
    offset
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::AtomicU64;

    static COUNTER: AtomicU64 = AtomicU64::new(0);

    struct TestDir {
        path: PathBuf,
    }

    impl TestDir {
        fn new(name: &str) -> Self {
            let unique = COUNTER.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "ade-fs-search-unit-{name}-{}-{unique}",
                std::process::id()
            ));
            let _ = fs::remove_dir_all(&path);
            fs::create_dir_all(&path).expect("create unit test dir");
            Self { path }
        }

        fn path(&self) -> &Path {
            &self.path
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    fn options(root: &Path, query: &str) -> SearchOptions {
        SearchOptions {
            query: query.to_string(),
            root_path: root.to_string_lossy().into_owned(),
            ..SearchOptions::default()
        }
    }

    #[test]
    fn splits_globs_like_the_shared_typescript_helper() {
        assert_eq!(
            split_search_glob_patterns("foo\\,bar/**, *.ts, dist/**"),
            vec!["foo\\,bar/**", "*.ts", "dist/**"]
        );
        assert_eq!(split_search_glob_patterns("src\\"), vec!["src\\"]);
        assert_eq!(
            split_search_glob_patterns(" , *.rs , "),
            vec!["*.rs"],
            "blank entries are dropped"
        );
        assert!(split_search_glob_patterns("").is_empty());
    }

    #[test]
    fn prefixes_bare_globs_with_recursive_match() {
        assert_eq!(to_glob_pattern("*.ts"), "**/*.ts");
        assert_eq!(to_glob_pattern("src/*.ts"), "src/*.ts");
        assert_eq!(to_glob_pattern("foo\\,bar/**"), "foo\\,bar/**");
    }

    #[test]
    fn normalized_globs_match_like_gitignore_semantics() {
        use globset::{GlobBuilder, GlobMatcher};

        // `literal_separator` mirrors the gitignore semantics the `ignore`
        // override matcher (ripgrep's glob engine) applies.
        let compile = |pattern: &str| -> GlobMatcher {
            GlobBuilder::new(&to_glob_pattern(pattern))
                .literal_separator(true)
                .build()
                .expect("valid glob")
                .compile_matcher()
        };
        assert!(compile("*.ts").is_match("nested/deep/a.ts"));
        assert!(compile("*.ts").is_match("a.ts"));
        assert!(!compile("*.ts").is_match("a.md"));
        assert!(compile("src/*.ts").is_match("src/a.ts"));
        assert!(!compile("src/*.ts").is_match("src/deep/a.ts"));
        assert!(compile("foo\\,bar/**").is_match("foo,bar/x.ts"));
        assert!(!compile("foo\\,bar/**").is_match("foo/x.ts"));
    }

    #[test]
    fn clamps_long_lines_around_the_match_with_display_offsets() {
        let text = format!("{}needle{}", "x".repeat(600), "y".repeat(600));
        let (snippet, display_column, display_match_length) = clamp_line_context(&text, 600, 6);
        assert_eq!(snippet.chars().count(), MAX_LINE_CONTENT_CHARS + 2);
        assert!(snippet.starts_with('…'));
        assert!(snippet.ends_with('…'));
        let display_column = display_column.unwrap();
        let display_match_length = display_match_length.unwrap();
        assert_eq!(display_match_length, 6);
        let sliced: String = snippet
            .chars()
            .skip(display_column - 1)
            .take(display_match_length)
            .collect();
        assert_eq!(sliced, "needle");
    }

    #[test]
    fn keeps_short_lines_verbatim_without_display_offsets() {
        let (snippet, display_column, display_match_length) =
            clamp_line_context("日本語needle", 9, 6);
        assert_eq!(snippet, "日本語needle");
        assert_eq!(display_column, None);
        assert_eq!(display_match_length, None);
    }

    #[test]
    fn truncates_on_a_character_window_for_multibyte_lines() {
        let text = format!("{}needle{}", "日".repeat(600), "本".repeat(600));
        let (snippet, display_column, display_match_length) = clamp_line_context(&text, 1800, 6);
        assert!(snippet.chars().count() <= MAX_LINE_CONTENT_CHARS + 2);
        let display_column = display_column.unwrap();
        let display_match_length = display_match_length.unwrap();
        let sliced: String = snippet
            .chars()
            .skip(display_column - 1)
            .take(display_match_length)
            .collect();
        assert_eq!(sliced, "needle");
    }

    #[test]
    fn search_stops_on_a_latched_cancellation_flag() {
        let root = TestDir::new("cancelled");
        fs::write(root.path().join("a.txt"), "needle\n").unwrap();
        let cancelled = AtomicBool::new(true);
        let result = search_resolved(
            root.path(),
            &options(root.path(), "needle"),
            Some(&cancelled),
            Duration::from_secs(60),
        )
        .unwrap();
        assert_eq!(result.total_matches, 0);
        assert!(result.files.is_empty());
        assert!(!result.truncated, "cancellation is not truncation");
    }

    #[test]
    fn search_marks_truncated_when_the_timeout_expires() {
        let root = TestDir::new("timeout");
        fs::write(root.path().join("a.txt"), "needle\n").unwrap();
        let result = search_resolved(
            root.path(),
            &options(root.path(), "needle"),
            None,
            Duration::ZERO,
        )
        .unwrap();
        assert_eq!(result.total_matches, 0);
        assert!(result.truncated);
    }

    #[test]
    fn starting_a_search_for_the_same_root_cancels_the_previous_run() {
        let root = TestDir::new("supersede");
        fs::write(root.path().join("a.txt"), "needle\n").unwrap();
        let service = FsService::new();
        service.authorize_root(root.path().to_str().unwrap()).unwrap();

        let resolved = fs::canonicalize(root.path()).unwrap();
        let token = resolved.to_string_lossy().into_owned();
        let previous = CancelGuard::new(service.cancel_registry(), &token);
        let result = service
            .search(
                options(&resolved, "needle"),
                service.cancel_registry(),
            )
            .unwrap();
        assert_eq!(result.total_matches, 1);
        assert!(
            previous.flag().load(Ordering::SeqCst),
            "a new search for the same root must abort the previous run"
        );
    }

    #[test]
    fn strips_only_the_line_feed_terminator() {
        assert_eq!(strip_line_terminator(b"abc\n"), b"abc");
        assert_eq!(strip_line_terminator(b"abc"), b"abc");
        assert_eq!(
            strip_line_terminator(b"abc\r\n"),
            b"abc\r",
            "CRLF keeps the carriage return (oracle parity)"
        );
        assert_eq!(strip_line_terminator(b"abc\n\n"), b"abc\n");
        assert_eq!(strip_line_terminator(b""), b"");
    }

    #[test]
    fn clamps_max_results_to_the_default_cap_and_minimum() {
        assert_eq!(clamp_max_results(None), MAX_RESULTS_DEFAULT);
        assert_eq!(clamp_max_results(Some(0)), 1);
        assert_eq!(clamp_max_results(Some(1)), 1);
        assert_eq!(clamp_max_results(Some(500)), 500);
        assert_eq!(clamp_max_results(Some(999_999)), MAX_RESULTS_CAP);
    }
}
