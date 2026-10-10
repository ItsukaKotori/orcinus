//! Integration coverage for the automations command surface: definition
//! validation, next-run computation, run-now dispatch, paging, the terminal
//! guard on dispatch results, precheck persistence, and workspace snapshots.
//!
//! Mirrors `git_commands.rs`: state-free `*_impl` helpers over a temp-dir
//! store (and a service with a recording sink), no Tauri runtime.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use ade_automations::{next_occurrence_after, AutomationService, AutomationStore, EventSink};
use ade_bridge::commands::automations::{
    create_impl, delete_impl, list_impl, mark_dispatch_result_impl, run_now_impl,
    run_precheck_impl, runs_impl, snapshot_workspace_name_impl, update_impl, AutomationsRunsArgs,
};
use serde_json::{json, Value};

/// Fixed wall-clock used by every impl call and by the service's injected
/// clock, so run-now's `scheduledFor`/`createdAt` are deterministic.
const TEST_NOW: i64 = 1_700_000_000_000;

fn service_now() -> i64 {
    TEST_NOW
}

struct TestDir {
    path: PathBuf,
}

impl TestDir {
    fn new(name: &str) -> Self {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let unique = COUNTER.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "ade-bridge-automations-{name}-{}-{unique}",
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
struct RecordingSink {
    changed: Mutex<Vec<String>>,
    dispatches: Mutex<Vec<Value>>,
}

impl RecordingSink {
    fn changed(&self) -> Vec<String> {
        self.changed.lock().unwrap().clone()
    }

    fn dispatches(&self) -> Vec<Value> {
        self.dispatches.lock().unwrap().clone()
    }
}

impl EventSink for RecordingSink {
    fn automations_changed(&self, reason: &str) {
        self.changed.lock().unwrap().push(reason.to_string());
    }

    fn dispatch_requested(&self, payload: Value) {
        self.dispatches.lock().unwrap().push(payload);
    }
}

fn load_store(dir: &TestDir) -> Arc<AutomationStore> {
    Arc::new(AutomationStore::load(dir.file("automations.json")))
}

fn service_for(store: &Arc<AutomationStore>) -> (Arc<AutomationService>, Arc<RecordingSink>) {
    let sink = Arc::new(RecordingSink::default());
    (
        AutomationService::new(Arc::clone(store), sink.clone(), service_now),
        sink,
    )
}

fn definition(rrule: &str, dtstart: i64) -> Value {
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
    })
}

fn created_id(store: &Arc<AutomationStore>, rrule: &str) -> String {
    create_impl(store, definition(rrule, TEST_NOW - 3_600_000), TEST_NOW).unwrap()["id"]
        .as_str()
        .expect("automation id")
        .to_string()
}

#[test]
fn create_persists_a_valid_definition_and_computes_next_run() {
    let dir = TestDir::new("create-valid");
    let store = load_store(&dir);
    let rrule = "FREQ=HOURLY;BYMINUTE=0";
    let dtstart = TEST_NOW - 3_600_000;

    let automation = create_impl(&store, definition(rrule, dtstart), TEST_NOW).unwrap();

    assert_eq!(automation["name"], "Nightly");
    assert_eq!(automation["enabled"], true);
    assert_eq!(automation["missedRunGraceMinutes"], 720);
    let expected = next_occurrence_after(rrule, dtstart, TEST_NOW).unwrap();
    assert_eq!(automation["nextRunAt"], expected);
    assert!(expected > TEST_NOW);

    // Reload from disk: the definition and its computed nextRunAt survive.
    let reloaded = AutomationStore::load(dir.file("automations.json"));
    assert_eq!(reloaded.list().len(), 1);
    let stored = reloaded
        .get(automation["id"].as_str().unwrap())
        .expect("persisted automation");
    assert_eq!(stored["nextRunAt"], expected);
    assert_eq!(list_impl(&store).len(), 1);
    assert_eq!(list_impl(&store)[0]["id"], automation["id"]);
}

#[test]
fn create_stamps_the_local_execution_target_when_absent() {
    let dir = TestDir::new("create-target-default");
    let store = load_store(&dir);

    let automation = create_impl(
        &store,
        definition("FREQ=HOURLY;BYMINUTE=0", TEST_NOW - 3_600_000),
        TEST_NOW,
    )
    .unwrap();

    assert_eq!(automation["executionTargetType"], "local");
    assert_eq!(automation["executionTargetId"], "local");
}

#[test]
fn create_preserves_an_explicit_execution_target() {
    let dir = TestDir::new("create-target-explicit");
    let store = load_store(&dir);
    let mut input = definition("FREQ=HOURLY;BYMINUTE=0", TEST_NOW - 3_600_000);
    input["executionTargetType"] = json!("ssh");
    input["executionTargetId"] = json!("x");

    let automation = create_impl(&store, input, TEST_NOW).unwrap();

    assert_eq!(automation["executionTargetType"], "ssh");
    assert_eq!(automation["executionTargetId"], "x");
}

#[test]
fn create_rejects_invalid_definitions_without_persisting() {
    let dir = TestDir::new("create-invalid");
    let store = load_store(&dir);
    let valid = definition("FREQ=HOURLY;BYMINUTE=0", TEST_NOW - 3_600_000);

    for (label, broken) in [
        ("blank name", json!({"name": "   "})),
        ("empty prompt", json!({"prompt": ""})),
        ("empty agentId", json!({"agentId": ""})),
        ("unparseable rrule", json!({"rrule": "not a schedule"})),
        ("impossible cron", json!({"rrule": "0 0 31 2 *"})),
        ("missing dtstart", json!({"dtstart": null})),
        (
            "precheck timeout zero",
            json!({"precheck": {"command": "echo ok", "timeoutSeconds": 0}}),
        ),
        (
            "precheck timeout over cap",
            json!({"precheck": {"command": "echo ok", "timeoutSeconds": 601}}),
        ),
        (
            "blank precheck command",
            json!({"precheck": {"command": " "}}),
        ),
    ] {
        let mut input = valid.clone();
        let object = input.as_object_mut().unwrap();
        for (key, value) in broken.as_object().unwrap() {
            object.insert(key.clone(), value.clone());
        }
        let error = create_impl(&store, input, TEST_NOW).unwrap_err();
        assert!(!error.to_string().is_empty(), "{label} must be rejected");
    }

    assert!(create_impl(&store, json!("not an object"), TEST_NOW).is_err());
    assert!(store.list().is_empty(), "invalid input must not be stored");
    assert!(
        !dir.file("automations.json").exists(),
        "rejected input must not touch the store file"
    );
}

#[test]
fn update_validates_and_recomputes_next_run_when_the_schedule_changes() {
    let dir = TestDir::new("update");
    let store = load_store(&dir);
    let dtstart = TEST_NOW - 3_600_000;
    let rrule = "FREQ=HOURLY;BYMINUTE=0";
    let id = created_id(&store, rrule);
    let original_next = store.get(&id).unwrap()["nextRunAt"].as_i64().unwrap();

    // A name-only update keeps the schedule untouched.
    let renamed = update_impl(&store, &id, json!({"name": "Renamed"}), TEST_NOW)
        .unwrap()
        .expect("automation exists");
    assert_eq!(renamed["name"], "Renamed");
    assert_eq!(renamed["nextRunAt"], original_next);

    // An invalid schedule is rejected and nothing is persisted.
    assert!(update_impl(&store, &id, json!({"rrule": "nope"}), TEST_NOW).is_err());
    assert_eq!(store.get(&id).unwrap()["rrule"], rrule);
    // Removing the schedule out from under the automation is rejected too.
    assert!(update_impl(&store, &id, json!({"rrule": null}), TEST_NOW).is_err());

    // A schedule change recomputes nextRunAt from the new rrule and now.
    let new_rrule = "FREQ=DAILY;BYHOUR=9;BYMINUTE=30";
    let later = TEST_NOW + 86_400_000;
    let updated = update_impl(&store, &id, json!({"rrule": new_rrule}), later)
        .unwrap()
        .expect("automation exists");
    let expected = next_occurrence_after(new_rrule, dtstart, later).unwrap();
    assert_eq!(updated["nextRunAt"], expected);

    // Invalid precheck updates are rejected as well.
    assert!(update_impl(
        &store,
        &id,
        json!({"precheck": {"command": "echo ok", "timeoutSeconds": 0}}),
        TEST_NOW,
    )
    .is_err());

    // Unknown ids report absence instead of failing.
    assert!(
        update_impl(&store, "missing", json!({"name": "x"}), TEST_NOW)
            .unwrap()
            .is_none()
    );
}

#[test]
fn delete_cascades_runs_and_reports_idempotence() {
    let dir = TestDir::new("delete");
    let store = load_store(&dir);
    let id = created_id(&store, "FREQ=HOURLY;BYMINUTE=0");
    store
        .create_run(json!({
            "automationId": id.as_str(),
            "scheduledFor": TEST_NOW - 3_600_000,
            "status": "completed",
            "trigger": "scheduled",
        }))
        .unwrap();

    let (runs, cursor) = runs_impl(&store, Some(id.as_str()), None, None);
    assert_eq!(runs.len(), 1);
    assert_eq!(cursor, None);

    assert!(delete_impl(&store, &id).unwrap());
    assert!(store.list().is_empty());
    let (runs, _) = runs_impl(&store, None, None, None);
    assert!(runs.is_empty(), "runs cascade with their automation");
    assert!(
        !delete_impl(&store, &id).unwrap(),
        "a second delete is a no-op"
    );
}

#[test]
fn run_now_creates_a_manual_run_and_dispatches_even_when_disabled() {
    let dir = TestDir::new("run-now");
    let store = load_store(&dir);
    let mut input = definition("FREQ=HOURLY;BYMINUTE=0", TEST_NOW - 3_600_000);
    input["enabled"] = json!(false);
    let id = create_impl(&store, input, TEST_NOW).unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();
    let (service, sink) = service_for(&store);

    let run = run_now_impl(&service, &id).unwrap();

    assert_eq!(run["automationId"], id.as_str());
    assert_eq!(run["trigger"], "manual");
    assert_eq!(run["status"], "dispatching");
    assert_eq!(run["scheduledFor"], TEST_NOW);
    assert_eq!(run["title"], "Nightly run 1");
    assert_eq!(run["createdAt"], TEST_NOW);

    let dispatches = sink.dispatches();
    assert_eq!(dispatches.len(), 1);
    assert_eq!(dispatches[0]["automation"]["id"], id.as_str());
    assert_eq!(dispatches[0]["run"]["id"], run["id"]);
    assert!(dispatches[0]["dispatchToken"]
        .as_str()
        .is_some_and(|token| !token.is_empty()));
    assert!(sink.changed().contains(&"run".to_string()));

    let (runs, _) = runs_impl(&store, Some(id.as_str()), None, None);
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0]["trigger"], "manual");
    assert_eq!(runs[0]["status"], "dispatching");

    let error = run_now_impl(&service, "missing").unwrap_err();
    assert!(
        error.to_string().contains("Automation not found"),
        "unexpected error: {error}"
    );
}

#[test]
fn runs_page_with_cursor_and_fenced_extra_fields_are_tolerated() {
    let dir = TestDir::new("runs");
    let store = load_store(&dir);
    let id = created_id(&store, "FREQ=HOURLY;BYMINUTE=0");
    for scheduled_for in [1_i64, 2, 3] {
        store
            .create_run(json!({
                "automationId": id.as_str(),
                "scheduledFor": scheduled_for,
                "status": "completed",
                "trigger": "scheduled",
                "createdAt": scheduled_for,
            }))
            .unwrap();
    }

    let (first, cursor) = runs_impl(&store, Some(id.as_str()), Some(2), None);
    assert_eq!(first.len(), 2);
    assert_eq!(first[0]["scheduledFor"], 3);
    assert_eq!(first[1]["scheduledFor"], 2);
    let cursor = cursor.expect("a second page exists");
    let (second, cursor) = runs_impl(&store, Some(id.as_str()), Some(2), Some(&cursor));
    assert_eq!(second.len(), 1);
    assert_eq!(second[0]["scheduledFor"], 1);
    assert_eq!(cursor, None);

    // Fenced/legacy extras (selector/expectedOwner/destination) deserialize
    // without failing; omitted paging fields stay absent.
    let args: AutomationsRunsArgs = serde_json::from_value(json!({
        "automationId": id.as_str(),
        "expectedOwner": {"kind": "self"},
        "selector": {"kind": "self"},
        "destination": "elsewhere",
    }))
    .unwrap();
    assert_eq!(args.automation_id.as_deref(), Some(id.as_str()));
    assert_eq!(args.limit, None);
    assert_eq!(args.cursor, None);
}

#[test]
fn mark_dispatch_result_whitelists_fields_and_guards_terminal_runs() {
    let dir = TestDir::new("mark-result");
    let store = load_store(&dir);
    let id = created_id(&store, "FREQ=HOURLY;BYMINUTE=0");
    let (run, _) = store
        .create_run(json!({
            "automationId": id.as_str(),
            "scheduledFor": 1,
            "status": "dispatching",
            "trigger": "scheduled",
            "createdAt": 1,
        }))
        .unwrap();
    let run_id = run["id"].as_str().unwrap().to_string();
    let (service, sink) = service_for(&store);

    // Non-object or status-less results are refused before touching the store.
    assert!(mark_dispatch_result_impl(&service, &run_id, &json!("nope")).is_err());
    assert!(
        mark_dispatch_result_impl(&service, &run_id, &json!({"terminalSessionId": "t1"})).is_err()
    );

    let dispatched = mark_dispatch_result_impl(
        &service,
        &run_id,
        &json!({
            "runId": run_id,
            "status": "dispatched",
            "workspaceId": "ws-1",
            "terminalSessionId": "t1",
            "runNumber": 999,
            "unexpectedField": "ignored",
        }),
    )
    .unwrap()
    .expect("run exists");
    assert_eq!(dispatched["status"], "dispatched");
    assert_eq!(dispatched["terminalSessionId"], "t1");
    assert_eq!(
        dispatched["startedAt"], TEST_NOW,
        "the dispatched transition is stamped host-side"
    );
    assert_eq!(dispatched["dispatchedAt"], TEST_NOW);
    assert_eq!(
        dispatched["runNumber"], 1,
        "store-owned fields are protected"
    );
    assert!(
        dispatched.get("unexpectedField").is_none(),
        "only whitelisted result fields are written"
    );

    let completed = mark_dispatch_result_impl(
        &service,
        &run_id,
        &json!({
            "status": "completed",
            "outputSnapshot": {
                "format": "plain_text",
                "content": "done",
                "capturedAt": 1,
                "truncated": false
            }
        }),
    )
    .unwrap()
    .expect("run exists");
    assert_eq!(completed["status"], "completed");
    assert_eq!(completed["startedAt"], TEST_NOW, "stamps survive completion");

    // A result that never passed through `dispatched` leaves both stamps absent.
    let (direct, _) = store
        .create_run(json!({
            "automationId": id.as_str(),
            "scheduledFor": 2,
            "status": "dispatching",
            "trigger": "scheduled",
            "createdAt": 2,
        }))
        .unwrap();
    let direct_completed = mark_dispatch_result_impl(
        &service,
        direct["id"].as_str().unwrap(),
        &json!({"status": "completed"}),
    )
    .unwrap()
    .expect("run exists");
    assert!(direct_completed.get("startedAt").is_none());
    assert!(direct_completed.get("dispatchedAt").is_none());

    // A late write must never revive a terminal run.
    let guarded = mark_dispatch_result_impl(
        &service,
        &run_id,
        &json!({"status": "dispatching", "terminalSessionId": "t2"}),
    )
    .unwrap()
    .expect("run exists");
    assert_eq!(guarded["status"], "completed");
    assert_eq!(guarded["terminalSessionId"], "t1");

    assert!(
        mark_dispatch_result_impl(&service, "missing", &json!({"status": "completed"}))
            .unwrap()
            .is_none()
    );
    assert!(sink.changed().contains(&"run".to_string()));
}

#[test]
fn run_precheck_persists_the_result_for_scheduled_runs() {
    let dir = TestDir::new("precheck");
    let store = load_store(&dir);
    let mut input = definition("FREQ=HOURLY;BYMINUTE=0", TEST_NOW - 3_600_000);
    input["precheck"] = json!({"command": "echo ok", "timeoutSeconds": 5});
    let id = create_impl(&store, input, TEST_NOW).unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();
    let (run, _) = store
        .create_run(json!({
            "automationId": id.as_str(),
            "scheduledFor": 1,
            "status": "dispatching",
            "trigger": "scheduled",
            "createdAt": 1,
        }))
        .unwrap();
    let run_id = run["id"].as_str().unwrap().to_string();
    let (service, _sink) = service_for(&store);

    let result = run_precheck_impl(&service, &id, &run_id).expect("scheduled precheck runs");
    assert_eq!(result["command"], "echo ok");
    assert_eq!(result["exitCode"], 0);
    assert_eq!(result["stdout"].as_str().unwrap().trim(), "ok");
    assert_eq!(result["timedOut"], false);

    let (runs, _) = runs_impl(&store, Some(id.as_str()), None, None);
    assert_eq!(runs[0]["precheckResult"], result);

    // Manual runs are never prechecked, and unknown runs report absence.
    let (manual, _) = store
        .create_run(json!({
            "automationId": id.as_str(),
            "scheduledFor": 2,
            "status": "dispatching",
            "trigger": "manual",
            "createdAt": 2,
        }))
        .unwrap();
    assert!(run_precheck_impl(&service, &id, manual["id"].as_str().unwrap()).is_none());
    assert!(run_precheck_impl(&service, &id, "missing").is_none());
}

#[test]
fn snapshot_workspace_name_rewrites_matching_runs_and_broadcasts() {
    let dir = TestDir::new("snapshot");
    let store = load_store(&dir);
    store
        .create_run(json!({
            "automationId": "a-1",
            "scheduledFor": 1,
            "status": "completed",
            "trigger": "scheduled",
            "workspaceId": "ws-1",
            "createdAt": 1,
        }))
        .unwrap();
    let (service, sink) = service_for(&store);

    assert_eq!(
        snapshot_workspace_name_impl(&service, "ws-1", "Nice Name"),
        1
    );
    let (runs, _) = runs_impl(&store, None, None, None);
    assert_eq!(runs[0]["workspaceDisplayName"], "Nice Name");
    assert_eq!(
        snapshot_workspace_name_impl(&service, "ws-1", "Nice Name"),
        0,
        "already-normalized names are not rewritten"
    );
    assert!(sink.changed().contains(&"run".to_string()));
}
