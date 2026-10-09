//! Persistence for local automation definitions and their run history.
//!
//! The whole state lives in one `automations.json` file shaped
//! `{ "schemaVersion": 1, "automations": [...], "runs": [...] }`. Every mutation
//! rewrites the file atomically (tmp file + rename); a file that cannot be
//! parsed — or whose top level is not an object — is renamed to
//! `automations.json.corrupt-<epoch-ms>` and the store starts empty.
//!
//! Records are opaque [`serde_json::Value`]s. The store only reads and
//! normalizes the identity/scheduling fields it needs (`id`, `enabled`,
//! `nextRunAt`, `creationKey` for automations; `id`, `automationId`,
//! `scheduledFor`, `status`, `createdAt`, `runNumber`, `workspaceId` for runs)
//! and leaves every other field untouched.
//!
//! Semantics ported from the Electron reference (orca) store operations:
//! - `create_run` dedupes on `(automationId, scheduledFor)`; a fresh insert
//!   assigns the next `runNumber` from the highest retained number (or the
//!   retained count for legacy unnumbered rows), so numbering survives
//!   retention pruning, and advances the parent automation's `lastRunAt`.
//! - `update_run` never overwrites a run that has reached a final status.
//! - `runs_page` is a keyset cursor (`"<createdAt>:<id>"`, or a legacy bare
//!   offset) over runs ordered `createdAt desc, id asc`, capped at
//!   [`MAX_RUNS_PER_AUTOMATION`] per page.
//! - `prune_runs` keeps the newest [`MAX_RUNS_PER_AUTOMATION`] final runs per
//!   automation and never evicts an in-flight run.
//! - `snapshot_workspace_name` writes the display name into every run still
//!   pointing at that workspace (walked newest first).
//! - Loading normalizes legacy runs: unnumbered rows are backfilled above the
//!   highest number they share an automation with, then each automation is
//!   pruned, and the repaired state is written back.

use std::collections::{HashMap, HashSet};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

use serde_json::{Map, Value};

/// Retention cap per automation, aligned with
/// `src/shared/automation-run-retention.ts`.
pub const MAX_RUNS_PER_AUTOMATION: usize = 100;

/// Schema version written for new state; a version already stored in the file is
/// preserved on rewrite.
pub const SCHEMA_VERSION: u64 = 1;

const FINAL_RUN_STATUSES: [&str; 6] = [
    "completed",
    "dispatch_failed",
    "skipped_precheck",
    "skipped_missed",
    "skipped_unavailable",
    "skipped_needs_interactive_auth",
];

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error("invalid automation store payload: {0}")]
    InvalidInput(String),
}

#[derive(Clone)]
struct Inner {
    schema_version: u64,
    automations: Vec<Value>,
    runs: Vec<Value>,
}

impl Inner {
    fn empty() -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            automations: Vec::new(),
            runs: Vec::new(),
        }
    }

    fn to_value(&self) -> Value {
        let mut object = Map::new();
        object.insert(
            "schemaVersion".to_string(),
            Value::from(self.schema_version),
        );
        object.insert(
            "automations".to_string(),
            Value::Array(self.automations.clone()),
        );
        object.insert("runs".to_string(), Value::Array(self.runs.clone()));
        Value::Object(object)
    }
}

/// A page of runs ordered `createdAt desc, id asc`.
pub struct RunsPage {
    pub runs: Vec<Value>,
    pub next_cursor: Option<String>,
}

enum Cursor {
    Key { created_at: i64, id: String },
    Offset(usize),
}

pub struct AutomationStore {
    path: PathBuf,
    inner: Mutex<Inner>,
}

impl AutomationStore {
    /// Loads state from `path`. A corrupt file is moved aside to
    /// `automations.json.corrupt-<epoch-ms>` and the store starts empty; a
    /// loadable file is normalized (legacy run numbers backfilled, retention
    /// enforced) and written back only when normalization changed something.
    pub fn load(path: PathBuf) -> Self {
        let (state, changed) = load_state(&path);
        let store = Self {
            path,
            inner: Mutex::new(state),
        };
        if changed {
            let _ = store.persist_current();
        }
        store
    }

    pub fn list(&self) -> Vec<Value> {
        self.lock().automations.clone()
    }

    pub fn get(&self, id: &str) -> Option<Value> {
        self.lock()
            .automations
            .iter()
            .find(|automation| string_field(automation, "id") == Some(id))
            .cloned()
    }

    /// Creates an automation from an opaque input object. Identity and
    /// timestamps are store-owned; defaults are applied only when the input
    /// omits them. When `creationKey` matches an existing automation the
    /// existing record is returned with `created == false`.
    pub fn create(&self, input: Value, now_ms: i64) -> Result<(Value, bool), StoreError> {
        let mut record = input.as_object().cloned().ok_or_else(|| {
            StoreError::InvalidInput("automation input must be a JSON object".into())
        })?;
        let mut inner = self.lock();
        if let Some(key) = record
            .get("creationKey")
            .and_then(Value::as_str)
            .filter(|key| !key.is_empty())
        {
            if let Some(existing) = inner
                .automations
                .iter()
                .find(|automation| string_field(automation, "creationKey") == Some(key))
            {
                return Ok((existing.clone(), false));
            }
        }
        record.insert("id".to_string(), Value::from(ade_core::ids::new_uuid()));
        record.insert("createdAt".to_string(), Value::from(now_ms));
        record.insert("updatedAt".to_string(), Value::from(now_ms));
        record
            .entry("schedulerOwner".to_string())
            .or_insert_with(|| Value::from("local_host_service"));
        record
            .entry("missedRunPolicy".to_string())
            .or_insert_with(|| Value::from("run_once_within_grace"));
        record
            .entry("enabled".to_string())
            .or_insert_with(|| Value::from(true));
        record
            .entry("missedRunGraceMinutes".to_string())
            .or_insert_with(|| Value::from(720));
        let record = Value::Object(record);

        let mut next = inner.clone();
        next.automations.push(record.clone());
        self.persist_state(&next)?;
        *inner = next;
        Ok((record, true))
    }

    /// Shallow-merges `updates` into the automation, removes keys set to
    /// `null`, and refreshes `updatedAt`. `id` and `createdAt` are store-owned
    /// and cannot be overwritten. Returns `None` when the automation is gone.
    pub fn update(
        &self,
        id: &str,
        updates: Value,
        now_ms: i64,
    ) -> Result<Option<Value>, StoreError> {
        let updates = updates.as_object().ok_or_else(|| {
            StoreError::InvalidInput("automation updates must be a JSON object".into())
        })?;
        let mut inner = self.lock();
        let Some(index) = inner
            .automations
            .iter()
            .position(|automation| string_field(automation, "id") == Some(id))
        else {
            return Ok(None);
        };
        let mut next = inner.clone();
        if let Some(record) = next.automations[index].as_object_mut() {
            for (key, value) in updates {
                if key == "id" || key == "createdAt" {
                    continue;
                }
                merge_field(record, key, value);
            }
            record.insert("updatedAt".to_string(), Value::from(now_ms));
        }
        let updated = next.automations[index].clone();
        self.persist_state(&next)?;
        *inner = next;
        Ok(Some(updated))
    }

    /// Deletes the automation and every run it owns. Returns `false` when the
    /// automation does not exist (runs are not touched).
    pub fn delete(&self, id: &str) -> Result<bool, StoreError> {
        let mut inner = self.lock();
        if !inner
            .automations
            .iter()
            .any(|automation| string_field(automation, "id") == Some(id))
        {
            return Ok(false);
        }
        let mut next = inner.clone();
        next.automations
            .retain(|automation| string_field(automation, "id") != Some(id));
        next.runs
            .retain(|run| string_field(run, "automationId") != Some(id));
        self.persist_state(&next)?;
        *inner = next;
        Ok(true)
    }

    /// Persists a run. `(automationId, scheduledFor)` is the dedupe key: a
    /// second insert returns the existing row with `created == false` and
    /// leaves the parent automation untouched. `runNumber` is always assigned
    /// by the store; `createdAt` is assigned only when absent. Retention
    /// pruning runs after the insert, and the parent automation's `lastRunAt`
    /// (plus `updatedAt`) advances to the run's `createdAt`, falling back to
    /// `scheduledFor` when that timestamp is unreadable.
    pub fn create_run(&self, run: Value) -> Result<(Value, bool), StoreError> {
        let mut record = run.as_object().cloned().ok_or_else(|| {
            StoreError::InvalidInput("automation run must be a JSON object".into())
        })?;
        let automation_id = record
            .get("automationId")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                StoreError::InvalidInput("automation run requires a string automationId".into())
            })?
            .to_string();
        let scheduled_for = record
            .get("scheduledFor")
            .and_then(Value::as_i64)
            .ok_or_else(|| {
                StoreError::InvalidInput("automation run requires a numeric scheduledFor".into())
            })?;
        let mut inner = self.lock();
        if let Some(existing) = inner.runs.iter().find(|existing| {
            string_field(existing, "automationId") == Some(automation_id.as_str())
                && i64_field(existing, "scheduledFor") == Some(scheduled_for)
        }) {
            return Ok((existing.clone(), false));
        }
        if record.get("id").and_then(Value::as_str).is_none() {
            record.insert("id".to_string(), Value::from(ade_core::ids::new_uuid()));
        }
        let run_number = next_run_number(&inner.runs, &automation_id);
        record.insert("runNumber".to_string(), Value::from(run_number));
        if record.get("createdAt").and_then(Value::as_i64).is_none() {
            record.insert("createdAt".to_string(), Value::from(now_millis()));
        }
        let record = Value::Object(record);
        // The parent automation's "last run" marker follows the run's creation
        // time; a deduped insert returns above and never touches it.
        let touched_at = i64_field(&record, "createdAt").unwrap_or(scheduled_for);

        let mut next = inner.clone();
        next.runs.push(record.clone());
        prune_runs_for(&mut next.runs, &automation_id);
        touch_automation(&mut next.automations, &automation_id, touched_at);
        self.persist_state(&next)?;
        *inner = next;
        Ok((record, true))
    }

    /// Shallow-merges `fields` into the run, removing keys set to `null`. A run
    /// that already holds a final status is returned untouched. Returns `None`
    /// when the run is gone.
    pub fn update_run(&self, run_id: &str, fields: Value) -> Result<Option<Value>, StoreError> {
        let fields = fields.as_object().ok_or_else(|| {
            StoreError::InvalidInput("automation run updates must be a JSON object".into())
        })?;
        let mut inner = self.lock();
        let Some(index) = inner
            .runs
            .iter()
            .position(|run| string_field(run, "id") == Some(run_id))
        else {
            return Ok(None);
        };
        if is_final_run(&inner.runs[index]) {
            return Ok(Some(inner.runs[index].clone()));
        }
        let mut next = inner.clone();
        if let Some(record) = next.runs[index].as_object_mut() {
            for (key, value) in fields {
                if key == "id" || key == "automationId" {
                    continue;
                }
                merge_field(record, key, value);
            }
        }
        let updated = next.runs[index].clone();
        self.persist_state(&next)?;
        *inner = next;
        Ok(Some(updated))
    }

    /// Keyset page over runs ordered `createdAt desc, id asc`. `limit` is
    /// clamped to `1..=MAX_RUNS_PER_AUTOMATION`. The cursor names the previous
    /// page's last run, so runs created between requests cannot shift the
    /// window; a cursor whose boundary run was pruned resumes at the first run
    /// the total order places after it.
    pub fn runs_page(
        &self,
        automation_id: Option<&str>,
        limit: usize,
        cursor: Option<&str>,
    ) -> RunsPage {
        let inner = self.lock();
        let mut runs: Vec<Value> = inner
            .runs
            .iter()
            .filter(|run| match automation_id {
                None => true,
                Some(id) => string_field(run, "automationId") == Some(id),
            })
            .cloned()
            .collect();
        runs.sort_by(compare_runs_newest_first);
        let start = page_start(&runs, cursor);
        let bounded = limit.clamp(1, MAX_RUNS_PER_AUTOMATION);
        let end = start.saturating_add(bounded).min(runs.len());
        let page = if start >= runs.len() {
            Vec::new()
        } else {
            runs[start..end].to_vec()
        };
        let next_cursor = if end < runs.len() {
            page.last().map(|last| {
                format!(
                    "{}:{}",
                    i64_field(last, "createdAt").unwrap_or(0),
                    string_field(last, "id").unwrap_or("")
                )
            })
        } else {
            None
        };
        RunsPage {
            runs: page,
            next_cursor,
        }
    }

    /// Enforces the retention cap for one automation. Only final runs are
    /// evictable; in-flight runs are always kept. No-op when under the cap.
    pub fn prune_runs(&self, automation_id: &str) {
        let mut inner = self.lock();
        let mut next = inner.clone();
        if prune_runs_for(&mut next.runs, automation_id) && self.persist_state(&next).is_ok() {
            *inner = next;
        }
    }

    /// Runs that have not reached a final status and were created before
    /// `cutoff_ms`, in append order.
    pub fn non_final_runs_before(&self, cutoff_ms: i64) -> Vec<Value> {
        self.lock()
            .runs
            .iter()
            .filter(|run| {
                !is_final_run(run) && i64_field(run, "createdAt").is_some_and(|at| at < cutoff_ms)
            })
            .cloned()
            .collect()
    }

    /// Writes `display_name` into every run whose `workspaceId` matches,
    /// newest first (in `createdAt desc, id asc` order). Runs already carrying
    /// the normalized name are skipped. Returns the number of runs updated.
    /// Empty/whitespace names are ignored.
    pub fn snapshot_workspace_name(&self, workspace_id: &str, display_name: &str) -> usize {
        let normalized = display_name.trim();
        if normalized.is_empty() {
            return 0;
        }
        let mut inner = self.lock();
        let mut matching: Vec<usize> = inner
            .runs
            .iter()
            .enumerate()
            .filter(|(_, run)| string_field(run, "workspaceId") == Some(workspace_id))
            .map(|(index, _)| index)
            .collect();
        matching.sort_by(|&left, &right| {
            compare_runs_newest_first(&inner.runs[left], &inner.runs[right])
        });
        let mut next = inner.clone();
        let mut updated = 0;
        for index in matching {
            if string_field(&next.runs[index], "workspaceDisplayName").map(str::trim)
                == Some(normalized)
            {
                continue;
            }
            if let Some(record) = next.runs[index].as_object_mut() {
                record.insert("workspaceDisplayName".to_string(), Value::from(normalized));
                updated += 1;
            }
        }
        if updated == 0 {
            return 0;
        }
        if self.persist_state(&next).is_err() {
            return 0;
        }
        *inner = next;
        updated
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().expect("automations store lock poisoned")
    }

    fn persist_current(&self) -> Result<(), StoreError> {
        self.persist_state(&self.lock())
    }

    fn persist_state(&self, state: &Inner) -> Result<(), StoreError> {
        if let Some(parent) = self
            .path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            std::fs::create_dir_all(parent)?;
        }
        let payload = format!(
            "{}\n",
            serde_json::to_string_pretty(&state.to_value()).map_err(std::io::Error::other)?
        );
        let tmp = self.path.with_extension("tmp");
        let mut file = std::fs::File::create(&tmp)?;
        file.write_all(payload.as_bytes())?;
        file.sync_all()?;
        std::fs::rename(&tmp, &self.path)?;
        Ok(())
    }
}

fn now_millis() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

fn string_field<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value.get(key).and_then(Value::as_str)
}

fn i64_field(value: &Value, key: &str) -> Option<i64> {
    value.get(key).and_then(Value::as_i64)
}

fn is_final_run(run: &Value) -> bool {
    string_field(run, "status").is_some_and(|status| FINAL_RUN_STATUSES.contains(&status))
}

/// `null` removes the key, anything else replaces it.
fn merge_field(record: &mut Map<String, Value>, key: &str, value: &Value) {
    if value.is_null() {
        record.remove(key);
    } else {
        record.insert(key.to_string(), value.clone());
    }
}

/// Advances the parent automation's `lastRunAt` and `updatedAt` when a run is
/// created. No-op when the automation is gone.
fn touch_automation(automations: &mut [Value], automation_id: &str, at_ms: i64) {
    let Some(automation) = automations
        .iter_mut()
        .find(|automation| string_field(automation, "id") == Some(automation_id))
    else {
        return;
    };
    if let Some(record) = automation.as_object_mut() {
        record.insert("lastRunAt".to_string(), Value::from(at_ms));
        record.insert("updatedAt".to_string(), Value::from(at_ms));
    }
}

/// Total order shared by pages, cursors, and snapshots.
fn compare_runs_newest_first(left: &Value, right: &Value) -> std::cmp::Ordering {
    i64_field(right, "createdAt")
        .unwrap_or(0)
        .cmp(&i64_field(left, "createdAt").unwrap_or(0))
        .then_with(|| {
            string_field(left, "id")
                .unwrap_or("")
                .cmp(string_field(right, "id").unwrap_or(""))
        })
}

/// Next run number for one automation: one above the highest retained number,
/// seeded with the retained count so legacy unnumbered rows still advance.
fn next_run_number(runs: &[Value], automation_id: &str) -> i64 {
    let mut highest: i64 = 0;
    let mut count: i64 = 0;
    for run in runs {
        if string_field(run, "automationId") != Some(automation_id) {
            continue;
        }
        count += 1;
        highest = highest.max(i64_field(run, "runNumber").unwrap_or(0));
    }
    highest.max(count) + 1
}

/// Stamps `runNumber` onto legacy rows that predate the field, continuing above
/// the highest number the automation already carries. Returns whether any row
/// changed.
fn backfill_run_numbers(runs: &mut [Value]) -> bool {
    let mut highest: HashMap<String, i64> = HashMap::new();
    for run in runs.iter() {
        let Some(automation_id) = string_field(run, "automationId") else {
            continue;
        };
        if let Some(number) = i64_field(run, "runNumber") {
            let entry = highest.entry(automation_id.to_string()).or_insert(0);
            *entry = (*entry).max(number);
        }
    }
    let mut changed = false;
    for run in runs.iter_mut() {
        if i64_field(run, "runNumber").is_some() {
            continue;
        }
        let Some(automation_id) = string_field(run, "automationId").map(str::to_string) else {
            continue;
        };
        let entry = highest.entry(automation_id).or_insert(0);
        *entry += 1;
        if let Some(record) = run.as_object_mut() {
            record.insert("runNumber".to_string(), Value::from(*entry));
        }
        changed = true;
    }
    changed
}

/// Evicts final runs past the cap for one automation. Survivors keep their
/// original append order; in-flight and other-automation runs are untouched.
/// Returns whether anything was evicted.
fn prune_runs_for(runs: &mut Vec<Value>, automation_id: &str) -> bool {
    let candidates: Vec<usize> = runs
        .iter()
        .enumerate()
        .filter(|(_, run)| {
            string_field(run, "automationId") == Some(automation_id) && is_final_run(run)
        })
        .map(|(index, _)| index)
        .collect();
    if candidates.len() <= MAX_RUNS_PER_AUTOMATION {
        return false;
    }
    let mut ranked = candidates.clone();
    ranked.sort_by(|&left, &right| {
        let left_run = &runs[left];
        let right_run = &runs[right];
        // Newest first; `scheduledFor` breaks createdAt ties for determinism.
        i64_field(right_run, "createdAt")
            .unwrap_or(0)
            .cmp(&i64_field(left_run, "createdAt").unwrap_or(0))
            .then_with(|| {
                i64_field(right_run, "scheduledFor")
                    .unwrap_or(0)
                    .cmp(&i64_field(left_run, "scheduledFor").unwrap_or(0))
            })
    });
    ranked.truncate(MAX_RUNS_PER_AUTOMATION);
    let candidate_set: HashSet<usize> = candidates.into_iter().collect();
    let keep: HashSet<usize> = ranked.into_iter().collect();
    let mut index = 0;
    runs.retain(|_| {
        let keep_this = !candidate_set.contains(&index) || keep.contains(&index);
        index += 1;
        keep_this
    });
    true
}

/// Prunes every automation that appears in `runs`.
fn prune_all_automation_runs(runs: &mut Vec<Value>) -> bool {
    let automation_ids: Vec<String> = runs
        .iter()
        .filter_map(|run| string_field(run, "automationId").map(str::to_string))
        .collect();
    let mut seen: HashSet<String> = HashSet::new();
    let mut changed = false;
    for automation_id in automation_ids {
        if seen.insert(automation_id.clone()) {
            changed |= prune_runs_for(runs, &automation_id);
        }
    }
    changed
}

fn decode_cursor(cursor: &str) -> Option<Cursor> {
    if let Some((created_at, id)) = cursor.split_once(':') {
        let created_at = created_at.parse::<i64>().ok()?;
        if id.is_empty() {
            return None;
        }
        Some(Cursor::Key {
            created_at,
            id: id.to_string(),
        })
    } else {
        let offset = cursor.parse::<i64>().ok()?;
        if offset > 0 {
            Some(Cursor::Offset(offset as usize))
        } else {
            None
        }
    }
}

fn page_start(runs: &[Value], cursor: Option<&str>) -> usize {
    let Some(cursor) = cursor.and_then(decode_cursor) else {
        return 0;
    };
    match cursor {
        Cursor::Offset(offset) => offset.min(runs.len()),
        Cursor::Key { created_at, id } => {
            if let Some(position) = runs.iter().position(|run| {
                string_field(run, "id") == Some(id.as_str())
                    && i64_field(run, "createdAt") == Some(created_at)
            }) {
                return position + 1;
            }
            // Boundary pruned: resume at the first run this cursor orders
            // before, so runs tied on `createdAt` are not dropped with it.
            let boundary = Value::Object(Map::from_iter([
                ("createdAt".to_string(), Value::from(created_at)),
                ("id".to_string(), Value::from(id)),
            ]));
            runs.iter()
                .position(|run| compare_runs_newest_first(&boundary, run).is_lt())
                .unwrap_or(runs.len())
        }
    }
}

fn load_state(path: &Path) -> (Inner, bool) {
    let Ok(text) = std::fs::read_to_string(path) else {
        return (Inner::empty(), false);
    };
    let parsed = match serde_json::from_str::<Value>(&text) {
        Ok(parsed) => parsed,
        Err(_) => {
            backup_corrupt(path);
            return (Inner::empty(), false);
        }
    };
    let Value::Object(object) = parsed else {
        backup_corrupt(path);
        return (Inner::empty(), false);
    };
    let schema_version = object.get("schemaVersion").and_then(Value::as_u64);
    let (automations, automations_changed) = records_from(object.get("automations"));
    let (mut runs, runs_changed) = records_from(object.get("runs"));
    let backfilled = backfill_run_numbers(&mut runs);
    let pruned = prune_all_automation_runs(&mut runs);
    let changed = schema_version != Some(SCHEMA_VERSION)
        || automations_changed
        || runs_changed
        || backfilled
        || pruned;
    let inner = Inner {
        schema_version: schema_version.unwrap_or(SCHEMA_VERSION),
        automations,
        runs,
    };
    (inner, changed)
}

fn records_from(value: Option<&Value>) -> (Vec<Value>, bool) {
    match value {
        None => (Vec::new(), false),
        Some(Value::Array(records)) => {
            let filtered: Vec<Value> = records
                .iter()
                .filter(|record| record.is_object())
                .cloned()
                .collect();
            let changed = filtered.len() != records.len();
            (filtered, changed)
        }
        Some(_) => (Vec::new(), true),
    }
}

fn backup_corrupt(path: &Path) {
    let timestamp = now_millis();
    let mut backup = path.as_os_str().to_os_string();
    backup.push(format!(".corrupt-{timestamp}"));
    let _ = std::fs::rename(path, PathBuf::from(backup));
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::sync::atomic::{AtomicU64, Ordering};

    static COUNTER: AtomicU64 = AtomicU64::new(0);

    struct TestDir {
        path: std::path::PathBuf,
    }

    impl TestDir {
        fn new(name: &str) -> Self {
            let unique = COUNTER.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "ade-automations-store-{name}-{}-{unique}",
                std::process::id()
            ));
            let _ = std::fs::remove_dir_all(&path);
            std::fs::create_dir_all(&path).expect("create test dir");
            Self { path }
        }

        fn file(&self, name: &str) -> std::path::PathBuf {
            self.path.join(name)
        }

        fn files_with_prefix(&self, prefix: &str) -> Vec<std::path::PathBuf> {
            let mut files: Vec<std::path::PathBuf> = std::fs::read_dir(&self.path)
                .expect("read test dir")
                .filter_map(Result::ok)
                .map(|entry| entry.path())
                .filter(|path| {
                    path.file_name()
                        .and_then(|name| name.to_str())
                        .is_some_and(|name| name.starts_with(prefix))
                })
                .collect();
            files.sort();
            files
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }

    fn load_store(dir: &TestDir) -> AutomationStore {
        AutomationStore::load(dir.file("automations.json"))
    }

    fn create_input() -> serde_json::Value {
        json!({
            "name": "Nightly",
            "prompt": "Run checks",
            "agentId": "claude",
            "projectId": "r1",
            "workspaceMode": "existing",
            "workspaceId": null,
            "timezone": "UTC",
            "rrule": "FREQ=DAILY;BYHOUR=9;BYMINUTE=0",
            "dtstart": 0,
            "nextRunAt": 1_000
        })
    }

    fn create_automation(store: &AutomationStore) -> serde_json::Value {
        store.create(create_input(), 1_000).unwrap().0
    }

    fn collect_runs(
        store: &AutomationStore,
        automation_id: Option<&str>,
    ) -> Vec<serde_json::Value> {
        let mut runs = Vec::new();
        let mut cursor: Option<String> = None;
        loop {
            let page = store.runs_page(automation_id, 100, cursor.as_deref());
            runs.extend(page.runs);
            match page.next_cursor {
                Some(next) => cursor = Some(next),
                None => break,
            }
        }
        runs
    }

    fn run_ids(runs: &[serde_json::Value]) -> Vec<String> {
        runs.iter()
            .map(|run| run["id"].as_str().unwrap().to_string())
            .collect()
    }

    #[test]
    fn create_assigns_identity_and_defaults_but_keeps_the_caller_next_run_at() {
        let dir = TestDir::new("store-create");
        let store = load_store(&dir);
        let (automation, created) = store
            .create(create_input(), 1_700_000_000_000)
            .expect("create automation");

        assert!(created);
        assert!(automation["id"].as_str().is_some_and(|id| !id.is_empty()));
        assert_eq!(automation["createdAt"], 1_700_000_000_000i64);
        assert_eq!(automation["updatedAt"], 1_700_000_000_000i64);
        assert_eq!(automation["schedulerOwner"], "local_host_service");
        assert_eq!(automation["missedRunPolicy"], "run_once_within_grace");
        assert_eq!(automation["enabled"], true);
        assert_eq!(automation["missedRunGraceMinutes"], 720);
        assert_eq!(automation["name"], "Nightly");
        // The store never computes schedules: the caller-provided value survives untouched.
        assert_eq!(automation["nextRunAt"], 1_000i64);

        let id = automation["id"].as_str().unwrap();
        assert_eq!(store.get(id).unwrap(), automation);
        assert_eq!(store.list(), vec![automation.clone()]);

        let (second, _) = store
            .create(
                json!({
                    "name": "Manual",
                    "prompt": "p",
                    "enabled": false,
                    "id": "caller-supplied-id",
                    "createdAt": 1,
                    "updatedAt": 1,
                }),
                5,
            )
            .unwrap();
        assert_eq!(second["enabled"], false);
        assert_ne!(second["id"], "caller-supplied-id");
        assert_eq!(second["createdAt"], 5);
        assert_eq!(second["updatedAt"], 5);
        // No nextRunAt was supplied, so none is materialized.
        assert!(second.get("nextRunAt").is_none());
    }

    #[test]
    fn create_with_creation_key_is_idempotent() {
        let dir = TestDir::new("store-creation-key");
        let store = load_store(&dir);
        let (first, created) = store
            .create(
                json!({"name": "A", "prompt": "p", "creationKey": "job-1", "nextRunAt": 1}),
                1_000,
            )
            .unwrap();
        assert!(created);

        let (second, created) = store
            .create(
                json!({"name": "B", "prompt": "q", "creationKey": "job-1", "nextRunAt": 2}),
                2_000,
            )
            .unwrap();
        assert!(!created);
        assert_eq!(second, first);
        assert_eq!(store.list().len(), 1);
        assert_eq!(load_store(&dir).list(), vec![first.clone()]);

        // An empty key is not an idempotency key.
        let (third, created) = store
            .create(
                json!({"name": "C", "prompt": "p", "creationKey": ""}),
                3_000,
            )
            .unwrap();
        assert!(created);
        let (fourth, created) = store
            .create(
                json!({"name": "D", "prompt": "p", "creationKey": ""}),
                4_000,
            )
            .unwrap();
        assert!(created);
        assert_ne!(third["id"], fourth["id"]);
    }

    #[test]
    fn update_merges_fields_refreshes_updated_at_and_removes_null_fields() {
        let dir = TestDir::new("store-update");
        let store = load_store(&dir);
        let automation = create_automation(&store);
        let id = automation["id"].as_str().unwrap();

        let updated = store
            .update(
                id,
                json!({
                    "name": "Renamed",
                    "nextRunAt": 4_242,
                    "workspaceId": null,
                    "futureField": {"x": 1}
                }),
                2_000,
            )
            .unwrap()
            .expect("automation exists");
        assert_eq!(updated["name"], "Renamed");
        assert_eq!(updated["nextRunAt"], 4_242i64);
        assert_eq!(updated["prompt"], "Run checks");
        assert_eq!(updated["createdAt"], 1_000i64);
        assert_eq!(updated["updatedAt"], 2_000i64);
        assert!(updated.get("workspaceId").is_none());
        // Unknown fields stay opaque and are merged through.
        assert_eq!(updated["futureField"], json!({"x": 1}));
        assert_eq!(store.get(id).unwrap()["name"], "Renamed");

        assert!(store
            .update("missing", json!({"name": "x"}), 3_000)
            .unwrap()
            .is_none());
        assert!(matches!(
            store.update(id, json!("not an object"), 3_000),
            Err(StoreError::InvalidInput(_))
        ));
        assert!(matches!(
            store.create(json!("not an object"), 3_000),
            Err(StoreError::InvalidInput(_))
        ));
    }

    #[test]
    fn delete_cascades_runs_and_reports_removal() {
        let dir = TestDir::new("store-delete");
        let store = load_store(&dir);
        let automation = create_automation(&store);
        let id = automation["id"].as_str().unwrap();
        let other = create_automation(&store);
        let other_id = other["id"].as_str().unwrap();

        store
            .create_run(json!({"automationId": id, "scheduledFor": 1, "status": "pending"}))
            .unwrap();
        store
            .create_run(json!({"automationId": other_id, "scheduledFor": 1, "status": "pending"}))
            .unwrap();

        assert!(store.delete(id).unwrap());
        assert!(store.get(id).is_none());
        assert!(collect_runs(&store, Some(id)).is_empty());
        assert_eq!(collect_runs(&store, Some(other_id)).len(), 1);
        assert!(!store.delete(id).unwrap());

        let saved: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(dir.file("automations.json")).unwrap())
                .unwrap();
        assert_eq!(saved["schemaVersion"], 1);
        assert_eq!(saved["automations"].as_array().unwrap().len(), 1);
        assert_eq!(saved["runs"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn create_run_dedupes_and_numbers_runs_monotonically_across_prunes() {
        let dir = TestDir::new("store-run-numbering");
        let store = load_store(&dir);
        let automation = create_automation(&store);
        let automation_id = automation["id"].as_str().unwrap();

        let (first, created) = store
            .create_run(json!({
                "automationId": automation_id,
                "scheduledFor": 100,
                "status": "pending",
                "trigger": "scheduled"
            }))
            .unwrap();
        assert!(created);
        assert_eq!(first["runNumber"], 1);
        assert!(first["createdAt"].as_i64().is_some_and(|value| value > 0));
        assert!(first["id"].as_str().is_some_and(|id| !id.is_empty()));

        let (duplicate, created) = store
            .create_run(json!({
                "automationId": automation_id,
                "scheduledFor": 100,
                "status": "pending",
                "trigger": "scheduled"
            }))
            .unwrap();
        assert!(!created);
        assert_eq!(duplicate, first);
        assert_eq!(
            store.runs_page(Some(automation_id), 100, None).runs.len(),
            1
        );

        let (second, created) = store
            .create_run(json!({
                "automationId": automation_id,
                "scheduledFor": 200,
                "status": "pending"
            }))
            .unwrap();
        assert!(created);
        assert_eq!(second["runNumber"], 2);

        for index in 0..101 {
            store
                .create_run(json!({
                    "automationId": automation_id,
                    "scheduledFor": 10_000 + index,
                    "status": "completed",
                    "createdAt": 10_000 + index
                }))
                .unwrap();
        }
        let (overflow, _) = store
            .create_run(json!({
                "automationId": automation_id,
                "scheduledFor": 20_000,
                "status": "completed",
                "createdAt": 20_000
            }))
            .unwrap();
        assert_eq!(overflow["runNumber"], 104);

        let runs = collect_runs(&store, Some(automation_id));
        assert_eq!(runs.len(), 102);
        let mut numbers: Vec<i64> = runs
            .iter()
            .map(|run| run["runNumber"].as_i64().unwrap())
            .collect();
        let total = numbers.len();
        numbers.sort_unstable();
        numbers.dedup();
        assert_eq!(numbers.len(), total);
        assert_eq!(numbers.iter().min().copied(), Some(1));
        assert_eq!(numbers.iter().max().copied(), Some(104));

        let (next, _) = store
            .create_run(json!({
                "automationId": automation_id,
                "scheduledFor": 30_000,
                "status": "completed",
                "createdAt": 30_000
            }))
            .unwrap();
        assert_eq!(next["runNumber"], 105);
    }

    #[test]
    fn update_run_merges_while_final_statuses_are_guarded() {
        let dir = TestDir::new("store-update-run");
        let store = load_store(&dir);
        let automation = create_automation(&store);
        let automation_id = automation["id"].as_str().unwrap();

        let (run, _) = store
            .create_run(json!({
                "automationId": automation_id,
                "scheduledFor": 1,
                "status": "pending",
                "trigger": "scheduled"
            }))
            .unwrap();
        let run_id = run["id"].as_str().unwrap();

        let dispatched = store
            .update_run(
                run_id,
                json!({"status": "dispatched", "terminalSessionId": "t1", "error": null}),
            )
            .unwrap()
            .unwrap();
        assert_eq!(dispatched["status"], "dispatched");
        assert_eq!(dispatched["terminalSessionId"], "t1");
        assert!(dispatched.get("error").is_none());

        let completed = store
            .update_run(
                run_id,
                json!({
                    "status": "completed",
                    "outputSnapshot": {
                        "format": "plain_text",
                        "content": "done",
                        "capturedAt": 1,
                        "truncated": false
                    },
                    "error": null
                }),
            )
            .unwrap()
            .unwrap();
        assert_eq!(completed["status"], "completed");

        let guarded = store
            .update_run(
                run_id,
                json!({"status": "dispatching", "terminalSessionId": "t2"}),
            )
            .unwrap()
            .unwrap();
        assert_eq!(guarded["status"], "completed");
        assert_eq!(guarded["terminalSessionId"], "t1");
        let reloaded = load_store(&dir);
        let persisted = collect_runs(&reloaded, Some(automation_id));
        assert_eq!(persisted[0]["status"], "completed");
        assert_eq!(persisted[0]["terminalSessionId"], "t1");

        for (index, final_status) in [
            "completed",
            "dispatch_failed",
            "skipped_precheck",
            "skipped_missed",
            "skipped_unavailable",
            "skipped_needs_interactive_auth",
        ]
        .iter()
        .enumerate()
        {
            let scheduled_for = 8_000 + index as i64;
            let (run, _) = store
                .create_run(json!({
                    "automationId": automation_id,
                    "scheduledFor": scheduled_for,
                    "status": "dispatched",
                    "createdAt": scheduled_for
                }))
                .unwrap();
            let run_id = run["id"].as_str().unwrap();
            let terminal = store
                .update_run(run_id, json!({"status": final_status}))
                .unwrap()
                .unwrap();
            assert_eq!(terminal["status"], *final_status);
            let guarded = store
                .update_run(
                    run_id,
                    json!({"status": "dispatching", "terminalSessionId": "late"}),
                )
                .unwrap()
                .unwrap();
            assert_eq!(guarded["status"], *final_status);
            assert!(guarded.get("terminalSessionId").is_none());
        }

        assert!(store
            .update_run("missing-run", json!({"status": "completed"}))
            .unwrap()
            .is_none());
        assert!(matches!(
            store.update_run(run_id, json!("not an object")),
            Err(StoreError::InvalidInput(_))
        ));
    }

    #[test]
    fn runs_page_orders_created_at_desc_then_id_asc() {
        let dir = TestDir::new("store-runs-order");
        let store = load_store(&dir);
        let automation = create_automation(&store);
        let automation_id = automation["id"].as_str().unwrap();

        for (id, scheduled_for, created_at) in [("b", 100, 10), ("a", 101, 10), ("c", 102, 11)] {
            store
                .create_run(json!({
                    "id": id,
                    "automationId": automation_id,
                    "scheduledFor": scheduled_for,
                    "status": "pending",
                    "createdAt": created_at
                }))
                .unwrap();
        }

        let page = store.runs_page(Some(automation_id), 100, None);
        assert_eq!(run_ids(&page.runs), vec!["c", "a", "b"]);
        assert!(page.next_cursor.is_none());
    }

    #[test]
    fn runs_page_clamps_limit_and_keeps_the_cursor_stable_when_new_runs_arrive() {
        let dir = TestDir::new("store-runs-cursor");
        let store = load_store(&dir);
        let automation = create_automation(&store);
        let automation_id = automation["id"].as_str().unwrap();

        for index in 0..6 {
            store
                .create_run(json!({
                    "id": format!("run-{index}"),
                    "automationId": automation_id,
                    "scheduledFor": index,
                    "status": "pending",
                    "createdAt": index
                }))
                .unwrap();
        }
        let page1 = store.runs_page(Some(automation_id), 2, None);
        assert_eq!(run_ids(&page1.runs), vec!["run-5", "run-4"]);
        assert_eq!(page1.next_cursor.as_deref(), Some("4:run-4"));

        // Runs arriving between page requests must not shift the window.
        store
            .create_run(json!({
                "id": "run-6",
                "automationId": automation_id,
                "scheduledFor": 6,
                "status": "pending",
                "createdAt": 6
            }))
            .unwrap();
        store
            .create_run(json!({
                "id": "run-7",
                "automationId": automation_id,
                "scheduledFor": 7,
                "status": "pending",
                "createdAt": 7
            }))
            .unwrap();

        let page2 = store.runs_page(Some(automation_id), 2, page1.next_cursor.as_deref());
        assert_eq!(run_ids(&page2.runs), vec!["run-3", "run-2"]);
        let page3 = store.runs_page(Some(automation_id), 2, page2.next_cursor.as_deref());
        assert_eq!(run_ids(&page3.runs), vec!["run-1", "run-0"]);
        assert!(page3.next_cursor.is_none());
        let fresh = store.runs_page(Some(automation_id), 2, None);
        assert_eq!(run_ids(&fresh.runs), vec!["run-7", "run-6"]);

        // limit is clamped to MAX_RUNS_PER_AUTOMATION.
        let wide = TestDir::new("store-runs-limit");
        let wide_store = load_store(&wide);
        let wide_automation = create_automation(&wide_store);
        let wide_id = wide_automation["id"].as_str().unwrap();
        for index in 0..105 {
            wide_store
                .create_run(json!({
                    "id": format!("wide-{index:03}"),
                    "automationId": wide_id,
                    "scheduledFor": index,
                    "status": "pending",
                    "createdAt": index
                }))
                .unwrap();
        }
        let clamped = wide_store.runs_page(Some(wide_id), 10_000, None);
        assert_eq!(clamped.runs.len(), MAX_RUNS_PER_AUTOMATION);
        assert_eq!(clamped.runs[0]["id"], "wide-104");
        assert_eq!(clamped.runs[99]["id"], "wide-005");
        let rest = wide_store.runs_page(Some(wide_id), 10_000, clamped.next_cursor.as_deref());
        assert_eq!(rest.runs.len(), 5);
        assert!(rest.next_cursor.is_none());
    }

    #[test]
    fn prune_runs_caps_final_runs_and_never_evicts_in_flight() {
        let dir = TestDir::new("store-prune");
        let store = load_store(&dir);
        let automation = create_automation(&store);
        let automation_id = automation["id"].as_str().unwrap();
        let other = create_automation(&store);
        let other_id = other["id"].as_str().unwrap();

        for (id, status, created_at) in [
            ("in-pending", "pending", -3),
            ("in-dispatching", "dispatching", -2),
            ("in-dispatched", "dispatched", -1),
        ] {
            store
                .create_run(json!({
                    "id": id,
                    "automationId": automation_id,
                    "scheduledFor": created_at,
                    "status": status,
                    "createdAt": created_at
                }))
                .unwrap();
        }
        for index in 0..105 {
            store
                .create_run(json!({
                    "id": format!("final-{index:03}"),
                    "automationId": automation_id,
                    "scheduledFor": 1_000 + index,
                    "status": "pending",
                    "createdAt": index
                }))
                .unwrap();
        }
        // Finish the batch without pruning: create_run prunes on insert, update_run does not.
        for index in 0..105 {
            store
                .update_run(&format!("final-{index:03}"), json!({"status": "completed"}))
                .unwrap();
        }
        store
            .create_run(json!({
                "id": "other-0",
                "automationId": other_id,
                "scheduledFor": 1,
                "status": "completed",
                "createdAt": 1
            }))
            .unwrap();
        store
            .create_run(json!({
                "id": "other-1",
                "automationId": other_id,
                "scheduledFor": 2,
                "status": "completed",
                "createdAt": 2
            }))
            .unwrap();

        store.prune_runs(automation_id);

        let runs = collect_runs(&store, Some(automation_id));
        assert_eq!(runs.len(), 103);
        let ids = run_ids(&runs);
        for in_flight in ["in-pending", "in-dispatching", "in-dispatched"] {
            assert!(
                ids.contains(&in_flight.to_string()),
                "{in_flight} was evicted"
            );
        }
        for evicted in [
            "final-000",
            "final-001",
            "final-002",
            "final-003",
            "final-004",
        ] {
            assert!(!ids.contains(&evicted.to_string()), "{evicted} survived");
        }
        assert!(ids.contains(&"final-005".to_string()));
        assert!(ids.contains(&"final-104".to_string()));
        // Other automations are untouched.
        assert_eq!(collect_runs(&store, Some(other_id)).len(), 2);
        // The prune is persisted.
        let reloaded = load_store(&dir);
        assert_eq!(collect_runs(&reloaded, Some(automation_id)).len(), 103);
    }

    #[test]
    fn corrupt_file_is_backed_up_and_the_store_recovers_empty() {
        let dir = TestDir::new("store-corrupt");
        let path = dir.file("automations.json");
        std::fs::write(&path, "not json").unwrap();

        let store = AutomationStore::load(path.clone());
        assert!(store.list().is_empty());
        assert!(store.runs_page(None, 100, None).runs.is_empty());
        let backups = dir.files_with_prefix("automations.json.corrupt-");
        assert_eq!(backups.len(), 1);
        assert_eq!(std::fs::read_to_string(&backups[0]).unwrap(), "not json");
        assert!(!path.exists());

        let (automation, created) = store.create(create_input(), 1_000).unwrap();
        assert!(created);
        let saved: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(saved["schemaVersion"], 1);
        assert_eq!(saved["automations"][0]["id"], automation["id"]);

        // A structurally wrong top level is corrupt too.
        std::fs::write(&path, "[1, 2]").unwrap();
        let store = AutomationStore::load(path.clone());
        assert!(store.list().is_empty());
        assert_eq!(dir.files_with_prefix("automations.json.corrupt-").len(), 2);
    }

    #[test]
    fn snapshot_workspace_name_updates_every_matching_run() {
        let dir = TestDir::new("store-snapshot");
        let store = load_store(&dir);
        let automation = create_automation(&store);
        let automation_id = automation["id"].as_str().unwrap();

        store
            .create_run(json!({
                "id": "wt1-old",
                "automationId": automation_id,
                "scheduledFor": 1,
                "status": "completed",
                "createdAt": 1,
                "workspaceId": "wt1"
            }))
            .unwrap();
        store
            .create_run(json!({
                "id": "wt1-new",
                "automationId": automation_id,
                "scheduledFor": 2,
                "status": "completed",
                "createdAt": 2,
                "workspaceId": "wt1"
            }))
            .unwrap();
        store
            .create_run(json!({
                "id": "wt2",
                "automationId": automation_id,
                "scheduledFor": 3,
                "status": "completed",
                "createdAt": 3,
                "workspaceId": "wt2"
            }))
            .unwrap();

        // Every run of the workspace is stamped so history stays consistent.
        assert_eq!(store.snapshot_workspace_name("wt1", "  Renamed  "), 2);
        let runs = collect_runs(&store, Some(automation_id));
        let newest = runs.iter().find(|run| run["id"] == "wt1-new").unwrap();
        let older = runs.iter().find(|run| run["id"] == "wt1-old").unwrap();
        let other = runs.iter().find(|run| run["id"] == "wt2").unwrap();
        assert_eq!(newest["workspaceDisplayName"], "Renamed");
        assert_eq!(older["workspaceDisplayName"], "Renamed");
        assert!(other.get("workspaceDisplayName").is_none());
        // Re-snapshotting the same name is a no-op.
        assert_eq!(store.snapshot_workspace_name("wt1", "Renamed"), 0);
        // Unknown workspace ids and empty names do nothing.
        assert_eq!(store.snapshot_workspace_name("missing", "X"), 0);
        assert_eq!(store.snapshot_workspace_name("wt2", "   "), 0);
        // The snapshot is persisted.
        let reloaded = load_store(&dir);
        let runs = collect_runs(&reloaded, Some(automation_id));
        let newest = runs.iter().find(|run| run["id"] == "wt1-new").unwrap();
        let older = runs.iter().find(|run| run["id"] == "wt1-old").unwrap();
        assert_eq!(newest["workspaceDisplayName"], "Renamed");
        assert_eq!(older["workspaceDisplayName"], "Renamed");
    }

    #[test]
    fn create_run_updates_parent_last_run_at_but_dedupe_does_not() {
        let dir = TestDir::new("store-touch-automation");
        let store = load_store(&dir);
        let automation = create_automation(&store);
        let automation_id = automation["id"].as_str().unwrap();
        assert_eq!(automation["updatedAt"], 1_000i64);
        assert!(automation.get("lastRunAt").is_none());

        let (run, created) = store
            .create_run(json!({
                "automationId": automation_id,
                "scheduledFor": 4_000,
                "status": "pending",
                "createdAt": 5_000
            }))
            .unwrap();
        assert!(created);
        let touched = store.get(automation_id).unwrap();
        assert_eq!(touched["lastRunAt"], 5_000i64);
        assert_eq!(touched["updatedAt"], 5_000i64);

        // A deduped insert returns the existing row and leaves the automation alone.
        let (duplicate, created) = store
            .create_run(json!({
                "automationId": automation_id,
                "scheduledFor": 4_000,
                "status": "completed",
                "createdAt": 9_999
            }))
            .unwrap();
        assert!(!created);
        assert_eq!(duplicate, run);
        let untouched = store.get(automation_id).unwrap();
        assert_eq!(untouched["lastRunAt"], 5_000i64);
        assert_eq!(untouched["updatedAt"], 5_000i64);

        // Without a caller-provided createdAt the store-assigned one is used.
        let (assigned, created) = store
            .create_run(json!({
                "automationId": automation_id,
                "scheduledFor": 6_000,
                "status": "pending"
            }))
            .unwrap();
        assert!(created);
        let assigned_at = assigned["createdAt"].as_i64().unwrap();
        assert!(assigned_at > 0);
        let touched = store.get(automation_id).unwrap();
        assert_eq!(touched["lastRunAt"], assigned_at);
        assert_eq!(touched["updatedAt"], assigned_at);

        // The touch is persisted.
        let reloaded = load_store(&dir);
        assert_eq!(
            reloaded.get(automation_id).unwrap()["lastRunAt"],
            assigned_at
        );
    }

    #[test]
    fn non_final_runs_before_filters_by_status_and_creation_cutoff() {
        let dir = TestDir::new("store-non-final");
        let store = load_store(&dir);
        let automation = create_automation(&store);
        let automation_id = automation["id"].as_str().unwrap();

        for (id, status, created_at) in [
            ("u1", "pending", 5),
            ("u2", "dispatched", 10),
            ("f1", "completed", 5),
            ("f2", "skipped_missed", 100),
        ] {
            store
                .create_run(json!({
                    "id": id,
                    "automationId": automation_id,
                    "scheduledFor": created_at,
                    "status": status,
                    "createdAt": created_at
                }))
                .unwrap();
        }

        assert_eq!(run_ids(&store.non_final_runs_before(11)), vec!["u1", "u2"]);
        assert_eq!(run_ids(&store.non_final_runs_before(6)), vec!["u1"]);
        assert!(store.non_final_runs_before(5).is_empty());
    }

    #[test]
    fn load_backfills_legacy_run_numbers_prunes_and_persists() {
        let dir = TestDir::new("store-legacy-load");
        let path = dir.file("automations.json");
        let mut runs = vec![json!({
            "id": "explicit",
            "automationId": "a1",
            "scheduledFor": 1_000,
            "status": "completed",
            "createdAt": 1_000,
            "runNumber": 200
        })];
        for index in 0..150 {
            runs.push(json!({
                "id": format!("legacy-{index:03}"),
                "automationId": "a1",
                "scheduledFor": index,
                "status": "completed",
                "createdAt": index
            }));
        }
        std::fs::write(
            &path,
            serde_json::to_string(&json!({
                "schemaVersion": 1,
                "automations": [{"id": "a1", "name": "Legacy"}],
                "runs": runs
            }))
            .unwrap(),
        )
        .unwrap();

        let store = AutomationStore::load(path.clone());
        let runs = collect_runs(&store, Some("a1"));
        assert_eq!(runs.len(), MAX_RUNS_PER_AUTOMATION);
        assert!(runs.iter().all(|run| run["runNumber"].as_i64().is_some()));
        assert_eq!(
            runs.iter().find(|run| run["id"] == "explicit").unwrap()["runNumber"],
            200
        );
        let highest = runs
            .iter()
            .map(|run| run["runNumber"].as_i64().unwrap())
            .max()
            .unwrap();
        assert_eq!(highest, 350);

        // The backfill and prune were written back to disk.
        let saved: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(saved["runs"].as_array().unwrap().len(), 100);
        assert!(saved["runs"]
            .as_array()
            .unwrap()
            .iter()
            .all(|run| run["runNumber"].as_i64().is_some()));

        // Numbering continues above the backfilled maximum, never reissuing one.
        let (next, _) = store
            .create_run(json!({
                "automationId": "a1",
                "scheduledFor": 5_000,
                "status": "completed",
                "createdAt": 5_000
            }))
            .unwrap();
        assert_eq!(next["runNumber"], 351);
    }
}
