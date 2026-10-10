//! The 60-second automation scheduler: armed-gated evaluation of due
//! automations, missed-run grace policy, next-run advance, in-memory dispatch
//! tokens, and startup reconciliation of stranded runs. Ported from
//! `orca:src/main/automations/service.ts`,
//! `orca:src/main/automations/retained-run-reconciliation.ts`,
//! `orca:src/main/automations/run-completion-watcher.ts`, and
//! `orca:src/main/automations/dispatch-tokens.ts` (local renderer dispatch
//! only; headless/SSH/external dispatch is out of scope, spec §2.2).
//!
//! Tick policy (spec §3.1): a tick only evaluates while armed by
//! `renderer_ready`, never overlaps another tick, reconciles stranded runs once
//! after a two-minute settle window, dispatches at most the latest occurrence
//! of each due automation, and always advances `nextRunAt` from `now` (never
//! backfills missed occurrences).

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::{json, Map, Value};

use crate::precheck::run_precheck;
use crate::schedule::{is_valid_schedule, latest_occurrence_at_or_before, next_occurrence_after};
use crate::store::{AutomationStore, MAX_RUNS_PER_AUTOMATION};

/// `service.ts:34` (`DEFAULT_TICK_MS`).
pub const TICK_INTERVAL: Duration = Duration::from_secs(60);
/// `retained-run-reconciliation.ts:8` (`SURFACE_SETTLE_MS`).
pub const RECONCILE_SETTLE_MS: i64 = 2 * 60 * 1000;
/// `dispatch-tokens.ts:3` (`DISPATCH_TOKEN_TTL_MS`).
pub const DISPATCH_TOKEN_TTL_MS: i64 = 30 * 60 * 1000;
/// Default missed-run grace applied when a record omits it.
pub const DEFAULT_MISSED_GRACE_MINUTES: i64 = 720;

/// `service.ts:259` (verbatim).
const MISSED_RUN_ERROR: &str = "Orca was unavailable during the missed-run grace window.";
/// `run-completion-watcher.ts:28` (verbatim).
const STRANDED_BEFORE_AGENT_START_ERROR: &str =
    "Orca stopped before this run reported that its agent started.";
/// `run-completion-watcher.ts:30` (verbatim).
const STRANDED_LOST_TERMINAL_ERROR: &str =
    "Orca lost the terminal for this run before it reported completion.";

/// Validates a renderer-supplied automation definition before it is persisted
/// (spec §3.1): non-empty `name`/`prompt`/`agentId`, a parseable schedule, a
/// millisecond `dtstart`, and — when a precheck is configured — a non-empty
/// command with a timeout inside 1..=600 seconds. Returns the renderer-facing
/// message for the first invalid field.
pub fn validate_definition(input: &Value) -> Result<(), String> {
    let object = input
        .as_object()
        .ok_or_else(|| "Automation input must be a JSON object.".to_string())?;
    require_text(object, "name")?;
    require_text(object, "prompt")?;
    require_text(object, "agentId")?;
    let rrule = require_text(object, "rrule")?;
    if !is_valid_schedule(rrule) {
        return Err("Automation schedule is not a valid cron expression or RRULE.".to_string());
    }
    if object.get("dtstart").and_then(Value::as_i64).is_none() {
        return Err("Automation dtstart must be a millisecond timestamp.".to_string());
    }
    if let Some(precheck) = object.get("precheck").filter(|value| !value.is_null()) {
        let precheck = precheck
            .as_object()
            .ok_or_else(|| "Automation precheck must be a JSON object.".to_string())?;
        require_text(precheck, "command")?;
        if let Some(timeout) = precheck
            .get("timeoutSeconds")
            .filter(|value| !value.is_null())
        {
            let seconds = timeout.as_u64().ok_or_else(|| {
                "Automation precheck timeoutSeconds must be a whole number.".to_string()
            })?;
            if !(1..=600).contains(&seconds) {
                return Err(
                    "Automation precheck timeoutSeconds must be between 1 and 600.".to_string(),
                );
            }
        }
    }
    Ok(())
}

/// A required field must be a string with non-whitespace content.
fn require_text<'a>(object: &'a Map<String, Value>, key: &str) -> Result<&'a str, String> {
    object
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .ok_or_else(|| format!("Automation {key} is required."))
}

/// Host-side event publication the bridge implements. `automations_changed`
/// fires after every store write (`"run"` or `"definition"`);
/// `dispatch_requested` carries the `AutomationDispatchRequest`
/// (`{automation, run, dispatchToken}`) the renderer consumes.
pub trait EventSink: Send + Sync {
    fn automations_changed(&self, reason: &str);
    fn dispatch_requested(&self, payload: Value);
}

pub struct AutomationService {
    store: Arc<AutomationStore>,
    sink: Arc<dyn EventSink>,
    now: fn() -> i64,
    started_at: i64,
    armed: AtomicBool,
    armed_at: Mutex<Option<i64>>,
    reconciled: AtomicBool,
    started: AtomicBool,
    evaluating: Mutex<()>,
    dispatch_tokens: Mutex<HashMap<String, i64>>,
}

impl AutomationService {
    pub fn new(
        store: Arc<AutomationStore>,
        sink: Arc<dyn EventSink>,
        now: fn() -> i64,
    ) -> Arc<Self> {
        Arc::new(Self {
            store,
            sink,
            now,
            started_at: now(),
            armed: AtomicBool::new(false),
            armed_at: Mutex::new(None),
            reconciled: AtomicBool::new(false),
            started: AtomicBool::new(false),
            evaluating: Mutex::new(()),
            dispatch_tokens: Mutex::new(HashMap::new()),
        })
    }

    /// Spawns the 60-second tick loop once. Tests drive [`Self::tick`] directly.
    pub fn start(self: &Arc<Self>) {
        if self.started.swap(true, Ordering::SeqCst) {
            return;
        }
        let service = Arc::clone(self);
        let spawned = std::thread::Builder::new()
            .name("automations-tick".to_string())
            .spawn(move || loop {
                std::thread::sleep(TICK_INTERVAL);
                service.tick();
            });
        if let Err(error) = spawned {
            eprintln!("automations service: failed to start the tick thread: {error}");
            self.started.store(false, Ordering::SeqCst);
        }
    }

    /// Arms the service and runs the catch-up tick immediately.
    pub fn renderer_ready(&self) {
        if !self.armed.swap(true, Ordering::SeqCst) {
            *lock(&self.armed_at) = Some((self.now)());
        }
        self.tick();
    }

    /// One evaluation pass: re-entrancy guarded, reconciliation first, then due
    /// automations. A tick before [`Self::renderer_ready`] does nothing.
    pub fn tick(&self) {
        let Ok(_evaluating) = self.evaluating.try_lock() else {
            return;
        };
        if !self.armed.load(Ordering::SeqCst) {
            return;
        }
        let now = (self.now)();
        self.reconcile_if_due(now);
        self.evaluate_due(now);
    }

    /// Merges `fields` into a run through the store (final statuses are
    /// guarded) and broadcasts the change.
    pub fn mark_dispatch_result(&self, run_id: &str, fields: Value) -> Option<Value> {
        match self.store.update_run(run_id, fields) {
            Ok(Some(run)) => {
                self.sink.automations_changed("run");
                Some(run)
            }
            Ok(None) => None,
            Err(error) => {
                eprintln!("automations service: failed to update run {run_id}: {error}");
                None
            }
        }
    }

    /// Manual `runNow`: creates a run (`trigger: "manual"`,
    /// `scheduledFor = now`) and publishes it through the same dispatch event a
    /// scheduled occurrence uses. Never gated by `enabled`; `None` when the
    /// automation is gone or the run could not be persisted (logged).
    ///
    /// `(automationId, scheduledFor)` still dedupes, so two manual runs in the
    /// same millisecond hand the existing row to the renderer instead of
    /// writing a second one.
    pub fn run_now(&self, automation_id: &str) -> Option<Value> {
        let automation = self.store.get(automation_id)?;
        let now = (self.now)();
        let candidate = self.build_run(&automation, now, "manual", "dispatching", None, now);
        let run = match self.store.create_run(candidate) {
            Ok((run, created)) => {
                if created {
                    self.sink.automations_changed("run");
                }
                run
            }
            Err(error) => {
                eprintln!("automations service: failed to create a manual run: {error}");
                return None;
            }
        };
        let dispatch_token = self.create_dispatch_token(now);
        self.sink.dispatch_requested(json!({
            "automation": automation,
            "run": run,
            "dispatchToken": dispatch_token,
        }));
        Some(run)
    }

    /// Runs the automation's configured precheck for a scheduled run and
    /// persists the result into the run.
    pub fn run_precheck_for(&self, automation_id: &str, run_id: &str) -> Option<Value> {
        let automation = self.store.get(automation_id)?;
        let run = self.find_run(automation_id, run_id)?;
        // service.ts:162 — only scheduled runs with a configured precheck.
        if run.get("trigger").and_then(Value::as_str) != Some("scheduled") {
            return None;
        }
        let precheck = automation
            .get("precheck")
            .filter(|value| value.is_object())?;
        let command = precheck.get("command").and_then(Value::as_str)?;
        let timeout_seconds = precheck.get("timeoutSeconds").and_then(Value::as_u64);
        let cwd = precheck_cwd(&run).or_else(|| precheck_cwd(&automation));
        let result = run_precheck(command, cwd.as_deref(), timeout_seconds);
        match self
            .store
            .update_run(run_id, json!({"precheckResult": result.clone()}))
        {
            Ok(Some(_)) => self.sink.automations_changed("run"),
            Ok(None) => {}
            Err(error) => {
                eprintln!(
                    "automations service: failed to persist precheck for run {run_id}: {error}"
                );
            }
        }
        Some(result)
    }

    /// Delegates to the store; broadcasts when runs were rewritten.
    pub fn snapshot_workspace_name(&self, workspace_id: &str, display_name: &str) -> usize {
        let updated = self
            .store
            .snapshot_workspace_name(workspace_id, display_name);
        if updated > 0 {
            self.sink.automations_changed("run");
        }
        updated
    }

    /// One-shot startup reconciliation: non-final runs created before this
    /// service started are stranded once the renderer surface has had its
    /// settle window (`retained-run-reconciliation.ts`).
    fn reconcile_if_due(&self, now: i64) {
        let Some(armed_at) = *lock(&self.armed_at) else {
            return;
        };
        if now - armed_at < RECONCILE_SETTLE_MS {
            return;
        }
        if self.reconciled.swap(true, Ordering::SeqCst) {
            return;
        }
        self.reconcile_stranded_runs();
    }

    fn reconcile_stranded_runs(&self) {
        for run in self.store.non_final_runs_before(self.started_at) {
            let Some(run_id) = run.get("id").and_then(Value::as_str) else {
                continue;
            };
            let error = describe_stranded_run(&run);
            match self
                .store
                .update_run(run_id, json!({"status": "dispatch_failed", "error": error}))
            {
                Ok(Some(_)) => self.sink.automations_changed("run"),
                Ok(None) => {}
                Err(store_error) => {
                    eprintln!(
                        "automations service: failed to reconcile run {run_id}: {store_error}"
                    );
                }
            }
        }
    }

    fn evaluate_due(&self, now: i64) {
        for automation in self.store.list() {
            if automation.get("enabled").and_then(Value::as_bool) != Some(true) {
                continue;
            }
            let Some(next_run_at) = automation.get("nextRunAt").and_then(Value::as_i64) else {
                continue;
            };
            if next_run_at > now {
                continue;
            }
            self.evaluate_automation(&automation, now);
        }
    }

    fn evaluate_automation(&self, automation: &Value, now: i64) {
        let Some(automation_id) = automation.get("id").and_then(Value::as_str) else {
            return;
        };
        let Some(scheduled_for) = latest_occurrence(automation, now) else {
            self.advance(automation_id, now);
            return;
        };
        let grace_minutes = automation
            .get("missedRunGraceMinutes")
            .and_then(Value::as_i64)
            .unwrap_or(DEFAULT_MISSED_GRACE_MINUTES);
        if now - scheduled_for > grace_minutes * 60_000 {
            self.record_missed_run(automation, scheduled_for, now);
            self.advance(automation_id, now);
            return;
        }
        if let Some(run) = self.record_due_run(automation, scheduled_for, now) {
            let dispatch_token = self.create_dispatch_token(now);
            self.sink.dispatch_requested(json!({
                "automation": automation,
                "run": run,
                "dispatchToken": dispatch_token,
            }));
        }
        self.advance(automation_id, now);
    }

    /// Creates (or dedupes onto) the run for one occurrence. Only a freshly
    /// created run is dispatched; a deduped second tick produces nothing.
    fn record_due_run(&self, automation: &Value, scheduled_for: i64, now: i64) -> Option<Value> {
        let run = self.build_run(
            automation,
            scheduled_for,
            "scheduled",
            "dispatching",
            None,
            now,
        );
        match self.store.create_run(run) {
            Ok((run, true)) => {
                self.sink.automations_changed("run");
                Some(run)
            }
            Ok((_, false)) => None,
            Err(error) => {
                eprintln!("automations service: failed to create automation run: {error}");
                None
            }
        }
    }

    fn record_missed_run(&self, automation: &Value, scheduled_for: i64, now: i64) {
        let run = self.build_run(
            automation,
            scheduled_for,
            "scheduled",
            "skipped_missed",
            Some(MISSED_RUN_ERROR),
            now,
        );
        match self.store.create_run(run) {
            Ok((_, true)) => self.sink.automations_changed("run"),
            Ok((_, false)) => {}
            Err(error) => {
                eprintln!("automations service: failed to record a missed run: {error}");
            }
        }
    }

    /// The `AutomationRun` shape the renderer expects; required nullable fields
    /// are materialized as `null`, optional ones are omitted when absent.
    fn build_run(
        &self,
        automation: &Value,
        scheduled_for: i64,
        trigger: &str,
        status: &str,
        error: Option<&str>,
        now: i64,
    ) -> Value {
        let automation_id = automation
            .get("id")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let run_number = self.next_run_number(automation_id);
        let name = automation
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or("Automation");
        let mut run = Map::new();
        run.insert("automationId".to_string(), Value::from(automation_id));
        for key in ["runContext", "sourceContext"] {
            if let Some(value) = automation.get(key).filter(|value| !value.is_null()) {
                run.insert(key.to_string(), value.clone());
            }
        }
        run.insert(
            "workspaceId".to_string(),
            automation
                .get("workspaceId")
                .cloned()
                .unwrap_or(Value::Null),
        );
        run.insert(
            "title".to_string(),
            Value::from(format!("{name} run {run_number}")),
        );
        run.insert("scheduledFor".to_string(), Value::from(scheduled_for));
        run.insert("status".to_string(), Value::from(status));
        run.insert("trigger".to_string(), Value::from(trigger));
        run.insert("sessionKind".to_string(), Value::from("terminal"));
        for key in [
            "chatSessionId",
            "terminalSessionId",
            "terminalPaneKey",
            "terminalPtyId",
            "outputSnapshot",
            "precheckResult",
            "usage",
            "error",
            "startedAt",
            "dispatchedAt",
        ] {
            run.insert(key.to_string(), Value::Null);
        }
        if let Some(error) = error {
            run.insert("error".to_string(), Value::from(error));
        }
        run.insert("createdAt".to_string(), Value::from(now));
        Value::Object(run)
    }

    /// Mirrors the store's run-number assignment so the run's title can be
    /// baked into the insert — a run marked final in the same insert (a
    /// skipped miss) can never be patched afterwards. The store stays
    /// authoritative and re-derives the same number under its lock.
    fn next_run_number(&self, automation_id: &str) -> i64 {
        let mut highest: i64 = 0;
        let mut count: i64 = 0;
        let mut cursor: Option<String> = None;
        loop {
            let page = self.store.runs_page(
                Some(automation_id),
                MAX_RUNS_PER_AUTOMATION,
                cursor.as_deref(),
            );
            for run in &page.runs {
                count += 1;
                highest = highest.max(run.get("runNumber").and_then(Value::as_i64).unwrap_or(0));
            }
            match page.next_cursor {
                Some(next) => cursor = Some(next),
                None => break,
            }
        }
        highest.max(count) + 1
    }

    /// `nextRunAt = next_occurrence_after(rrule, dtstart, now)`, no backfill.
    /// A schedule with no further occurrence parks the automation far in the
    /// future instead of re-evaluating it on every tick.
    fn advance(&self, automation_id: &str, now: i64) {
        let Some(automation) = self.store.get(automation_id) else {
            return;
        };
        let Some(rrule) = automation.get("rrule").and_then(Value::as_str) else {
            return;
        };
        let dtstart = automation
            .get("dtstart")
            .and_then(Value::as_i64)
            .unwrap_or(0);
        let next_run_at = next_occurrence_after(rrule, dtstart, now).unwrap_or(i64::MAX);
        match self
            .store
            .update(automation_id, json!({"nextRunAt": next_run_at}), now)
        {
            Ok(Some(_)) => self.sink.automations_changed("definition"),
            Ok(None) => {}
            Err(error) => {
                eprintln!(
                    "automations service: failed to advance automation {automation_id}: {error}"
                );
            }
        }
    }

    fn find_run(&self, automation_id: &str, run_id: &str) -> Option<Value> {
        let mut cursor: Option<String> = None;
        loop {
            let page = self.store.runs_page(
                Some(automation_id),
                MAX_RUNS_PER_AUTOMATION,
                cursor.as_deref(),
            );
            if let Some(run) = page
                .runs
                .into_iter()
                .find(|run| run.get("id").and_then(Value::as_str) == Some(run_id))
            {
                return Some(run);
            }
            cursor = Some(page.next_cursor?);
        }
    }

    /// In-memory dispatch token (`dispatch-tokens.ts`): a fresh UUID with a
    /// 30-minute TTL, expired entries pruned lazily on every mint. Tokens are
    /// never persisted and, in this slice, never validated (spec §7.6).
    fn create_dispatch_token(&self, now: i64) -> String {
        let token = ade_core::ids::new_uuid();
        let mut tokens = lock(&self.dispatch_tokens);
        tokens.retain(|_, expires_at| *expires_at > now);
        tokens.insert(token.clone(), now + DISPATCH_TOKEN_TTL_MS);
        token
    }
}

fn latest_occurrence(automation: &Value, now: i64) -> Option<i64> {
    let rrule = automation.get("rrule").and_then(Value::as_str)?;
    let dtstart = automation
        .get("dtstart")
        .and_then(Value::as_i64)
        .unwrap_or(0);
    latest_occurrence_at_or_before(rrule, dtstart, now)
}

/// `run-completion-watcher.ts:26-31` (verbatim sentences).
fn describe_stranded_run(run: &Value) -> &'static str {
    if run.get("status").and_then(Value::as_str) == Some("dispatching") {
        STRANDED_BEFORE_AGENT_START_ERROR
    } else {
        STRANDED_LOST_TERMINAL_ERROR
    }
}

/// The renderer resolves the target workspace itself; the only cwd it hands
/// back in this slice is the `WorkspaceRunContext.path` recorded on the run or
/// the automation.
fn precheck_cwd(record: &Value) -> Option<String> {
    record
        .get("runContext")
        .and_then(|context| context.get("path"))
        .and_then(Value::as_str)
        .filter(|path| !path.is_empty())
        .map(str::to_string)
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicI64, AtomicU64};

    static COUNTER: AtomicU64 = AtomicU64::new(0);

    struct TestDir {
        path: PathBuf,
    }

    impl TestDir {
        fn new(name: &str) -> Self {
            let unique = COUNTER.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "ade-automations-service-{name}-{}-{unique}",
                std::process::id()
            ));
            let _ = std::fs::remove_dir_all(&path);
            std::fs::create_dir_all(&path).expect("create test dir");
            Self { path }
        }

        fn file(&self, name: &str) -> PathBuf {
            self.path.join(name)
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }

    #[derive(Default)]
    struct FakeSink {
        changed: Mutex<Vec<String>>,
        dispatches: Mutex<Vec<Value>>,
        reenter: Mutex<Option<Arc<AutomationService>>>,
    }

    impl EventSink for FakeSink {
        fn automations_changed(&self, reason: &str) {
            lock(&self.changed).push(reason.to_string());
        }

        fn dispatch_requested(&self, payload: Value) {
            lock(&self.dispatches).push(payload);
            let service = lock(&self.reenter).clone();
            if let Some(service) = service {
                service.tick();
            }
        }
    }

    fn local_ms(year: i32, month: u32, day: u32, hour: u32, minute: u32) -> i64 {
        use chrono::{Local, TimeZone};
        Local
            .with_ymd_and_hms(year, month, day, hour, minute, 0)
            .single()
            .expect("local time")
            .timestamp_millis()
    }

    fn load_store(dir: &TestDir) -> Arc<AutomationStore> {
        Arc::new(AutomationStore::load(dir.file("automations.json")))
    }

    fn automation_input(rrule: &str, dtstart: i64, next_run_at: i64) -> Value {
        json!({
            "name": "Nightly",
            "prompt": "Run checks",
            "agentId": "claude",
            "projectId": "r1",
            "workspaceMode": "existing",
            "workspaceId": "ws-1",
            "timezone": "UTC",
            "rrule": rrule,
            "dtstart": dtstart,
            "nextRunAt": next_run_at,
        })
    }

    fn runs_of(store: &AutomationStore, automation_id: Option<&str>) -> Vec<Value> {
        let mut runs = Vec::new();
        let mut cursor: Option<String> = None;
        loop {
            let page = store.runs_page(automation_id, MAX_RUNS_PER_AUTOMATION, cursor.as_deref());
            runs.extend(page.runs);
            match page.next_cursor {
                Some(next) => cursor = Some(next),
                None => break,
            }
        }
        runs
    }

    fn changed_reasons(sink: &FakeSink) -> Vec<String> {
        lock(&sink.changed).clone()
    }

    #[test]
    fn service_unarmed_tick_neither_dispatches_nor_writes_runs() {
        static NOW_MS: AtomicI64 = AtomicI64::new(0);
        fn now() -> i64 {
            NOW_MS.load(Ordering::SeqCst)
        }

        let now_ms = local_ms(2026, 10, 9, 12, 34);
        NOW_MS.store(now_ms, Ordering::SeqCst);
        let dir = TestDir::new("unarmed");
        let store = load_store(&dir);
        let scheduled_for = local_ms(2026, 10, 9, 12, 0);
        let automation = store
            .create(
                automation_input(
                    "FREQ=HOURLY;BYMINUTE=0",
                    local_ms(2026, 10, 9, 0, 0),
                    scheduled_for,
                ),
                now_ms,
            )
            .unwrap()
            .0;
        let automation_id = automation["id"].as_str().unwrap().to_string();
        let sink = Arc::new(FakeSink::default());
        let service = AutomationService::new(store.clone(), sink.clone(), now);

        service.tick();

        assert!(lock(&sink.dispatches).is_empty());
        assert!(changed_reasons(&sink).is_empty());
        assert!(runs_of(&store, Some(&automation_id)).is_empty());
        // No advance either: the unarmed tick must be a complete no-op.
        assert_eq!(
            store.get(&automation_id).unwrap()["nextRunAt"],
            scheduled_for
        );
    }

    #[test]
    fn service_armed_due_tick_dispatches_once_and_advances() {
        static NOW_MS: AtomicI64 = AtomicI64::new(0);
        fn now() -> i64 {
            NOW_MS.load(Ordering::SeqCst)
        }

        let now_ms = local_ms(2026, 10, 9, 12, 34);
        NOW_MS.store(now_ms, Ordering::SeqCst);
        let dir = TestDir::new("due");
        let store = load_store(&dir);
        let rrule = "FREQ=HOURLY;BYMINUTE=0";
        let dtstart = local_ms(2026, 10, 9, 0, 0);
        let scheduled_for = local_ms(2026, 10, 9, 12, 0);
        let automation = store
            .create(automation_input(rrule, dtstart, scheduled_for), now_ms)
            .unwrap()
            .0;
        let automation_id = automation["id"].as_str().unwrap().to_string();
        let sink = Arc::new(FakeSink::default());
        let service = AutomationService::new(store.clone(), sink.clone(), now);

        service.renderer_ready();

        let dispatches = lock(&sink.dispatches);
        assert_eq!(dispatches.len(), 1);
        let payload = &dispatches[0];
        assert_eq!(payload["automation"]["id"], automation["id"]);
        let run = &payload["run"];
        assert_eq!(run["automationId"], automation["id"]);
        assert_eq!(run["status"], "dispatching");
        assert_eq!(run["trigger"], "scheduled");
        assert_eq!(run["title"], "Nightly run 1");
        assert_eq!(run["runNumber"], 1);
        assert_eq!(run["scheduledFor"], scheduled_for);
        assert_eq!(run["createdAt"], now_ms);
        assert_eq!(run["workspaceId"], "ws-1");
        assert_eq!(run["sessionKind"], "terminal");
        for key in [
            "chatSessionId",
            "terminalSessionId",
            "terminalPaneKey",
            "terminalPtyId",
            "outputSnapshot",
            "precheckResult",
            "usage",
            "error",
            "startedAt",
            "dispatchedAt",
        ] {
            assert!(
                run.get(key).is_some_and(Value::is_null),
                "{key} must be materialized null"
            );
        }
        let run_id = run["id"].as_str().expect("run id").to_string();
        assert!(payload["dispatchToken"]
            .as_str()
            .is_some_and(|token| !token.is_empty()));
        drop(dispatches);

        let stored = runs_of(&store, Some(&automation_id));
        assert_eq!(stored.len(), 1);
        assert_eq!(stored[0]["id"], run_id.as_str());
        assert_eq!(stored[0]["title"], "Nightly run 1");
        assert_eq!(stored[0]["status"], "dispatching");
        assert_eq!(stored[0]["scheduledFor"], scheduled_for);

        let advanced = store.get(&automation_id).unwrap()["nextRunAt"]
            .as_i64()
            .unwrap();
        assert_eq!(
            advanced,
            next_occurrence_after(rrule, dtstart, now_ms).unwrap()
        );
        assert!(advanced > now_ms);
        let reasons = changed_reasons(&sink);
        assert!(reasons.contains(&"run".to_string()));
        assert!(reasons.contains(&"definition".to_string()));
    }

    #[test]
    fn service_missed_beyond_grace_records_one_skipped_missed_and_advances() {
        static NOW_MS: AtomicI64 = AtomicI64::new(0);
        fn now() -> i64 {
            NOW_MS.load(Ordering::SeqCst)
        }

        let now_ms = local_ms(2026, 10, 9, 12, 34);
        NOW_MS.store(now_ms, Ordering::SeqCst);
        let dir = TestDir::new("missed");
        let store = load_store(&dir);
        let rrule = "FREQ=HOURLY;BYMINUTE=0";
        let dtstart = local_ms(2026, 10, 9, 0, 0);
        let scheduled_for = local_ms(2026, 10, 9, 12, 0);
        let mut input = automation_input(rrule, dtstart, scheduled_for);
        input["missedRunGraceMinutes"] = json!(1);
        let automation = store.create(input, now_ms).unwrap().0;
        let automation_id = automation["id"].as_str().unwrap().to_string();
        let sink = Arc::new(FakeSink::default());
        let service = AutomationService::new(store.clone(), sink.clone(), now);

        service.renderer_ready();

        assert!(lock(&sink.dispatches).is_empty());
        let stored = runs_of(&store, Some(&automation_id));
        assert_eq!(stored.len(), 1);
        assert_eq!(stored[0]["status"], "skipped_missed");
        assert_eq!(stored[0]["trigger"], "scheduled");
        assert_eq!(stored[0]["title"], "Nightly run 1");
        assert_eq!(stored[0]["scheduledFor"], scheduled_for);
        assert_eq!(
            stored[0]["error"],
            "Orca was unavailable during the missed-run grace window."
        );
        assert_eq!(
            store.get(&automation_id).unwrap()["nextRunAt"],
            next_occurrence_after(rrule, dtstart, now_ms).unwrap()
        );
        let reasons = changed_reasons(&sink);
        assert!(reasons.contains(&"run".to_string()));
        assert!(reasons.contains(&"definition".to_string()));
    }

    #[test]
    fn service_duplicate_occurrence_tick_creates_nothing_new() {
        static NOW_MS: AtomicI64 = AtomicI64::new(0);
        fn now() -> i64 {
            NOW_MS.load(Ordering::SeqCst)
        }

        let now_ms = local_ms(2026, 10, 9, 12, 34);
        NOW_MS.store(now_ms, Ordering::SeqCst);
        let dir = TestDir::new("dedupe");
        let store = load_store(&dir);
        let scheduled_for = local_ms(2026, 10, 9, 12, 0);
        let automation = store
            .create(
                automation_input(
                    "FREQ=HOURLY;BYMINUTE=0",
                    local_ms(2026, 10, 9, 0, 0),
                    scheduled_for,
                ),
                now_ms,
            )
            .unwrap()
            .0;
        let automation_id = automation["id"].as_str().unwrap().to_string();
        let sink = Arc::new(FakeSink::default());
        let service = AutomationService::new(store.clone(), sink.clone(), now);

        service.renderer_ready();
        assert_eq!(lock(&sink.dispatches).len(), 1);

        // Re-arm the same occurrence (as a crash before advance would leave it)
        // and tick again: the `(automationId, scheduledFor)` dedupe must win.
        store
            .update(&automation_id, json!({"nextRunAt": scheduled_for}), now_ms)
            .unwrap();
        service.tick();

        assert_eq!(lock(&sink.dispatches).len(), 1);
        assert_eq!(runs_of(&store, Some(&automation_id)).len(), 1);
    }

    #[test]
    fn service_disabled_automation_never_schedules_but_manual_runs_still_create() {
        static NOW_MS: AtomicI64 = AtomicI64::new(0);
        fn now() -> i64 {
            NOW_MS.load(Ordering::SeqCst)
        }

        let now_ms = local_ms(2026, 10, 9, 12, 34);
        NOW_MS.store(now_ms, Ordering::SeqCst);
        let dir = TestDir::new("disabled");
        let store = load_store(&dir);
        let scheduled_for = local_ms(2026, 10, 9, 12, 0);
        let mut input = automation_input(
            "FREQ=HOURLY;BYMINUTE=0",
            local_ms(2026, 10, 9, 0, 0),
            scheduled_for,
        );
        input["enabled"] = json!(false);
        let automation = store.create(input, now_ms).unwrap().0;
        let automation_id = automation["id"].as_str().unwrap().to_string();
        let sink = Arc::new(FakeSink::default());
        let service = AutomationService::new(store.clone(), sink.clone(), now);

        service.renderer_ready();

        assert!(lock(&sink.dispatches).is_empty());
        assert!(runs_of(&store, Some(&automation_id)).is_empty());
        // The disabled flag survives; nothing advanced it.
        assert_eq!(
            store.get(&automation_id).unwrap()["nextRunAt"],
            scheduled_for
        );

        // runNow semantics live in the store and are not gated by `enabled`.
        let (manual, created) = store
            .create_run(json!({
                "automationId": automation_id,
                "scheduledFor": now_ms,
                "status": "dispatching",
                "trigger": "manual",
                "createdAt": now_ms,
            }))
            .unwrap();
        assert!(created);
        assert_eq!(manual["trigger"], "manual");

        service.tick();
        assert!(lock(&sink.dispatches).is_empty());
        assert_eq!(runs_of(&store, Some(&automation_id)).len(), 1);
    }

    #[test]
    fn service_mark_dispatch_result_merges_and_guards_final_statuses() {
        static NOW_MS: AtomicI64 = AtomicI64::new(0);
        fn now() -> i64 {
            NOW_MS.load(Ordering::SeqCst)
        }

        let now_ms = local_ms(2026, 10, 9, 12, 34);
        NOW_MS.store(now_ms, Ordering::SeqCst);
        let dir = TestDir::new("mark-result");
        let store = load_store(&dir);
        let automation = store
            .create(
                automation_input(
                    "FREQ=HOURLY;BYMINUTE=0",
                    local_ms(2026, 10, 9, 0, 0),
                    now_ms,
                ),
                now_ms,
            )
            .unwrap()
            .0;
        let automation_id = automation["id"].as_str().unwrap().to_string();
        let (run, _) = store
            .create_run(json!({
                "automationId": automation_id,
                "scheduledFor": 1,
                "status": "dispatching",
                "trigger": "scheduled",
                "createdAt": now_ms,
            }))
            .unwrap();
        let run_id = run["id"].as_str().unwrap().to_string();
        let sink = Arc::new(FakeSink::default());
        let service = AutomationService::new(store.clone(), sink.clone(), now);

        let dispatched = service
            .mark_dispatch_result(
                &run_id,
                json!({"status": "dispatched", "terminalSessionId": "t1", "error": null}),
            )
            .expect("run exists");
        assert_eq!(dispatched["status"], "dispatched");
        assert_eq!(dispatched["terminalSessionId"], "t1");
        assert!(dispatched.get("error").is_none());

        let completed = service
            .mark_dispatch_result(
                &run_id,
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
            .expect("run exists");
        assert_eq!(completed["status"], "completed");

        // A final run must never be overwritten by a late write.
        let guarded = service
            .mark_dispatch_result(
                &run_id,
                json!({"status": "dispatching", "terminalSessionId": "t2"}),
            )
            .expect("run exists");
        assert_eq!(guarded["status"], "completed");
        assert_eq!(guarded["terminalSessionId"], "t1");

        assert!(service
            .mark_dispatch_result("missing-run", json!({"status": "completed"}))
            .is_none());
        assert!(changed_reasons(&sink).len() >= 3);
    }

    #[test]
    fn service_reconciliation_strands_pre_start_runs_after_the_settle_window() {
        static NOW_MS: AtomicI64 = AtomicI64::new(0);
        fn now() -> i64 {
            NOW_MS.load(Ordering::SeqCst)
        }

        let started_at = local_ms(2026, 10, 9, 12, 0);
        NOW_MS.store(started_at, Ordering::SeqCst);
        let dir = TestDir::new("reconcile");
        let store = load_store(&dir);
        // Runs left behind by the process that died before this service started.
        for (index, (id, status, created_at)) in [
            ("run-dispatching", "dispatching", started_at - 10_000),
            ("run-dispatched", "dispatched", started_at - 5_000),
            ("run-final", "completed", started_at - 5_000),
        ]
        .into_iter()
        .enumerate()
        {
            store
                .create_run(json!({
                    "id": id,
                    "automationId": "auto-1",
                    "scheduledFor": index as i64 + 1,
                    "status": status,
                    "trigger": "scheduled",
                    "createdAt": created_at,
                }))
                .unwrap();
        }
        let sink = Arc::new(FakeSink::default());
        let service = AutomationService::new(store.clone(), sink.clone(), now);

        service.renderer_ready();
        assert_eq!(lock(&sink.dispatches).len(), 0);

        // Inside the settle window nothing is stranded yet.
        NOW_MS.store(started_at + 119_000, Ordering::SeqCst);
        service.tick();
        let runs = runs_of(&store, Some("auto-1"));
        let find = |id: &str| {
            runs.iter()
                .find(|run| run["id"] == id)
                .expect("run present")
                .clone()
        };
        assert_eq!(find("run-dispatching")["status"], "dispatching");
        assert_eq!(find("run-dispatched")["status"], "dispatched");

        // After the two-minute settle the retained runs are closed out once.
        NOW_MS.store(started_at + 120_000, Ordering::SeqCst);
        service.tick();
        let runs = runs_of(&store, Some("auto-1"));
        let find = |id: &str| {
            runs.iter()
                .find(|run| run["id"] == id)
                .expect("run present")
                .clone()
        };
        assert_eq!(find("run-dispatching")["status"], "dispatch_failed");
        assert_eq!(
            find("run-dispatching")["error"],
            "Orca stopped before this run reported that its agent started."
        );
        assert_eq!(find("run-dispatched")["status"], "dispatch_failed");
        assert_eq!(
            find("run-dispatched")["error"],
            "Orca lost the terminal for this run before it reported completion."
        );
        // A run that already reached a final status is untouched.
        assert_eq!(find("run-final")["status"], "completed");

        // A run created after this service started is not a candidate.
        store
            .create_run(json!({
                    "id": "run-new",
                    "automationId": "auto-1",
                    "scheduledFor": 4,
                "status": "dispatching",
                "trigger": "scheduled",
                "createdAt": started_at + 1,
            }))
            .unwrap();
        NOW_MS.store(started_at + 600_000, Ordering::SeqCst);
        service.tick();
        let runs = runs_of(&store, Some("auto-1"));
        let find = |id: &str| {
            runs.iter()
                .find(|run| run["id"] == id)
                .expect("run present")
                .clone()
        };
        // Reconciliation is a one-shot pass.
        assert_eq!(find("run-dispatching")["status"], "dispatch_failed");
        assert_eq!(find("run-new")["status"], "dispatching");
        assert!(changed_reasons(&sink).iter().all(|reason| reason == "run"));
    }

    #[test]
    fn service_tick_returns_while_another_tick_is_evaluating() {
        static NOW_MS: AtomicI64 = AtomicI64::new(0);
        fn now() -> i64 {
            NOW_MS.load(Ordering::SeqCst)
        }

        let now_ms = local_ms(2026, 10, 9, 12, 34);
        NOW_MS.store(now_ms, Ordering::SeqCst);
        let dir = TestDir::new("reentrant");
        let store = load_store(&dir);
        let automation = store
            .create(
                automation_input(
                    "FREQ=HOURLY;BYMINUTE=0",
                    local_ms(2026, 10, 9, 0, 0),
                    local_ms(2026, 10, 9, 12, 0),
                ),
                now_ms,
            )
            .unwrap()
            .0;
        let automation_id = automation["id"].as_str().unwrap().to_string();
        let sink = Arc::new(FakeSink::default());
        let service = AutomationService::new(store.clone(), sink.clone(), now);
        service.renderer_ready();

        // Simulate an in-flight tick: a re-entrant tick must return immediately.
        let guard = lock(&service.evaluating);
        store
            .update(
                &automation_id,
                json!({"nextRunAt": local_ms(2026, 10, 9, 12, 0)}),
                now_ms,
            )
            .unwrap();
        service.tick();
        assert_eq!(lock(&sink.dispatches).len(), 1);
        assert_eq!(runs_of(&store, Some(&automation_id)).len(), 1);
        drop(guard);

        service.tick();
        // The second tick dedupes the occurrence; no second dispatch happened
        // while the first was still evaluating.
        assert_eq!(lock(&sink.dispatches).len(), 1);
    }

    #[test]
    fn service_reentrant_dispatch_callback_does_not_dispatch_twice() {
        static NOW_MS: AtomicI64 = AtomicI64::new(0);
        fn now() -> i64 {
            NOW_MS.load(Ordering::SeqCst)
        }

        let now_ms = local_ms(2026, 10, 9, 12, 34);
        NOW_MS.store(now_ms, Ordering::SeqCst);
        let dir = TestDir::new("reentrant-sink");
        let store = load_store(&dir);
        let automation = store
            .create(
                automation_input(
                    "FREQ=HOURLY;BYMINUTE=0",
                    local_ms(2026, 10, 9, 0, 0),
                    local_ms(2026, 10, 9, 12, 0),
                ),
                now_ms,
            )
            .unwrap()
            .0;
        let automation_id = automation["id"].as_str().unwrap().to_string();
        let sink = Arc::new(FakeSink::default());
        let service = AutomationService::new(store.clone(), sink.clone(), now);
        *lock(&sink.reenter) = Some(service.clone());

        // The sink re-enters `tick` from inside the dispatch callback.
        service.renderer_ready();

        assert_eq!(lock(&sink.dispatches).len(), 1);
        assert_eq!(runs_of(&store, Some(&automation_id)).len(), 1);
    }

    #[test]
    fn service_run_precheck_for_executes_and_persists_the_result() {
        static NOW_MS: AtomicI64 = AtomicI64::new(0);
        fn now() -> i64 {
            NOW_MS.load(Ordering::SeqCst)
        }

        let now_ms = local_ms(2026, 10, 9, 12, 34);
        NOW_MS.store(now_ms, Ordering::SeqCst);
        let dir = TestDir::new("precheck");
        let store = load_store(&dir);
        let mut input = automation_input(
            "FREQ=HOURLY;BYMINUTE=0",
            local_ms(2026, 10, 9, 0, 0),
            now_ms,
        );
        input["precheck"] = json!({"command": "printf ok", "timeoutSeconds": 5});
        let automation = store.create(input, now_ms).unwrap().0;
        let automation_id = automation["id"].as_str().unwrap().to_string();
        let (run, _) = store
            .create_run(json!({
                "automationId": automation_id,
                "scheduledFor": 1,
                "status": "dispatching",
                "trigger": "scheduled",
                "createdAt": now_ms,
            }))
            .unwrap();
        let run_id = run["id"].as_str().unwrap().to_string();
        let sink = Arc::new(FakeSink::default());
        let service = AutomationService::new(store.clone(), sink.clone(), now);

        let result = service
            .run_precheck_for(&automation_id, &run_id)
            .expect("precheck runs");
        assert_eq!(result["command"], "printf ok");
        assert_eq!(result["exitCode"], 0);
        assert_eq!(result["stdout"], "ok");
        assert_eq!(result["timedOut"], false);
        assert!(result.get("error").is_some_and(Value::is_null));
        let stored = runs_of(&store, Some(&automation_id));
        assert_eq!(stored[0]["precheckResult"], result);
        assert!(changed_reasons(&sink).contains(&"run".to_string()));

        // Manual runs are never prechecked (service.ts:162).
        let (manual, _) = store
            .create_run(json!({
                "automationId": automation_id,
                "scheduledFor": 2,
                "status": "dispatching",
                "trigger": "manual",
                "createdAt": now_ms,
            }))
            .unwrap();
        let manual_id = manual["id"].as_str().unwrap();
        assert!(service
            .run_precheck_for(&automation_id, manual_id)
            .is_none());

        assert!(service
            .run_precheck_for(&automation_id, "missing-run")
            .is_none());
        assert!(service
            .run_precheck_for("missing-automation", &run_id)
            .is_none());
    }

    #[test]
    fn service_run_precheck_for_uses_the_run_workspace_cwd() {
        static NOW_MS: AtomicI64 = AtomicI64::new(0);
        fn now() -> i64 {
            NOW_MS.load(Ordering::SeqCst)
        }

        let now_ms = local_ms(2026, 10, 9, 12, 34);
        NOW_MS.store(now_ms, Ordering::SeqCst);
        let dir = TestDir::new("precheck-cwd");
        let store = load_store(&dir);
        let mut input = automation_input(
            "FREQ=HOURLY;BYMINUTE=0",
            local_ms(2026, 10, 9, 0, 0),
            now_ms,
        );
        input["precheck"] = json!({"command": "pwd -P", "timeoutSeconds": 5});
        let automation = store.create(input, now_ms).unwrap().0;
        let automation_id = automation["id"].as_str().unwrap().to_string();
        let (run, _) = store
            .create_run(json!({
                "automationId": automation_id,
                "scheduledFor": 1,
                "status": "dispatching",
                "trigger": "scheduled",
                "createdAt": now_ms,
                "runContext": {
                    "kind": "workspace-run",
                    "projectId": "p1",
                    "hostId": "local",
                    "projectHostSetupId": "s1",
                    "repoId": "r1",
                    "path": dir.path.to_string_lossy(),
                },
            }))
            .unwrap();
        let run_id = run["id"].as_str().unwrap().to_string();
        let sink = Arc::new(FakeSink::default());
        let service = AutomationService::new(store.clone(), sink.clone(), now);

        let result = service
            .run_precheck_for(&automation_id, &run_id)
            .expect("precheck runs");
        assert_eq!(result["exitCode"], 0);
        let expected = std::fs::canonicalize(&dir.path).unwrap();
        assert_eq!(
            result["stdout"].as_str().unwrap().trim(),
            expected.to_string_lossy()
        );
    }

    #[test]
    fn service_snapshot_workspace_name_delegates_and_broadcasts() {
        static NOW_MS: AtomicI64 = AtomicI64::new(0);
        fn now() -> i64 {
            NOW_MS.load(Ordering::SeqCst)
        }

        let now_ms = local_ms(2026, 10, 9, 12, 34);
        NOW_MS.store(now_ms, Ordering::SeqCst);
        let dir = TestDir::new("snapshot");
        let store = load_store(&dir);
        store
            .create_run(json!({
                "id": "run-1",
                "automationId": "auto-1",
                "scheduledFor": 1,
                "status": "completed",
                "trigger": "scheduled",
                "createdAt": now_ms,
                "workspaceId": "ws-1",
            }))
            .unwrap();
        let sink = Arc::new(FakeSink::default());
        let service = AutomationService::new(store.clone(), sink.clone(), now);

        assert_eq!(service.snapshot_workspace_name("ws-1", "Nice Name"), 1);
        let runs = runs_of(&store, Some("auto-1"));
        assert_eq!(runs[0]["workspaceDisplayName"], "Nice Name");
        assert!(changed_reasons(&sink).contains(&"run".to_string()));
        assert_eq!(service.snapshot_workspace_name("ws-2", "Other"), 0);
    }
}
