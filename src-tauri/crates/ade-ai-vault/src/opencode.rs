use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, UNIX_EPOCH};

use rusqlite::ffi::ErrorCode;
use rusqlite::types::ValueRef;
use rusqlite::{Connection, OpenFlags};
use serde_json::{json, Value};

use crate::claude::{
    effective_recency, extract_string, normalize_content_text, normalize_title_text, number_value,
    parse_timestamp_ms, quote_posix, system_time_to_iso, timestamp_iso, PREVIEW_TEXT_LIMIT,
};
use crate::ScanIssue;

const PREVIEW_MESSAGE_LIMIT: usize = 5;
const SQLITE_PREVIEW_MESSAGE_WINDOW: usize = 100;
const SQLITE_BUSY_TIMEOUT_MS: u64 = 1_500;
const MAX_TIMESTAMP_MILLIS: i64 = 1_000_000_000_000;

pub fn opencode_db_paths(data_dir: &Path, env_db: Option<&str>) -> Vec<PathBuf> {
    match env_db.map(str::trim).filter(|value| !value.is_empty()) {
        Some(":memory:") => Vec::new(),
        Some("-") => discover_opencode_databases(data_dir),
        Some(raw) => {
            let candidate = Path::new(raw);
            let path = if candidate.is_absolute() {
                candidate.to_path_buf()
            } else {
                data_dir.join(candidate)
            };
            if path.is_file() {
                vec![path]
            } else {
                Vec::new()
            }
        }
        None => discover_opencode_databases(data_dir),
    }
}

fn discover_opencode_databases(data_dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(data_dir) else {
        return Vec::new();
    };
    let mut names: Vec<String> = entries
        .filter_map(Result::ok)
        .filter(|entry| {
            entry
                .file_type()
                .map(|kind| kind.is_file())
                .unwrap_or(false)
        })
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| is_opencode_db_name(name))
        .collect();
    names.sort();
    let mut paths = Vec::new();
    if let Some(index) = names.iter().position(|name| name == "opencode.db") {
        let canonical = names.remove(index);
        paths.push(data_dir.join(canonical));
    }
    paths.extend(names.into_iter().map(|name| data_dir.join(name)));
    paths
}

fn is_opencode_db_name(name: &str) -> bool {
    let Some(rest) = name.strip_prefix("opencode") else {
        return false;
    };
    if rest == ".db" {
        return true;
    }
    let Some(rest) = rest.strip_prefix('-') else {
        return false;
    };
    let Some(stem) = rest.strip_suffix(".db") else {
        return false;
    };
    !stem.is_empty()
        && stem
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '.' | '-'))
}

pub fn scan_opencode_sessions(data_dir: &Path, limit: usize) -> (Vec<Value>, Vec<ScanIssue>) {
    let env_db = std::env::var("OPENCODE_DB").ok();
    scan_opencode_sessions_with_env(data_dir, limit, env_db.as_deref())
}

pub(crate) fn scan_opencode_sessions_with_env(
    data_dir: &Path,
    limit: usize,
    env_db: Option<&str>,
) -> (Vec<Value>, Vec<ScanIssue>) {
    let mut issues = Vec::new();
    let mut rows = scan_legacy_sessions(data_dir, &mut issues);
    sort_rows_by_recency(&mut rows);
    rows.truncate(limit);
    rows.extend(scan_sqlite_sessions(data_dir, env_db, limit, &mut issues));
    (rows, issues)
}

fn sort_rows_by_recency(rows: &mut [Value]) {
    rows.sort_by(|left, right| effective_recency(right).cmp(effective_recency(left)));
}

fn scan_legacy_sessions(data_dir: &Path, issues: &mut Vec<ScanIssue>) -> Vec<Value> {
    let storage_root = data_dir.join("storage");
    let session_root = storage_root.join("session");
    let mut files = Vec::new();
    collect_legacy_session_files(&session_root, &mut files, issues);
    files.sort();
    let mut rows = Vec::new();
    for path in files {
        match read_legacy_session_row(&path, &storage_root) {
            Ok(Some(row)) => rows.push(row),
            Ok(None) => {}
            Err(message) => issues.push(ScanIssue {
                agent: "opencode".to_string(),
                kind: "scope".to_string(),
                path: path.to_string_lossy().into_owned(),
                message,
            }),
        }
    }
    rows
}

fn collect_legacy_session_files(dir: &Path, files: &mut Vec<PathBuf>, issues: &mut Vec<ScanIssue>) {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) => {
            if error.kind() != std::io::ErrorKind::NotFound {
                issues.push(ScanIssue {
                    agent: "opencode".to_string(),
                    kind: "scope".to_string(),
                    path: dir.to_string_lossy().into_owned(),
                    message: error.to_string(),
                });
            }
            return;
        }
    };
    let mut entries: Vec<_> = entries.filter_map(Result::ok).collect();
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let path = entry.path();
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if file_type.is_dir() {
            collect_legacy_session_files(&path, files, issues);
        } else if file_type.is_file() && has_json_extension(&path) {
            files.push(path);
        }
    }
}

fn has_json_extension(path: &Path) -> bool {
    path.extension()
        .is_some_and(|extension| extension.to_string_lossy().eq_ignore_ascii_case("json"))
}

fn read_legacy_session_row(path: &Path, storage_root: &Path) -> Result<Option<Value>, String> {
    let metadata = fs::metadata(path).map_err(|error| error.to_string())?;
    let modified_at = metadata
        .modified()
        .map(system_time_to_iso)
        .map_err(|error| error.to_string())?;
    let bytes = fs::read(path).map_err(|error| error.to_string())?;
    let parsed: Value = serde_json::from_slice(&bytes).map_err(|error| error.to_string())?;
    let Some(record) = parsed.as_object() else {
        return Ok(None);
    };

    let session_id = extract_string(record.get("id"))
        .map(str::to_string)
        .unwrap_or_else(|| file_stem(path));
    let mut title = normalize_title_text(extract_string(record.get("title")).unwrap_or(""));
    let cwd = extract_string(record.get("directory"))
        .or_else(|| extract_string(record.get("cwd")))
        .map(str::to_string);

    let mut timeline = Timeline::default();
    timeline.update(time_field(record, "created"));
    timeline.update(time_field(record, "updated"));

    let mut message_count = 0u64;
    let mut total_tokens = 0u64;
    let mut model = None;
    let mut preview_messages = Vec::new();
    let mut preview_messages_truncated = false;

    for message in read_legacy_messages(&storage_root.join("message").join(&session_id)) {
        let Some(message) = message.as_object() else {
            continue;
        };
        let role = match extract_string(message.get("role")) {
            Some("user") => "user",
            Some("assistant") => "assistant",
            _ => continue,
        };
        message_count += 1;
        timeline.update(time_field(message, "created"));
        let summary = message.get("summary").and_then(Value::as_object);
        if role == "user" && title.is_none() {
            title = extract_string(summary.and_then(|summary| summary.get("title")))
                .or_else(|| extract_string(summary.and_then(|summary| summary.get("body"))))
                .map(str::to_string);
        }
        let summary_text = extract_string(summary.and_then(|summary| summary.get("body")))
            .or_else(|| extract_string(summary.and_then(|summary| summary.get("title"))));
        let text = normalize_content_text(message.get("content"), PREVIEW_TEXT_LIMIT)
            .or_else(|| summary_text.map(str::to_string));
        if let Some(text) = text {
            push_preview(
                &mut preview_messages,
                &mut preview_messages_truncated,
                role,
                text,
                timestamp_iso(time_field(message, "created")),
            );
        }
        let message_model = message
            .get("model")
            .and_then(Value::as_object)
            .and_then(|model| extract_string(model.get("modelID")))
            .or_else(|| extract_string(message.get("modelID")));
        if let Some(message_model) = message_model {
            model = Some(message_model.to_string());
        }
        total_tokens = total_tokens.saturating_add(opencode_token_total(message.get("tokens")));
    }

    let file_path = path.to_string_lossy().into_owned();
    let id = format!("local:opencode:{session_id}:{file_path}");
    let resume_command = opencode_resume_command(cwd.as_deref(), &session_id);
    let title = title.unwrap_or_else(|| generated_session_title(&session_id));

    let mut row = json!({
        "id": id,
        "executionHostId": "local",
        "agent": "opencode",
        "sessionId": session_id,
        "title": title,
        "cwd": cwd,
        "branch": Value::Null,
        "model": model,
        "filePath": file_path,
        "codexHome": Value::Null,
        "createdAt": timeline.created_iso(),
        "updatedAt": timeline.updated_iso(),
        "modifiedAt": modified_at,
        "messageCount": message_count,
        "totalTokens": total_tokens,
        "previewMessages": preview_messages,
        "queuedMessageCount": 0,
        "subagentTranscriptCount": 0,
        "resumeCommand": resume_command,
        "subagent": Value::Null,
    });
    if preview_messages_truncated {
        row["previewMessagesTruncated"] = Value::Bool(true);
    }
    Ok(Some(row))
}

fn read_legacy_messages(dir: &Path) -> Vec<Value> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut messages: Vec<(i64, String, Value)> = Vec::new();
    for entry in entries.filter_map(Result::ok) {
        let name = entry.file_name().to_string_lossy().into_owned();
        if !name.to_ascii_lowercase().ends_with(".json") {
            continue;
        }
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if !file_type.is_file() {
            continue;
        }
        let Ok(bytes) = fs::read(entry.path()) else {
            continue;
        };
        let Ok(parsed) = serde_json::from_slice::<Value>(&bytes) else {
            continue;
        };
        if !parsed.is_object() {
            continue;
        }
        let created = parsed
            .get("time")
            .and_then(Value::as_object)
            .and_then(|time| time.get("created"))
            .and_then(|value| timestamp_ms(Some(value)))
            .unwrap_or(i64::MAX);
        messages.push((created, name, parsed));
    }
    messages.sort_by(|left, right| left.0.cmp(&right.0).then_with(|| left.1.cmp(&right.1)));
    messages
        .into_iter()
        .map(|(_, _, message)| message)
        .collect()
}

fn file_stem(path: &Path) -> String {
    path.file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn time_field<'a>(record: &'a serde_json::Map<String, Value>, key: &str) -> Option<&'a Value> {
    record
        .get("time")
        .and_then(Value::as_object)
        .and_then(|time| time.get(key))
}

fn timestamp_ms(value: Option<&Value>) -> Option<i64> {
    match value? {
        Value::Number(number) => number.as_i64().filter(|millis| *millis > 0).map(|millis| {
            if millis > MAX_TIMESTAMP_MILLIS {
                millis
            } else {
                millis * 1000
            }
        }),
        Value::String(raw) => parse_timestamp_ms(raw.trim()),
        _ => None,
    }
}

#[derive(Default)]
struct Timeline {
    created_ms: Option<i64>,
    updated_ms: Option<i64>,
}

impl Timeline {
    fn update(&mut self, value: Option<&Value>) {
        if let Some(ms) = timestamp_ms(value) {
            self.update_ms(ms);
        }
    }

    fn update_ms(&mut self, ms: i64) {
        if ms <= 0 {
            return;
        }
        if self.created_ms.is_none_or(|current| ms < current) {
            self.created_ms = Some(ms);
        }
        if self.updated_ms.is_none_or(|current| ms >= current) {
            self.updated_ms = Some(ms);
        }
    }

    fn created_iso(&self) -> Option<String> {
        self.created_ms.map(ms_to_iso)
    }

    fn updated_iso(&self) -> Option<String> {
        self.updated_ms.map(ms_to_iso)
    }
}

fn ms_to_iso(millis: i64) -> String {
    system_time_to_iso(UNIX_EPOCH + Duration::from_millis(millis.max(0) as u64))
}

fn push_preview(
    preview_messages: &mut Vec<Value>,
    truncated: &mut bool,
    role: &str,
    text: String,
    timestamp: Option<String>,
) {
    preview_messages.push(json!({
        "role": role,
        "text": text,
        "timestamp": timestamp,
    }));
    if preview_messages.len() > PREVIEW_MESSAGE_LIMIT {
        preview_messages.remove(0);
        *truncated = true;
    }
}

const TOKEN_ALIASES: [&str; 18] = [
    "input",
    "inputTokens",
    "input_tokens",
    "output",
    "outputTokens",
    "output_tokens",
    "cacheRead",
    "cacheReadTokens",
    "cache_read_input_tokens",
    "cacheWrite",
    "cacheWriteTokens",
    "cache_creation_input_tokens",
    "cached",
    "cachedInputTokens",
    "cached_input_tokens",
    "reasoning",
    "reasoningOutputTokens",
    "reasoning_output_tokens",
];

fn opencode_token_total(value: Option<&Value>) -> u64 {
    let Some(usage) = value.and_then(Value::as_object) else {
        return 0;
    };
    if let Some(total) = ["total", "totalTokens", "total_tokens"]
        .into_iter()
        .map(|key| number_value(usage.get(key)))
        .find(|total| *total > 0)
    {
        return total;
    }
    TOKEN_ALIASES
        .into_iter()
        .map(|key| number_value(usage.get(key)))
        .sum()
}

fn opencode_resume_command(cwd: Option<&str>, session_id: &str) -> String {
    let resume = format!("opencode --session {}", quote_posix(session_id));
    match cwd.map(str::trim).filter(|cwd| !cwd.is_empty()) {
        Some(cwd) => format!("cd {} && {resume}", quote_posix(cwd)),
        None => resume,
    }
}

fn generated_session_title(session_id: &str) -> String {
    format!(
        "OpenCode {}",
        session_id.chars().take(8).collect::<String>()
    )
}

fn scan_sqlite_sessions(
    data_dir: &Path,
    env_db: Option<&str>,
    limit: usize,
    issues: &mut Vec<ScanIssue>,
) -> Vec<Value> {
    let mut entries: Vec<(String, i64, Value)> = Vec::new();
    let mut index: HashMap<String, usize> = HashMap::new();
    for db_path in opencode_db_paths(data_dir, env_db) {
        match read_database_sessions(&db_path, limit, issues) {
            Ok(sessions) => {
                for (session_id, recency, row) in sessions {
                    match index.get(&session_id).copied() {
                        Some(position) if entries[position].1 < recency => {
                            entries[position] = (session_id, recency, row);
                        }
                        Some(_) => {}
                        None => {
                            index.insert(session_id.clone(), entries.len());
                            entries.push((session_id, recency, row));
                        }
                    }
                }
            }
            Err(error) => issues.push(database_scan_issue(&db_path, &error)),
        }
    }
    entries.sort_by_key(|entry| std::cmp::Reverse(entry.1));
    entries.into_iter().map(|(_, _, row)| row).collect()
}

fn read_database_sessions(
    db_path: &Path,
    limit: usize,
    issues: &mut Vec<ScanIssue>,
) -> rusqlite::Result<Vec<(String, i64, Value)>> {
    let conn = open_opencode_db_readonly(db_path)?;
    let columns = probe_sqlite_session_columns(&conn)?;
    if !columns.readable {
        return Ok(Vec::new());
    }
    let db_path_string = db_path.to_string_lossy().into_owned();
    let mut sessions = Vec::new();
    for (session_id, time_created, time_updated) in list_sqlite_sessions(&conn, &columns, limit)? {
        match read_sqlite_session(
            &conn,
            &db_path_string,
            &session_id,
            time_created,
            time_updated,
            &columns,
        ) {
            Ok(row) => {
                let recency = if time_updated > 0 {
                    time_updated
                } else {
                    time_created
                };
                sessions.push((session_id, recency, row));
            }
            Err(error) => issues.push(database_scan_issue(db_path, &error)),
        }
    }
    Ok(sessions)
}

fn open_opencode_db_readonly(db_path: &Path) -> rusqlite::Result<Connection> {
    let conn = Connection::open_with_flags(db_path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    conn.busy_timeout(Duration::from_millis(SQLITE_BUSY_TIMEOUT_MS))?;
    conn.pragma_update(None, "query_only", "ON")?;
    Ok(conn)
}

#[derive(Default)]
struct SqliteSessionColumns {
    readable: bool,
    title: bool,
    directory: bool,
    model: bool,
    tokens_input: bool,
    tokens_output: bool,
    tokens_reasoning: bool,
    parent_id: bool,
    time_archived: bool,
    can_count_messages: bool,
    can_read_message_parts: bool,
}

fn probe_sqlite_session_columns(conn: &Connection) -> rusqlite::Result<SqliteSessionColumns> {
    let session = table_columns(conn, "session")?;
    let message = table_columns(conn, "message")?;
    let part = table_columns(conn, "part")?;
    let can_count_messages = message.contains("session_id") && message.contains("data");
    Ok(SqliteSessionColumns {
        readable: session.contains("time_created") && session.contains("time_updated"),
        title: session.contains("title"),
        directory: session.contains("directory"),
        model: session.contains("model"),
        tokens_input: session.contains("tokens_input"),
        tokens_output: session.contains("tokens_output"),
        tokens_reasoning: session.contains("tokens_reasoning"),
        parent_id: session.contains("parent_id"),
        time_archived: session.contains("time_archived"),
        can_count_messages,
        can_read_message_parts: can_count_messages
            && message.contains("id")
            && message.contains("time_created")
            && part.contains("message_id")
            && part.contains("time_created")
            && part.contains("data"),
    })
}

fn table_columns(conn: &Connection, table: &str) -> rusqlite::Result<HashSet<String>> {
    let mut statement = conn.prepare(&format!("PRAGMA table_info({table})"))?;
    let mut rows = statement.query([])?;
    let mut columns = HashSet::new();
    while let Some(row) = rows.next()? {
        if let Ok(name) = row.get::<_, String>(1) {
            columns.insert(name);
        }
    }
    Ok(columns)
}

fn list_sqlite_sessions(
    conn: &Connection,
    columns: &SqliteSessionColumns,
    limit: usize,
) -> rusqlite::Result<Vec<(String, i64, i64)>> {
    let mut sql = String::from("SELECT id, time_created, time_updated FROM session WHERE 1=1");
    if columns.parent_id {
        sql.push_str(" AND parent_id IS NULL");
    }
    if columns.time_archived {
        sql.push_str(" AND time_archived IS NULL");
    }
    sql.push_str(
        " ORDER BY CASE WHEN time_updated > 0 THEN time_updated ELSE time_created END DESC LIMIT ?1",
    );
    let mut statement = conn.prepare(&sql)?;
    let limit = i64::try_from(limit).unwrap_or(i64::MAX);
    let rows = statement.query_map([limit], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, i64>(1)?,
            row.get::<_, i64>(2)?,
        ))
    })?;
    rows.collect()
}

fn optional_session_column(present: bool, name: &str) -> String {
    if present {
        format!("s.{name}")
    } else {
        "NULL".to_string()
    }
}

fn read_sqlite_session(
    conn: &Connection,
    db_path: &str,
    session_id: &str,
    time_created: i64,
    time_updated: i64,
    columns: &SqliteSessionColumns,
) -> rusqlite::Result<Value> {
    let message_count_sql = if columns.can_count_messages {
        "(SELECT COUNT(*) FROM message m WHERE m.session_id = s.id AND json_extract(m.data, '$.role') IN ('user','assistant'))"
    } else {
        "0"
    };
    let sql = format!(
        "SELECT {}, {}, {}, COALESCE({}, 0), COALESCE({}, 0), COALESCE({}, 0), {} FROM session s WHERE s.id = ?1 LIMIT 1",
        optional_session_column(columns.title, "title"),
        optional_session_column(columns.directory, "directory"),
        optional_session_column(columns.model, "model"),
        optional_session_column(columns.tokens_input, "tokens_input"),
        optional_session_column(columns.tokens_output, "tokens_output"),
        optional_session_column(columns.tokens_reasoning, "tokens_reasoning"),
        message_count_sql,
    );
    let mut statement = conn.prepare(&sql)?;
    let mut rows = statement.query([session_id])?;
    let row = rows.next()?.ok_or(rusqlite::Error::QueryReturnedNoRows)?;

    let title: Option<String> = row.get(0)?;
    let directory: Option<String> = row.get(1)?;
    let model_json: Option<String> = row.get(2)?;
    let tokens_input: i64 = row.get(3)?;
    let tokens_output: i64 = row.get(4)?;
    let tokens_reasoning: i64 = row.get(5)?;
    let message_count: i64 = row.get(6)?;

    let mut timeline = Timeline::default();
    timeline.update_ms(time_created);
    timeline.update_ms(time_updated);
    let modified_ms = if time_updated > 0 {
        time_updated
    } else {
        time_created
    };
    let mut title = normalize_title_text(title.as_deref().unwrap_or(""));
    let mut preview_messages = Vec::new();
    let mut preview_messages_truncated = false;
    if columns.can_read_message_parts {
        read_sqlite_previews(
            conn,
            session_id,
            &mut title,
            &mut preview_messages,
            &mut preview_messages_truncated,
        )?;
    }
    let title = title.unwrap_or_else(|| generated_session_title(session_id));
    let total_tokens = tokens_input
        .max(0)
        .saturating_add(tokens_output.max(0))
        .saturating_add(tokens_reasoning.max(0)) as u64;
    let message_count = message_count.max(0) as u64;
    let resume_command = opencode_resume_command(directory.as_deref(), session_id);
    let id = format!("local:opencode:{session_id}:{db_path}");
    let model = extract_model_id(model_json.as_deref());

    let mut row = json!({
        "id": id,
        "executionHostId": "local",
        "agent": "opencode",
        "sessionId": session_id,
        "title": title,
        "cwd": directory,
        "branch": Value::Null,
        "model": model,
        "filePath": db_path,
        "codexHome": Value::Null,
        "createdAt": timeline.created_iso(),
        "updatedAt": timeline.updated_iso(),
        "modifiedAt": ms_to_iso(modified_ms),
        "messageCount": message_count,
        "totalTokens": total_tokens,
        "previewMessages": preview_messages,
        "queuedMessageCount": 0,
        "subagentTranscriptCount": 0,
        "resumeCommand": resume_command,
        "subagent": Value::Null,
    });
    if preview_messages_truncated {
        row["previewMessagesTruncated"] = Value::Bool(true);
    }
    Ok(row)
}

struct SqlitePreviewRow {
    role: String,
    part_data: String,
    part_time: i64,
    summary_title: Option<String>,
    summary_body: Option<String>,
}

fn read_sqlite_previews(
    conn: &Connection,
    session_id: &str,
    title: &mut Option<String>,
    preview_messages: &mut Vec<Value>,
    truncated: &mut bool,
) -> rusqlite::Result<()> {
    let sql = format!(
        "SELECT json_extract(m.data, '$.role') AS role,
                p.data AS part_data,
                p.time_created AS part_time,
                json_extract(m.data, '$.summary.title') AS summary_title,
                json_extract(m.data, '$.summary.body') AS summary_body
         FROM (SELECT id, data FROM message WHERE session_id = ?1 ORDER BY time_created DESC, id DESC LIMIT {SQLITE_PREVIEW_MESSAGE_WINDOW}) m
         JOIN part p ON p.message_id = m.id
         WHERE json_extract(m.data, '$.role') IN ('user','assistant')
           AND json_extract(p.data, '$.type') = 'text'
         ORDER BY p.time_created DESC
         LIMIT {}",
        PREVIEW_MESSAGE_LIMIT + 1
    );
    let mut statement = conn.prepare(&sql)?;
    let mut rows = statement.query([session_id])?;
    let mut probed = Vec::new();
    while let Some(row) = rows.next()? {
        let Some(part_data) = text_column(row, 1)? else {
            continue;
        };
        probed.push(SqlitePreviewRow {
            role: text_column(row, 0)?.unwrap_or_default(),
            part_data,
            part_time: row.get(2)?,
            summary_title: text_column(row, 3)?,
            summary_body: text_column(row, 4)?,
        });
    }
    *truncated = probed.len() > PREVIEW_MESSAGE_LIMIT;
    let window = &probed[..probed.len().min(PREVIEW_MESSAGE_LIMIT)];
    for preview in window.iter().rev() {
        let Some(text) = preview_part_text(&preview.part_data) else {
            continue;
        };
        let Some(text) = normalize_content_text(Some(&Value::String(text)), PREVIEW_TEXT_LIMIT)
        else {
            continue;
        };
        if preview.role == "user" && title.is_none() {
            *title = normalize_title_text(preview.summary_title.as_deref().unwrap_or(""))
                .or_else(|| normalize_title_text(preview.summary_body.as_deref().unwrap_or("")));
        }
        preview_messages.push(json!({
            "role": preview.role,
            "text": text,
            "timestamp": ms_to_iso(preview.part_time),
        }));
    }
    Ok(())
}

fn text_column(row: &rusqlite::Row<'_>, index: usize) -> rusqlite::Result<Option<String>> {
    match row.get_ref(index)? {
        ValueRef::Text(bytes) => Ok(Some(String::from_utf8_lossy(bytes).into_owned())),
        _ => Ok(None),
    }
}

fn preview_part_text(part_data: &str) -> Option<String> {
    let parsed: Value = serde_json::from_str(part_data).ok()?;
    let record = parsed.as_object()?;
    extract_string(record.get("text")).map(str::to_string)
}

fn extract_model_id(model_json: Option<&str>) -> Option<String> {
    let parsed: Value = serde_json::from_str(model_json?).ok()?;
    let record = parsed.as_object()?;
    extract_string(record.get("id"))
        .or_else(|| extract_string(record.get("modelID")))
        .map(str::to_string)
}

fn database_scan_issue(db_path: &Path, error: &rusqlite::Error) -> ScanIssue {
    let name = db_path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| db_path.to_string_lossy().into_owned());
    let message = match classify_sqlite_read_failure(error) {
        SqliteReadFailure::Contended => format!(
            "OpenCode is writing to {name} right now, so its history was skipped. It is read again on the next refresh."
        ),
        SqliteReadFailure::WalIndexUnavailable => format!(
            "OpenCode history in {name} could not be read. Its write-ahead log cannot be opened read-only on this filesystem. Exit OpenCode cleanly to flush the log."
        ),
        SqliteReadFailure::Unreadable => {
            format!("OpenCode history in {name} could not be read: {error}")
        }
    };
    ScanIssue {
        agent: "opencode".to_string(),
        kind: "scope".to_string(),
        path: db_path.to_string_lossy().into_owned(),
        message,
    }
}

enum SqliteReadFailure {
    Contended,
    WalIndexUnavailable,
    Unreadable,
}

fn classify_sqlite_read_failure(error: &rusqlite::Error) -> SqliteReadFailure {
    if let rusqlite::Error::SqliteFailure(inner, _) = error {
        let primary = inner.extended_code & 0xff;
        if primary == 5
            || primary == 6
            || matches!(
                inner.code,
                ErrorCode::DatabaseBusy | ErrorCode::DatabaseLocked
            )
        {
            return SqliteReadFailure::Contended;
        }
        if primary == 14 || matches!(inner.code, ErrorCode::CannotOpen) {
            return SqliteReadFailure::WalIndexUnavailable;
        }
    }
    let message = error.to_string().to_ascii_lowercase();
    if message.contains("database is locked") || message.contains("database is busy") {
        SqliteReadFailure::Contended
    } else {
        SqliteReadFailure::Unreadable
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::params;
    use std::sync::mpsc;
    use std::thread;
    use tempfile::TempDir;

    fn write_file(root: &Path, rel: &str, contents: &str) -> PathBuf {
        let path = root.join(rel);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(&path, contents).unwrap();
        path
    }

    fn set_mtime(path: &Path, millis: u64) {
        let file = fs::OpenOptions::new().write(true).open(path).unwrap();
        file.set_modified(UNIX_EPOCH + Duration::from_millis(millis))
            .unwrap();
    }

    fn scan(root: &TempDir, limit: usize) -> (Vec<Value>, Vec<ScanIssue>) {
        scan_opencode_sessions_with_env(root.path(), limit, None)
    }

    fn row_for<'a>(rows: &'a [Value], session_id: &str) -> &'a Value {
        rows.iter()
            .find(|row| row["sessionId"] == json!(session_id))
            .unwrap_or_else(|| panic!("no row for {session_id}"))
    }

    fn write_legacy_session(root: &Path, project: &str, name: &str, record: &Value) -> PathBuf {
        write_file(
            root,
            &format!("storage/session/{project}/{name}"),
            &record.to_string(),
        )
    }

    fn write_legacy_message(root: &Path, session_id: &str, name: &str, record: &Value) {
        write_file(
            root,
            &format!("storage/message/{session_id}/{name}"),
            &record.to_string(),
        );
    }

    fn legacy_user_message(text: &str, created: i64) -> Value {
        json!({
            "role": "user",
            "content": [{ "type": "text", "text": text }],
            "time": { "created": created },
            "tokens": { "input": 5, "output": 0 }
        })
    }

    fn apply_full_schema(conn: &Connection) {
        conn.execute_batch(
            "CREATE TABLE session (
                id TEXT PRIMARY KEY,
                project_id TEXT NOT NULL,
                parent_id TEXT,
                slug TEXT NOT NULL,
                directory TEXT NOT NULL,
                title TEXT NOT NULL,
                version TEXT NOT NULL,
                time_created INTEGER NOT NULL,
                time_updated INTEGER NOT NULL,
                time_archived INTEGER,
                model TEXT,
                agent TEXT,
                cost REAL DEFAULT 0 NOT NULL,
                tokens_input INTEGER DEFAULT 0 NOT NULL,
                tokens_output INTEGER DEFAULT 0 NOT NULL,
                tokens_reasoning INTEGER DEFAULT 0 NOT NULL,
                tokens_cache_read INTEGER DEFAULT 0 NOT NULL,
                tokens_cache_write INTEGER DEFAULT 0 NOT NULL,
                metadata TEXT
            );
            CREATE TABLE message (
                id TEXT PRIMARY KEY,
                session_id TEXT NOT NULL,
                time_created INTEGER NOT NULL,
                time_updated INTEGER NOT NULL,
                data TEXT NOT NULL
            );
            CREATE TABLE part (
                id TEXT PRIMARY KEY,
                message_id TEXT NOT NULL,
                session_id TEXT NOT NULL,
                time_created INTEGER NOT NULL,
                time_updated INTEGER NOT NULL,
                data TEXT NOT NULL
            );",
        )
        .unwrap();
    }

    fn apply_minimal_schema(conn: &Connection) {
        conn.execute_batch(
            "CREATE TABLE session (
                id TEXT PRIMARY KEY,
                time_created INTEGER NOT NULL,
                time_updated INTEGER NOT NULL
            );",
        )
        .unwrap();
    }

    fn insert_session(
        conn: &Connection,
        id: &str,
        title: &str,
        directory: &str,
        created: i64,
        updated: i64,
    ) {
        let model = json!({ "id": "glm-5.2", "providerID": "zai-coding-plan" }).to_string();
        conn.execute(
            "INSERT INTO session (id, project_id, slug, directory, title, version,
               time_created, time_updated, model, cost,
               tokens_input, tokens_output, tokens_reasoning, tokens_cache_read, tokens_cache_write)
             VALUES (?1, 'proj-1', 'slug', ?2, ?3, '1.0.0', ?4, ?5, ?6, 0.01, 100, 40, 10, 5, 0)",
            params![id, directory, title, created, updated, model],
        )
        .unwrap();
    }

    fn insert_message(
        conn: &Connection,
        id: &str,
        session_id: &str,
        role: &str,
        created: i64,
        summary_title: Option<&str>,
        summary_body: Option<&str>,
    ) {
        let mut data = json!({
            "role": role,
            "time": { "created": created },
            "agent": "build"
        });
        if summary_title.is_some() || summary_body.is_some() {
            data["summary"] = json!({
                "title": summary_title,
                "body": summary_body
            });
        }
        conn.execute(
            "INSERT INTO message (id, session_id, time_created, time_updated, data)
             VALUES (?1, ?2, ?3, ?3, ?4)",
            params![id, session_id, created, data.to_string()],
        )
        .unwrap();
    }

    fn insert_part(
        conn: &Connection,
        id: &str,
        message_id: &str,
        session_id: &str,
        created: i64,
        part_type: &str,
        text: &str,
    ) {
        let data = json!({ "type": part_type, "text": text });
        conn.execute(
            "INSERT INTO part (id, message_id, session_id, time_created, time_updated, data)
             VALUES (?1, ?2, ?3, ?4, ?4, ?5)",
            params![id, message_id, session_id, created, data.to_string()],
        )
        .unwrap();
    }

    #[test]
    fn opencode_db_paths_discovers_canonical_first_then_siblings() {
        let root = tempfile::tempdir().unwrap();
        write_file(root.path(), "opencode.db", "");
        write_file(root.path(), "opencode-alt.db", "");
        write_file(root.path(), "opencode-backup-2.db", "");
        write_file(root.path(), "other.db", "");
        write_file(root.path(), "opencode.txt", "");
        write_file(root.path(), "opencode-.db", "");
        write_file(root.path(), "opencode_v2.db", "");
        fs::create_dir_all(root.path().join("opencode-dir.db")).unwrap();

        let paths = opencode_db_paths(root.path(), None);

        assert_eq!(
            paths,
            vec![
                root.path().join("opencode.db"),
                root.path().join("opencode-alt.db"),
                root.path().join("opencode-backup-2.db"),
            ]
        );
    }

    #[test]
    fn opencode_db_paths_finds_siblings_when_canonical_is_missing() {
        let root = tempfile::tempdir().unwrap();
        write_file(root.path(), "opencode-stable.db", "");

        assert_eq!(
            opencode_db_paths(root.path(), None),
            vec![root.path().join("opencode-stable.db")]
        );
    }

    #[test]
    fn opencode_db_paths_returns_empty_for_a_missing_directory() {
        let root = tempfile::tempdir().unwrap();
        assert_eq!(
            opencode_db_paths(&root.path().join("absent"), None),
            Vec::<PathBuf>::new()
        );
    }

    #[test]
    fn opencode_db_paths_honors_absolute_env_override() {
        let root = tempfile::tempdir().unwrap();
        write_file(root.path(), "opencode.db", "");
        let override_path = write_file(root.path(), "elsewhere/custom.db", "");

        assert_eq!(
            opencode_db_paths(root.path(), Some(override_path.to_str().unwrap())),
            vec![override_path.clone()]
        );
        assert_eq!(
            opencode_db_paths(
                root.path(),
                Some(root.path().join("missing/absent.db").to_str().unwrap())
            ),
            Vec::<PathBuf>::new()
        );
    }

    #[test]
    fn opencode_db_paths_joins_relative_env_override_to_data_dir() {
        let root = tempfile::tempdir().unwrap();
        write_file(root.path(), "opencode.db", "");
        let override_path = write_file(root.path(), "custom.db", "");

        assert_eq!(
            opencode_db_paths(root.path(), Some("  custom.db  ")),
            vec![override_path]
        );
        assert_eq!(
            opencode_db_paths(root.path(), Some("missing.db")),
            Vec::<PathBuf>::new()
        );
    }

    #[test]
    fn opencode_db_paths_ignores_dash_and_memory_overrides() {
        let root = tempfile::tempdir().unwrap();
        write_file(root.path(), "opencode.db", "");

        let canonical = vec![root.path().join("opencode.db")];
        assert_eq!(opencode_db_paths(root.path(), Some("-")), canonical);
        assert_eq!(opencode_db_paths(root.path(), Some("   ")), canonical);
        assert_eq!(
            opencode_db_paths(root.path(), Some(":memory:")),
            Vec::<PathBuf>::new()
        );
    }

    #[test]
    fn parses_legacy_session_and_messages_into_a_row() {
        let root = tempfile::tempdir().unwrap();
        let session_path = write_legacy_session(
            root.path(),
            "project",
            "ses_legacy.json",
            &json!({
                "id": "ses_legacy",
                "title": "Legacy title",
                "directory": "/tmp/opencode",
                "time": { "created": 1_777_634_000_000i64, "updated": 1_777_634_001_000i64 }
            }),
        );
        set_mtime(&session_path, 1_700_000_000_123);
        write_legacy_message(
            root.path(),
            "ses_legacy",
            "msg_1.json",
            &legacy_user_message("First prompt", 1_777_634_000_500),
        );
        write_legacy_message(
            root.path(),
            "ses_legacy",
            "msg_2.json",
            &json!({
                "role": "assistant",
                "content": [{ "type": "text", "text": "Reply" }],
                "time": { "created": 1_777_634_002_000i64 },
                "tokens": { "input": 0, "output": 5 }
            }),
        );
        write_file(
            root.path(),
            "storage/message/ses_legacy/msg_bad.json",
            "{\"role\":\"user\",\"content\":[{\"type\":",
        );

        let (rows, issues) = scan(&root, 100);

        assert!(issues.is_empty());
        assert_eq!(rows.len(), 1);
        let row = &rows[0];
        assert_eq!(
            row["id"],
            json!(format!(
                "local:opencode:ses_legacy:{}",
                session_path.display()
            ))
        );
        assert_eq!(row["executionHostId"], json!("local"));
        assert_eq!(row["agent"], json!("opencode"));
        assert_eq!(row["sessionId"], json!("ses_legacy"));
        assert_eq!(row["title"], json!("Legacy title"));
        assert_eq!(row["cwd"], json!("/tmp/opencode"));
        assert_eq!(row["branch"], Value::Null);
        assert_eq!(row["model"], Value::Null);
        assert_eq!(row["filePath"], json!(session_path.to_string_lossy()));
        assert_eq!(row["codexHome"], Value::Null);
        assert_eq!(row["createdAt"], json!("2026-05-01T11:13:20.000Z"));
        assert_eq!(row["updatedAt"], json!("2026-05-01T11:13:22.000Z"));
        assert_eq!(row["modifiedAt"], json!("2023-11-14T22:13:20.123Z"));
        assert_eq!(row["messageCount"], json!(2));
        assert_eq!(row["totalTokens"], json!(10));
        assert_eq!(row["queuedMessageCount"], json!(0));
        assert_eq!(row["subagentTranscriptCount"], json!(0));
        assert_eq!(row["subagent"], Value::Null);
        assert_eq!(
            row["resumeCommand"],
            json!("cd '/tmp/opencode' && opencode --session 'ses_legacy'")
        );
        assert_eq!(
            row["previewMessages"],
            json!([
                {
                    "role": "user",
                    "text": "First prompt",
                    "timestamp": "2026-05-01T11:13:20.500Z"
                },
                {
                    "role": "assistant",
                    "text": "Reply",
                    "timestamp": "2026-05-01T11:13:22.000Z"
                }
            ])
        );
        assert!(row.get("previewMessagesTruncated").is_none());
    }

    #[test]
    fn legacy_session_id_falls_back_to_the_file_stem() {
        let root = tempfile::tempdir().unwrap();
        write_legacy_session(
            root.path(),
            "project",
            "plain-session.json",
            &json!({
                "title": "No id",
                "time": { "created": 1_777_634_000_000i64 }
            }),
        );

        let (rows, issues) = scan(&root, 100);

        assert!(issues.is_empty());
        let row = row_for(&rows, "plain-session");
        assert_eq!(row["title"], json!("No id"));
        assert_eq!(row["cwd"], Value::Null);
        assert_eq!(
            row["resumeCommand"],
            json!("opencode --session 'plain-session'")
        );
    }

    #[test]
    fn legacy_messages_sort_by_created_time_not_file_order() {
        let root = tempfile::tempdir().unwrap();
        write_legacy_session(
            root.path(),
            "project",
            "ses_order.json",
            &json!({ "id": "ses_order", "directory": "/tmp/opencode" }),
        );
        write_legacy_message(
            root.path(),
            "ses_order",
            "msg_a.json",
            &legacy_user_message("Later turn", 1_777_634_002_000),
        );
        write_legacy_message(
            root.path(),
            "ses_order",
            "msg_b.json",
            &legacy_user_message("Earliest turn", 1_777_634_000_000),
        );

        let (rows, issues) = scan(&root, 100);

        assert!(issues.is_empty());
        let row = row_for(&rows, "ses_order");
        let texts: Vec<&str> = row["previewMessages"]
            .as_array()
            .unwrap()
            .iter()
            .map(|message| message["text"].as_str().unwrap())
            .collect();
        assert_eq!(texts, vec!["Earliest turn", "Later turn"]);
    }

    #[test]
    fn legacy_title_falls_back_to_the_user_summary() {
        let root = tempfile::tempdir().unwrap();
        write_legacy_session(root.path(), "project", "ses_title.json", &json!({}));
        write_legacy_message(
            root.path(),
            "ses_title",
            "msg_1.json",
            &json!({
                "role": "user",
                "content": [{ "type": "text", "text": "First prompt" }],
                "summary": { "title": "Summary title" },
                "time": { "created": 1_777_634_000_000i64 }
            }),
        );
        write_legacy_session(root.path(), "project", "ses_body.json", &json!({}));
        write_legacy_message(
            root.path(),
            "ses_body",
            "msg_1.json",
            &json!({
                "role": "user",
                "content": [{ "type": "text", "text": "Another prompt" }],
                "summary": { "body": "Summary body" },
                "time": { "created": 1_777_634_000_000i64 }
            }),
        );

        let (rows, issues) = scan(&root, 100);

        assert!(issues.is_empty());
        assert_eq!(row_for(&rows, "ses_title")["title"], json!("Summary title"));
        assert_eq!(row_for(&rows, "ses_body")["title"], json!("Summary body"));
    }

    #[test]
    fn legacy_preview_window_keeps_the_newest_five_and_sets_truncated() {
        let root = tempfile::tempdir().unwrap();
        write_legacy_session(root.path(), "project", "ses_window.json", &json!({}));
        for index in 0..7 {
            write_legacy_message(
                root.path(),
                "ses_window",
                &format!("msg_{index}.json"),
                &legacy_user_message(
                    &format!("message {index}"),
                    1_777_634_000_000 + index as i64 * 1_000,
                ),
            );
        }

        let (rows, issues) = scan(&root, 100);

        assert!(issues.is_empty());
        let row = row_for(&rows, "ses_window");
        let texts: Vec<&str> = row["previewMessages"]
            .as_array()
            .unwrap()
            .iter()
            .map(|message| message["text"].as_str().unwrap())
            .collect();
        assert_eq!(
            texts,
            vec![
                "message 2",
                "message 3",
                "message 4",
                "message 5",
                "message 6"
            ]
        );
        assert_eq!(row["previewMessagesTruncated"], json!(true));
    }

    #[test]
    fn sqlite_lists_and_parses_sessions_sorted_by_recency() {
        let root = tempfile::tempdir().unwrap();
        let db_path = root.path().join("opencode.db");
        let conn = Connection::open(&db_path).unwrap();
        apply_full_schema(&conn);
        insert_session(
            &conn,
            "ses_old",
            "Old",
            "/tmp/old",
            1_777_634_000_000,
            1_777_634_001_000,
        );
        insert_session(
            &conn,
            "ses_new",
            "OpenCode title",
            "/tmp/opencode",
            1_777_634_002_000,
            1_777_634_003_000,
        );
        insert_message(
            &conn,
            "msg_1",
            "ses_new",
            "user",
            1_777_634_002_500,
            Some("OpenCode title"),
            None,
        );
        insert_part(
            &conn,
            "prt_1",
            "msg_1",
            "ses_new",
            1_777_634_002_500,
            "text",
            "Plan the work",
        );
        insert_message(
            &conn,
            "msg_2",
            "ses_new",
            "assistant",
            1_777_634_002_900,
            None,
            None,
        );
        insert_part(
            &conn,
            "prt_2",
            "msg_2",
            "ses_new",
            1_777_634_003_000,
            "text",
            "Done",
        );
        drop(conn);

        let (rows, issues) = scan(&root, 10);

        assert!(issues.is_empty());
        assert_eq!(rows.len(), 2);
        let row = row_for(&rows, "ses_new");
        assert_eq!(
            row["id"],
            json!(format!("local:opencode:ses_new:{}", db_path.display()))
        );
        assert_eq!(row["agent"], json!("opencode"));
        assert_eq!(row["title"], json!("OpenCode title"));
        assert_eq!(row["cwd"], json!("/tmp/opencode"));
        assert_eq!(row["model"], json!("glm-5.2"));
        assert_eq!(row["filePath"], json!(db_path.to_string_lossy()));
        assert_eq!(row["createdAt"], json!("2026-05-01T11:13:22.000Z"));
        assert_eq!(row["updatedAt"], json!("2026-05-01T11:13:23.000Z"));
        assert_eq!(row["modifiedAt"], json!("2026-05-01T11:13:23.000Z"));
        assert_eq!(row["messageCount"], json!(2));
        assert_eq!(row["totalTokens"], json!(150));
        assert_eq!(
            row["resumeCommand"],
            json!("cd '/tmp/opencode' && opencode --session 'ses_new'")
        );
        assert_eq!(
            row["previewMessages"],
            json!([
                {
                    "role": "user",
                    "text": "Plan the work",
                    "timestamp": "2026-05-01T11:13:22.500Z"
                },
                {
                    "role": "assistant",
                    "text": "Done",
                    "timestamp": "2026-05-01T11:13:23.000Z"
                }
            ])
        );
        assert!(row.get("previewMessagesTruncated").is_none());
        assert_eq!(row_for(&rows, "ses_old")["title"], json!("Old"));
    }

    #[test]
    fn sqlite_model_is_stored_as_id_and_provider_json() {
        let root = tempfile::tempdir().unwrap();
        let db_path = root.path().join("opencode.db");
        let conn = Connection::open(&db_path).unwrap();
        apply_full_schema(&conn);
        insert_session(
            &conn,
            "ses_model",
            "Model",
            "/tmp/opencode",
            1_777_634_000_000,
            1_777_634_001_000,
        );
        drop(conn);

        let (rows, issues) = scan(&root, 10);

        assert!(issues.is_empty());
        assert_eq!(row_for(&rows, "ses_model")["model"], json!("glm-5.2"));
    }

    #[test]
    fn sqlite_excludes_child_and_archived_sessions() {
        let root = tempfile::tempdir().unwrap();
        let db_path = root.path().join("opencode.db");
        let conn = Connection::open(&db_path).unwrap();
        apply_full_schema(&conn);
        insert_session(
            &conn,
            "ses_normal",
            "Normal",
            "/tmp/opencode",
            1_777_634_000_000,
            1_777_634_001_000,
        );
        insert_session(
            &conn,
            "ses_archived",
            "Archived",
            "/tmp/opencode",
            1_777_634_000_000,
            1_777_634_002_000,
        );
        conn.execute(
            "UPDATE session SET time_archived = 1_777_634_002_500 WHERE id = 'ses_archived'",
            [],
        )
        .unwrap();
        insert_session(
            &conn,
            "ses_child",
            "Child",
            "/tmp/opencode",
            1_777_634_000_000,
            1_777_634_003_000,
        );
        conn.execute(
            "UPDATE session SET parent_id = 'ses_normal' WHERE id = 'ses_child'",
            [],
        )
        .unwrap();
        drop(conn);

        let (rows, issues) = scan(&root, 10);

        assert!(issues.is_empty());
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0]["sessionId"], json!("ses_normal"));
    }

    #[test]
    fn sqlite_minimal_schema_parses_without_optional_columns() {
        let root = tempfile::tempdir().unwrap();
        let db_path = root.path().join("opencode.db");
        let conn = Connection::open(&db_path).unwrap();
        apply_minimal_schema(&conn);
        conn.execute(
            "INSERT INTO session VALUES ('ses_minimal', 1_777_634_000_000, 1_777_634_001_000)",
            [],
        )
        .unwrap();
        drop(conn);

        let (rows, issues) = scan(&root, 10);

        assert!(issues.is_empty());
        assert_eq!(rows.len(), 1);
        let row = &rows[0];
        assert_eq!(row["sessionId"], json!("ses_minimal"));
        assert_eq!(row["filePath"], json!(db_path.to_string_lossy()));
        assert_eq!(row["title"], json!("OpenCode ses_mini"));
        assert_eq!(row["cwd"], Value::Null);
        assert_eq!(row["model"], Value::Null);
        assert_eq!(row["createdAt"], json!("2026-05-01T11:13:20.000Z"));
        assert_eq!(row["updatedAt"], json!("2026-05-01T11:13:21.000Z"));
        assert_eq!(row["messageCount"], json!(0));
        assert_eq!(row["totalTokens"], json!(0));
        assert_eq!(row["previewMessages"], json!([]));
        assert!(row.get("previewMessagesTruncated").is_none());
        assert_eq!(
            row["resumeCommand"],
            json!("opencode --session 'ses_minimal'")
        );
    }

    #[test]
    fn sqlite_partial_schemas_degrade_without_errors() {
        let root = tempfile::tempdir().unwrap();
        let db_path = root.path().join("opencode-message.db");
        let conn = Connection::open(&db_path).unwrap();
        apply_minimal_schema(&conn);
        conn.execute_batch(
            "CREATE TABLE message (
                id TEXT PRIMARY KEY,
                session_id TEXT NOT NULL,
                time_created INTEGER NOT NULL,
                data TEXT NOT NULL
            );",
        )
        .unwrap();
        conn.execute(
            "INSERT INTO session VALUES ('ses_partial', 1_777_634_000_000, 1_777_634_001_000)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO message (id, session_id, time_created, data) VALUES (?1, ?2, ?3, ?4)",
            params![
                "msg_1",
                "ses_partial",
                1_777_634_000_500i64,
                json!({ "role": "user" }).to_string()
            ],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO message (id, session_id, time_created, data) VALUES (?1, ?2, ?3, ?4)",
            params![
                "msg_2",
                "ses_partial",
                1_777_634_000_600i64,
                json!({ "role": "tool" }).to_string()
            ],
        )
        .unwrap();
        drop(conn);

        let db_path = root.path().join("opencode-nodata.db");
        let conn = Connection::open(&db_path).unwrap();
        apply_minimal_schema(&conn);
        conn.execute_batch(
            "CREATE TABLE message (
                id TEXT PRIMARY KEY,
                session_id TEXT NOT NULL,
                time_created INTEGER NOT NULL
            );",
        )
        .unwrap();
        conn.execute(
            "INSERT INTO session VALUES ('ses_nodata', 1_777_634_000_000, 1_777_634_001_000)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO message VALUES ('msg_1', 'ses_nodata', 1_777_634_000_500)",
            [],
        )
        .unwrap();
        drop(conn);

        let (rows, issues) = scan(&root, 10);

        assert!(issues.is_empty());
        assert_eq!(rows.len(), 2);
        let partial = row_for(&rows, "ses_partial");
        assert_eq!(partial["messageCount"], json!(1));
        assert_eq!(partial["previewMessages"], json!([]));
        assert!(partial.get("previewMessagesTruncated").is_none());
        let nodata = row_for(&rows, "ses_nodata");
        assert_eq!(nodata["messageCount"], json!(0));
        assert_eq!(nodata["previewMessages"], json!([]));
    }

    #[test]
    fn sqlite_preview_window_keeps_the_newest_five_and_sets_truncated() {
        let root = tempfile::tempdir().unwrap();
        let db_path = root.path().join("opencode.db");
        let conn = Connection::open(&db_path).unwrap();
        apply_full_schema(&conn);
        insert_session(
            &conn,
            "ses_window",
            "Window",
            "/tmp/opencode",
            1_777_634_000_000,
            1_777_634_100_000,
        );
        for index in 0..6 {
            let created = 1_777_634_000_000 + index * 1_000;
            insert_message(
                &conn,
                &format!("msg_{index}"),
                "ses_window",
                "user",
                created,
                None,
                None,
            );
            insert_part(
                &conn,
                &format!("prt_{index}"),
                &format!("msg_{index}"),
                "ses_window",
                created,
                "text",
                &format!("message {index}"),
            );
        }
        drop(conn);

        let (rows, issues) = scan(&root, 10);

        assert!(issues.is_empty());
        let row = row_for(&rows, "ses_window");
        let texts: Vec<&str> = row["previewMessages"]
            .as_array()
            .unwrap()
            .iter()
            .map(|message| message["text"].as_str().unwrap())
            .collect();
        assert_eq!(
            texts,
            vec![
                "message 1",
                "message 2",
                "message 3",
                "message 4",
                "message 5"
            ]
        );
        assert_eq!(row["previewMessagesTruncated"], json!(true));
    }

    #[test]
    fn sqlite_preview_only_reads_the_newest_message_window() {
        let root = tempfile::tempdir().unwrap();
        let db_path = root.path().join("opencode.db");
        let conn = Connection::open(&db_path).unwrap();
        apply_full_schema(&conn);
        insert_session(
            &conn,
            "ses_big",
            "Big",
            "/tmp/opencode",
            1_777_634_000_000,
            1_777_634_900_000,
        );
        for index in 0..3 {
            let created = 1_777_634_000_000 + index * 100;
            insert_message(
                &conn,
                &format!("old_{index}"),
                "ses_big",
                "user",
                created,
                None,
                None,
            );
            insert_part(
                &conn,
                &format!("prt_old_{index}"),
                &format!("old_{index}"),
                "ses_big",
                created,
                "text",
                &format!("OLD_USER_TEXT_{index}"),
            );
        }
        for index in 0..100 {
            let created = 1_777_634_010_000 + index * 100;
            insert_message(
                &conn,
                &format!("new_{index}"),
                "ses_big",
                "assistant",
                created,
                None,
                None,
            );
            insert_part(
                &conn,
                &format!("prt_new_{index}"),
                &format!("new_{index}"),
                "ses_big",
                created,
                "tool",
                &format!("tool output {index}"),
            );
        }
        drop(conn);

        let (rows, issues) = scan(&root, 10);

        assert!(issues.is_empty());
        let row = row_for(&rows, "ses_big");
        assert_eq!(row["messageCount"], json!(103));
        assert_eq!(row["previewMessages"], json!([]));
    }

    #[test]
    fn sqlite_counts_only_user_and_assistant_messages() {
        let root = tempfile::tempdir().unwrap();
        let db_path = root.path().join("opencode.db");
        let conn = Connection::open(&db_path).unwrap();
        apply_full_schema(&conn);
        insert_session(
            &conn,
            "ses_roles",
            "Roles",
            "/tmp/opencode",
            1_777_634_000_000,
            1_777_634_100_000,
        );
        insert_message(
            &conn,
            "msg_user",
            "ses_roles",
            "user",
            1_777_634_000_100,
            None,
            None,
        );
        insert_message(
            &conn,
            "msg_tool",
            "ses_roles",
            "tool",
            1_777_634_000_200,
            None,
            None,
        );
        insert_message(
            &conn,
            "msg_assistant",
            "ses_roles",
            "assistant",
            1_777_634_000_300,
            None,
            None,
        );
        drop(conn);

        let (rows, issues) = scan(&root, 10);

        assert!(issues.is_empty());
        assert_eq!(row_for(&rows, "ses_roles")["messageCount"], json!(2));
    }

    #[test]
    fn sqlite_title_falls_back_to_the_user_summary_body() {
        let root = tempfile::tempdir().unwrap();
        let db_path = root.path().join("opencode.db");
        let conn = Connection::open(&db_path).unwrap();
        apply_full_schema(&conn);
        insert_session(
            &conn,
            "ses_summary",
            "",
            "/tmp/opencode",
            1_777_634_000_000,
            1_777_634_001_000,
        );
        insert_message(
            &conn,
            "msg_1",
            "ses_summary",
            "user",
            1_777_634_000_500,
            None,
            Some("fallback title from summary"),
        );
        insert_part(
            &conn,
            "prt_1",
            "msg_1",
            "ses_summary",
            1_777_634_000_500,
            "text",
            "hello",
        );
        drop(conn);

        let (rows, issues) = scan(&root, 10);

        assert!(issues.is_empty());
        assert_eq!(
            row_for(&rows, "ses_summary")["title"],
            json!("fallback title from summary")
        );
    }

    #[test]
    fn sqlite_model_from_the_older_model_id_schema() {
        let root = tempfile::tempdir().unwrap();
        let db_path = root.path().join("opencode.db");
        let conn = Connection::open(&db_path).unwrap();
        apply_full_schema(&conn);
        insert_session(
            &conn,
            "ses_model",
            "Model",
            "/tmp/opencode",
            1_777_634_000_000,
            1_777_634_001_000,
        );
        conn.execute(
            "UPDATE session SET model = ?1 WHERE id = 'ses_model'",
            params![json!({ "modelID": "claude-sonnet-4-5" }).to_string()],
        )
        .unwrap();
        drop(conn);

        let (rows, issues) = scan(&root, 10);

        assert!(issues.is_empty());
        assert_eq!(
            row_for(&rows, "ses_model")["model"],
            json!("claude-sonnet-4-5")
        );
    }

    #[test]
    fn sqlite_schema_without_time_columns_is_skipped_silently() {
        let root = tempfile::tempdir().unwrap();
        let db_path = root.path().join("opencode.db");
        let conn = Connection::open(&db_path).unwrap();
        conn.execute_batch(
            "CREATE TABLE session (
                id TEXT PRIMARY KEY,
                time_created INTEGER,
                title TEXT
            );",
        )
        .unwrap();
        conn.execute(
            "INSERT INTO session (id, time_created, title) VALUES ('ses_odd', 1, 'Odd')",
            [],
        )
        .unwrap();
        drop(conn);

        let (rows, issues) = scan(&root, 10);

        assert!(rows.is_empty());
        assert!(issues.is_empty());
    }

    #[test]
    fn sqlite_list_limit_applies_to_the_newest_sessions() {
        let root = tempfile::tempdir().unwrap();
        let db_path = root.path().join("opencode.db");
        let conn = Connection::open(&db_path).unwrap();
        apply_minimal_schema(&conn);
        for index in 0..5 {
            conn.execute(
                "INSERT INTO session VALUES (?1, ?2, ?3)",
                params![
                    format!("ses_{index}"),
                    1_777_634_000_000i64,
                    1_777_634_000_000i64 + index * 1_000
                ],
            )
            .unwrap();
        }
        drop(conn);

        let (rows, issues) = scan(&root, 2);

        assert!(issues.is_empty());
        assert_eq!(rows.len(), 2);
        let ids: Vec<&str> = rows
            .iter()
            .map(|row| row["sessionId"].as_str().unwrap())
            .collect();
        assert_eq!(ids, vec!["ses_4", "ses_3"]);
    }

    #[test]
    fn readonly_open_sets_query_only_and_blocks_writes() {
        let root = tempfile::tempdir().unwrap();
        let db_path = root.path().join("opencode.db");
        let conn = Connection::open(&db_path).unwrap();
        apply_minimal_schema(&conn);
        drop(conn);

        let readonly = open_opencode_db_readonly(&db_path).unwrap();
        assert!(readonly.is_readonly(rusqlite::DatabaseName::Main).unwrap());
        let query_only: i64 = readonly
            .query_row("PRAGMA query_only", [], |row| row.get(0))
            .unwrap();
        assert_eq!(query_only, 1);
        assert!(readonly
            .execute("CREATE TABLE forbidden (id TEXT)", [])
            .is_err());
        assert!(readonly
            .execute("INSERT INTO session VALUES ('nope', 1, 1)", [],)
            .is_err());
    }

    #[test]
    fn sqlite_locked_database_reports_scope_issue_and_other_databases_still_parse() {
        let root = tempfile::tempdir().unwrap();
        let locked_path = root.path().join("opencode.db");
        let conn = Connection::open(&locked_path).unwrap();
        conn.execute_batch("PRAGMA journal_mode=DELETE;").unwrap();
        apply_minimal_schema(&conn);
        conn.execute(
            "INSERT INTO session VALUES ('ses_locked', 1_777_634_000_000, 1_777_634_001_000)",
            [],
        )
        .unwrap();

        let healthy_path = root.path().join("opencode-alt.db");
        let healthy = Connection::open(&healthy_path).unwrap();
        apply_minimal_schema(&healthy);
        healthy
            .execute(
                "INSERT INTO session VALUES ('ses_healthy', 1_777_634_000_000, 1_777_634_002_000)",
                [],
            )
            .unwrap();
        drop(healthy);

        conn.execute_batch("BEGIN EXCLUSIVE;").unwrap();
        conn.execute("INSERT INTO session VALUES ('ses_writer', 1, 1)", [])
            .unwrap();

        let started = std::time::Instant::now();
        let (rows, issues) = scan(&root, 10);
        let elapsed = started.elapsed();

        conn.execute_batch("ROLLBACK;").unwrap();

        assert!(elapsed >= Duration::from_millis(1_000));
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0]["sessionId"], json!("ses_healthy"));
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].agent, "opencode");
        assert_eq!(issues[0].kind, "scope");
        assert_eq!(issues[0].path, locked_path.to_string_lossy());
        assert!(issues[0]
            .message
            .contains("OpenCode is writing to opencode.db"));
        assert_ne!(issues[0].message, "database is locked");
    }

    #[test]
    fn sqlite_writer_release_inside_busy_timeout_still_reads() {
        let root = tempfile::tempdir().unwrap();
        let db_path = root.path().join("opencode.db");
        let conn = Connection::open(&db_path).unwrap();
        conn.execute_batch("PRAGMA journal_mode=DELETE;").unwrap();
        apply_minimal_schema(&conn);
        conn.execute(
            "INSERT INTO session VALUES ('ses_slow', 1_777_634_000_000, 1_777_634_001_000)",
            [],
        )
        .unwrap();
        drop(conn);

        let (locked_sender, locked_receiver) = mpsc::channel();
        let writer_path = db_path.clone();
        let writer = thread::spawn(move || {
            let conn = Connection::open(&writer_path).unwrap();
            conn.execute_batch("BEGIN EXCLUSIVE;").unwrap();
            conn.execute("INSERT INTO session VALUES ('ses_writer', 2, 2)", [])
                .unwrap();
            locked_sender.send(()).unwrap();
            thread::sleep(Duration::from_millis(600));
            conn.execute_batch("ROLLBACK;").unwrap();
        });
        locked_receiver.recv().unwrap();

        let (rows, issues) = scan(&root, 10);
        writer.join().unwrap();

        assert!(issues.is_empty());
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0]["sessionId"], json!("ses_slow"));
    }

    #[test]
    fn sqlite_unreadable_database_reports_write_ahead_advice() {
        use std::os::unix::fs::PermissionsExt;

        let root = tempfile::tempdir().unwrap();
        let db_path = root.path().join("opencode.db");
        let conn = Connection::open(&db_path).unwrap();
        apply_minimal_schema(&conn);
        conn.execute(
            "INSERT INTO session VALUES ('ses_unreadable', 1_777_634_000_000, 1_777_634_001_000)",
            [],
        )
        .unwrap();
        drop(conn);
        fs::set_permissions(&db_path, fs::Permissions::from_mode(0o000)).unwrap();

        let (rows, issues) = scan(&root, 10);

        fs::set_permissions(&db_path, fs::Permissions::from_mode(0o644)).unwrap();

        assert!(rows.is_empty());
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].kind, "scope");
        assert_eq!(issues[0].path, db_path.to_string_lossy());
        assert!(issues[0].message.contains("write-ahead log"));
    }

    #[test]
    fn database_scan_issue_maps_sqlite_failures() {
        let busy = rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error {
                code: ErrorCode::DatabaseBusy,
                extended_code: 5,
            },
            Some("database is locked".to_string()),
        );
        let issue = database_scan_issue(Path::new("/data/opencode.db"), &busy);
        assert_eq!(issue.kind, "scope");
        assert!(issue.message.contains("OpenCode is writing to opencode.db"));

        let cant_open = rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error {
                code: ErrorCode::CannotOpen,
                extended_code: 14,
            },
            Some("unable to open database file".to_string()),
        );
        let issue = database_scan_issue(Path::new("/data/opencode.db"), &cant_open);
        assert!(issue.message.contains("write-ahead log"));

        let corrupt = rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error {
                code: ErrorCode::DatabaseCorrupt,
                extended_code: 11,
            },
            Some("database disk image is malformed".to_string()),
        );
        let issue = database_scan_issue(Path::new("/data/opencode.db"), &corrupt);
        assert!(issue.message.contains("database disk image is malformed"));
        assert!(!issue.message.contains("write-ahead log"));
    }

    #[test]
    fn duplicate_session_ids_across_databases_keep_the_newest() {
        let root = tempfile::tempdir().unwrap();
        let stale_path = root.path().join("opencode-alt.db");
        let stale = Connection::open(&stale_path).unwrap();
        apply_minimal_schema(&stale);
        stale
            .execute(
                "INSERT INTO session VALUES ('ses_duplicate', 1_777_634_000_000, 1_777_634_001_000)",
                [],
            )
            .unwrap();
        drop(stale);

        let fresh_path = root.path().join("opencode.db");
        let fresh = Connection::open(&fresh_path).unwrap();
        apply_minimal_schema(&fresh);
        fresh
            .execute(
                "INSERT INTO session VALUES ('ses_duplicate', 1_777_634_002_000, 1_777_634_003_000)",
                [],
            )
            .unwrap();
        drop(fresh);

        let (rows, issues) = scan(&root, 10);

        assert!(issues.is_empty());
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0]["filePath"], json!(fresh_path.to_string_lossy()));
        assert_eq!(rows[0]["createdAt"], json!("2026-05-01T11:13:22.000Z"));
    }

    #[test]
    fn duplicate_with_equal_recency_prefers_the_canonical_database() {
        let root = tempfile::tempdir().unwrap();
        let canonical_path = root.path().join("opencode.db");
        let canonical = Connection::open(&canonical_path).unwrap();
        apply_minimal_schema(&canonical);
        canonical
            .execute(
                "INSERT INTO session VALUES ('ses_tie', 1_777_634_000_000, 1_777_634_001_000)",
                [],
            )
            .unwrap();
        drop(canonical);

        let sibling_path = root.path().join("opencode-sibling.db");
        let sibling = Connection::open(&sibling_path).unwrap();
        apply_minimal_schema(&sibling);
        sibling
            .execute(
                "INSERT INTO session VALUES ('ses_tie', 1_777_634_000_000, 1_777_634_001_000)",
                [],
            )
            .unwrap();
        drop(sibling);

        let (rows, issues) = scan(&root, 10);

        assert!(issues.is_empty());
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0]["filePath"], json!(canonical_path.to_string_lossy()));
    }

    #[test]
    fn legacy_and_sqlite_rows_with_the_same_session_id_are_both_parsed() {
        let root = tempfile::tempdir().unwrap();
        let legacy_path = write_legacy_session(
            root.path(),
            "project",
            "ses_shared.json",
            &json!({
                "id": "ses_shared",
                "title": "Legacy copy",
                "directory": "/tmp/legacy",
                "time": { "created": 1_777_634_000_000i64 }
            }),
        );
        write_legacy_message(
            root.path(),
            "ses_shared",
            "msg_1.json",
            &legacy_user_message("Legacy prompt", 1_777_634_000_100),
        );

        let db_path = root.path().join("opencode.db");
        let conn = Connection::open(&db_path).unwrap();
        apply_minimal_schema(&conn);
        conn.execute(
            "INSERT INTO session VALUES ('ses_shared', 1_777_634_002_000, 1_777_634_003_000)",
            [],
        )
        .unwrap();
        drop(conn);

        let (rows, issues) = scan(&root, 10);

        assert!(issues.is_empty());
        assert_eq!(rows.len(), 2);
        let legacy = row_for(&rows, "ses_shared");
        assert_eq!(legacy["filePath"], json!(legacy_path.to_string_lossy()));
        let file_paths: Vec<&str> = rows
            .iter()
            .map(|row| row["filePath"].as_str().unwrap())
            .collect();
        assert!(file_paths
            .iter()
            .any(|path| path.ends_with("ses_shared.json")));
        assert!(file_paths.iter().any(|path| path.ends_with("opencode.db")));
    }
}
