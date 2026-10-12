//! Worker manager: the pool that owns at most one [`WorkerSupervisor`] per
//! scope-qualified instance key ([`WorkerSpec::instance_key`]; spec §3.2: max
//! 5 workers), bridges child `HostCall`
//! frames to a [`HostCallHandler`], and funnels logs/fatal frames to a
//! [`FrameEventSink`].
//!
//! Activation is lazy: construction resolves the Node binary once (a missing
//! node is a terminal `node_missing` error, never a retry), and
//! [`WorkerManager::ensure_active`] is the only entry point that spawns.
//! Asking for a plugin already running on the same manifest revision is a
//! no-op; a different revision is applied through the supervisor's
//! `set_revision`, which stops and respawns the worker. A new key while the
//! pool is full fails with [`WORKER_LIMIT`] before any process starts.
//!
//! Host calls are answered from the manager's frame sink: because the
//! supervisor owns the child's stdin, the manager keeps a duplicated
//! descriptor of the worker's stdin pipe and writes `hostResult` frames
//! through that copy. The sink runs synchronously on the manager thread while
//! it awaits a command/event reply, and [`WorkerManager::pump`] drains frames
//! that arrived while idle. If a write-back fails the child will surface the
//! failure itself (host-call timeout in the plugin or worker exit).

use std::collections::HashMap;
use std::fs::File;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use serde_json::Value;

use crate::types::{ACTION_FAILED, UNAVAILABLE};
use crate::PluginError;

use super::protocol::{write_frame, ChildFrame, LogLevel, ParentFrame};
use super::spawn::{resolve_node, spawn_worker, WorkerHandle, WorkerSpec, NODE_MISSING};
use super::supervisor::{FrameSink, ReplyKind, SupervisorConfig, WorkerState, WorkerSupervisor};

/// Stable error code for a full worker pool (spec §3.2 max 5 workers). A
/// `rate_limited`-style soft failure: the caller may retry after stopping
/// another plugin's worker.
pub const WORKER_LIMIT: &str = "worker_limit";

/// Stable error code for an `invoke_command` naming a command outside the
/// manifest's declared set. Soft failure: the frame is never sent and the
/// worker keeps running.
pub const UNKNOWN_COMMAND: &str = "unknown_command";

/// Handles a child `HostCall` frame. `instance_key` is the authenticated
/// worker identity (the scope-qualified instance key the frame arrived from,
/// bound by the manager's per-worker frame sink — never anything the child
/// sent). `params` is `null` when the frame omits it. `Err((code, error))` is
/// written back as `hostResult{ok:false}`.
pub trait HostCallHandler: Send + Sync {
    fn handle(
        &self,
        instance_key: &str,
        call_id: u64,
        method: &str,
        params: Value,
    ) -> Result<Value, (String, String)>;
}

/// Sink for worker frames that are not part of a command/event exchange.
pub trait FrameEventSink: Send + Sync {
    fn on_log(&self, plugin_key: &str, level: &str, message: &str);
    fn on_fatal(&self, plugin_key: &str, error: &str);
}

/// Pool limits plus the spawn template for one manager.
#[derive(Debug, Clone)]
pub struct WorkerManagerConfig {
    /// Maximum number of workers with a live process (spec §3.2: 5).
    pub max_active: usize,
    /// Absolute path of the `host-entry.mjs` resource passed to `node`.
    pub host_entry_path: PathBuf,
    /// Node binary; resolved once at construction (missing → `node_missing`).
    pub node: PathBuf,
    /// Per-worker supervision limits (ready/invoke/event/idle/grace/restart).
    pub supervisor: SupervisorConfig,
}

struct WorkerEntry {
    supervisor: WorkerSupervisor,
    /// Latest spec seen by `ensure_active`; the spawn closure reads it so a
    /// restart after `set_revision` starts the new revision.
    spec: Arc<Mutex<WorkerSpec>>,
    /// Duplicated stdin write end of the current worker (see module docs).
    writer: Arc<Mutex<Option<File>>>,
}

/// Owns the worker pool for every active worker instance key.
pub struct WorkerManager {
    config: WorkerManagerConfig,
    node: Result<PathBuf, PluginError>,
    host_call: Arc<dyn HostCallHandler>,
    event_sink: Arc<dyn FrameEventSink>,
    workers: HashMap<String, WorkerEntry>,
}

impl WorkerManager {
    /// Resolve the Node binary once; a missing/unusable node is stored as a
    /// terminal error returned by every [`WorkerManager::ensure_active`].
    pub fn new(
        config: WorkerManagerConfig,
        host_call: Arc<dyn HostCallHandler>,
        event_sink: Arc<dyn FrameEventSink>,
    ) -> Self {
        let node = match config.node.to_str() {
            Some(path) => resolve_node(Some(path), None),
            None => Err(PluginError::new(
                NODE_MISSING,
                format!(
                    "node executable path is not valid UTF-8: {}",
                    config.node.display()
                ),
            )),
        };
        Self {
            config,
            node,
            host_call,
            event_sink,
            workers: HashMap::new(),
        }
    }

    /// Lazily activate `spec.instance_key`: spawn + `init`/`ready` handshake
    /// if the instance is not active, refresh the revision if it changed, and require
    /// [`WORKER_LIMIT`] when starting a *new* key would exceed `max_active`.
    /// The worker's `ready` command list is checked against
    /// `spec.declared_commands`: a registered command outside the manifest's
    /// set parks the worker in `errored` (terminal until
    /// [`WorkerManager::stop`]) and is returned here.
    pub fn ensure_active(&mut self, spec: WorkerSpec) -> Result<(), PluginError> {
        let node = match &self.node {
            Ok(node) => node.clone(),
            Err(error) => return Err(error.clone()),
        };
        let key = spec.instance_key.clone();
        let revision = spec.manifest_revision.clone();

        if let Some(entry) = self.workers.get_mut(&key) {
            *entry.spec.lock().expect("worker spec lock poisoned") = spec;
            let state = entry.supervisor.state();
            if let WorkerState::Errored { reason } = &state {
                return Err(PluginError::new(
                    UNAVAILABLE,
                    format!("plugin worker {key} is errored: {reason}"),
                ));
            }
            if matches!(&state, WorkerState::Running { revision: current } if *current == revision)
            {
                return Ok(());
            }
            let result = match &state {
                WorkerState::Running { .. } => entry.supervisor.set_revision(&revision),
                _ => {
                    let spawner = make_spawner(
                        Arc::clone(&entry.spec),
                        self.config.host_entry_path.clone(),
                        node,
                        Arc::clone(&entry.writer),
                    );
                    entry.supervisor.start(spawner)
                }
            };
            result?;
            let declared = entry
                .spec
                .lock()
                .expect("worker spec lock poisoned")
                .declared_commands
                .clone();
            return verify_ready_commands(&mut entry.supervisor, &declared);
        }

        if self.active_count() >= self.config.max_active {
            return Err(PluginError::new(
                WORKER_LIMIT,
                format!(
                    "worker limit reached (max_active={}): plugin {key} cannot activate",
                    self.config.max_active
                ),
            ));
        }

        let declared = spec.declared_commands.clone();
        let shared = Arc::new(Mutex::new(spec.clone()));
        let writer = Arc::new(Mutex::new(None));
        let sink = make_sink(
            key.clone(),
            Arc::clone(&self.host_call),
            Arc::clone(&self.event_sink),
            Arc::clone(&writer),
        );
        let spawner = make_spawner(
            Arc::clone(&shared),
            self.config.host_entry_path.clone(),
            node,
            Arc::clone(&writer),
        );
        let mut supervisor = WorkerSupervisor::new(spec, self.config.supervisor.clone());
        supervisor.set_frame_sink(Some(sink));
        let result = supervisor
            .start(spawner)
            .and_then(|()| verify_ready_commands(&mut supervisor, &declared));
        self.workers.insert(
            key,
            WorkerEntry {
                supervisor,
                spec: shared,
                writer,
            },
        );
        result
    }

    /// Send `invokeCommand` and await the correlated `commandResult` within
    /// `invoke`. The worker must be active, and `command_id` must be in the
    /// manifest's declared set ([`UNKNOWN_COMMAND`] otherwise, no kill: the
    /// worker never receives the frame). An `ok:false` reply is passed through
    /// as an `action_failed` [`PluginError`] whose message is the
    /// worker-provided text — worker error text alone never changes lifecycle
    /// state (undeclared *registrations* are handled in `ensure_active`).
    pub fn invoke_command(
        &mut self,
        instance_key: &str,
        call_id: u64,
        command_id: &str,
        args: Option<Value>,
    ) -> Result<Value, PluginError> {
        let entry = self.workers.get_mut(instance_key).ok_or_else(|| {
            PluginError::new(
                UNAVAILABLE,
                format!("plugin worker {instance_key} is not active"),
            )
        })?;
        let declared = {
            let spec = entry.spec.lock().expect("worker spec lock poisoned");
            !spec
                .declared_commands
                .iter()
                .any(|declared| declared == command_id)
        };
        if declared {
            return Err(PluginError::new(
                UNKNOWN_COMMAND,
                format!("plugin {instance_key} does not declare command {command_id}"),
            ));
        }
        let reply = entry.supervisor.send(
            &ParentFrame::InvokeCommand {
                call_id,
                command_id: command_id.to_owned(),
                args,
            },
            ReplyKind::Command { call_id },
        )?;
        match reply {
            ChildFrame::CommandResult {
                ok: true, value, ..
            } => Ok(value.unwrap_or(Value::Null)),
            ChildFrame::CommandResult {
                ok: false, error, ..
            } => Err(PluginError::new(
                ACTION_FAILED,
                error.unwrap_or_else(|| "plugin command failed".to_owned()),
            )),
            other => Err(PluginError::new(
                ACTION_FAILED,
                format!("unexpected reply to command {call_id}: {other:?}"),
            )),
        }
    }

    /// Send `deliverEvent` and await its `eventAck` within the supervisor's
    /// `event_timeout`. The worker must be active.
    pub fn deliver_event(
        &mut self,
        instance_key: &str,
        event_id: u64,
        event: &str,
        payload: Value,
    ) -> Result<(), PluginError> {
        let supervisor = self.workers.get_mut(instance_key).ok_or_else(|| {
            PluginError::new(
                UNAVAILABLE,
                format!("plugin worker {instance_key} is not active"),
            )
        })?;
        supervisor.supervisor.send(
            &ParentFrame::DeliverEvent {
                event_id,
                event: event.to_owned(),
                payload,
            },
            ReplyKind::Event { event_id },
        )?;
        Ok(())
    }

    /// Non-blockingly drain frames that already arrived: `hostCall` frames are
    /// answered through the [`HostCallHandler`], logs/fatal go to the
    /// [`FrameEventSink`]. `Some(key)` drains one worker, `None` drains every
    /// active worker; an unknown/inactive key is a no-op.
    pub fn pump(&mut self, plugin_key: Option<&str>) -> Result<(), PluginError> {
        match plugin_key {
            Some(key) => {
                if let Some(entry) = self.workers.get_mut(key) {
                    entry.supervisor.drain()?;
                }
                Ok(())
            }
            None => {
                for entry in self.workers.values_mut() {
                    entry.supervisor.drain()?;
                }
                Ok(())
            }
        }
    }

    /// Stop and remove every running worker untouched for at least the
    /// supervisor's `idle_timeout`; returns the reaped instance keys. Inactive
    /// and errored entries are left alone (an errored entry owns no process).
    pub fn reap_idle(&mut self) -> Vec<String> {
        let now = std::time::Instant::now();
        let keys: Vec<String> = self
            .workers
            .iter()
            .filter(|(_, entry)| entry.supervisor.is_idle(now))
            .map(|(key, _)| key.clone())
            .collect();
        for key in &keys {
            self.stop(key);
        }
        keys
    }

    /// Restart count of `instance_key` in its current activation lifetime;
    /// unknown keys report zero.
    pub fn restarts(&self, instance_key: &str) -> u32 {
        self.workers
            .get(instance_key)
            .map(|entry| entry.supervisor.restart_count())
            .unwrap_or(0)
    }

    /// Stop and remove one worker (graceful `shutdown` then group kill).
    pub fn stop(&mut self, instance_key: &str) {
        if let Some(mut entry) = self.workers.remove(instance_key) {
            entry.supervisor.stop(self.config.supervisor.grace);
        }
    }

    /// Stop and remove every worker.
    pub fn stop_all(&mut self) {
        let grace = self.config.supervisor.grace;
        for (_, mut entry) in self.workers.drain() {
            entry.supervisor.stop(grace);
        }
    }

    /// Number of workers currently occupying a pool slot (not `Inactive` and
    /// not `Errored`; an errored entry no longer owns a process).
    pub fn active_count(&self) -> usize {
        self.workers
            .values()
            .filter(|entry| {
                !matches!(
                    entry.supervisor.state(),
                    WorkerState::Inactive | WorkerState::Errored { .. }
                )
            })
            .count()
    }

    /// Lifecycle state of `instance_key`; unknown keys are `Inactive`.
    pub fn state(&self, instance_key: &str) -> WorkerState {
        self.workers
            .get(instance_key)
            .map(|entry| entry.supervisor.state())
            .unwrap_or(WorkerState::Inactive)
    }
}

/// Build the supervisor spawn closure: reads the latest spec, spawns through
/// [`spawn_worker`], and refreshes the duplicated stdin writer for host-call
/// write-back. The revision reported to the supervisor is the one current at
/// spawn time, so `set_revision` restarts report the new revision.
fn make_spawner(
    spec: Arc<Mutex<WorkerSpec>>,
    host_entry: PathBuf,
    node: PathBuf,
    writer: Arc<Mutex<Option<File>>>,
) -> impl FnMut() -> Result<(WorkerHandle, String), PluginError> + Send + 'static {
    move || {
        let spec = spec.lock().expect("worker spec lock poisoned").clone();
        let handle = spawn_worker(&spec, &host_entry, &node, &[])?;
        if let Some(duplicate) = duplicate_stdin(&handle) {
            *writer.lock().expect("worker writer lock poisoned") = Some(duplicate);
        }
        Ok((handle, spec.manifest_revision))
    }
}

/// Build the frame sink: answers host calls through the duplicated stdin
/// writer and forwards logs/fatal frames to the event sink. Stale results for
/// other correlations are ignored.
fn make_sink(
    plugin_key: String,
    host_call: Arc<dyn HostCallHandler>,
    event_sink: Arc<dyn FrameEventSink>,
    writer: Arc<Mutex<Option<File>>>,
) -> FrameSink {
    Box::new(move |frame| match frame {
        ChildFrame::HostCall {
            call_id,
            method,
            params,
        } => {
            let params = params.clone().unwrap_or(Value::Null);
            let result = host_call.handle(&plugin_key, *call_id, method, params);
            let host_result = match result {
                Ok(value) => ParentFrame::HostResult {
                    call_id: *call_id,
                    ok: true,
                    value: Some(value),
                    error_code: None,
                    error: None,
                },
                Err((code, error)) => ParentFrame::HostResult {
                    call_id: *call_id,
                    ok: false,
                    value: None,
                    error_code: Some(code),
                    error: Some(error),
                },
            };
            // Best effort: if the write fails the worker is already gone and
            // the next supervisor read/`pump` reports the exit.
            if let Ok(mut guard) = writer.lock() {
                if let Some(file) = guard.as_mut() {
                    let _ = write_frame(file, &host_result);
                }
            }
        }
        ChildFrame::Log { level, message } => {
            event_sink.on_log(&plugin_key, log_level_str(*level), message);
        }
        ChildFrame::Fatal { error } => event_sink.on_fatal(&plugin_key, error),
        _ => {}
    })
}

fn log_level_str(level: LogLevel) -> &'static str {
    match level {
        LogLevel::Info => "info",
        LogLevel::Warn => "warn",
        LogLevel::Error => "error",
    }
}

/// Check the worker's `ready` command list against the manifest's declared
/// set (exact string equality). A registered command outside the set is a
/// declaration bug: the worker is stopped and parked in `errored`.
fn verify_ready_commands(
    supervisor: &mut WorkerSupervisor,
    declared: &[String],
) -> Result<(), PluginError> {
    let message = {
        let undeclared: Vec<&str> = supervisor
            .ready_commands()
            .iter()
            .filter(|command| !declared.iter().any(|declared| declared == *command))
            .map(String::as_str)
            .collect();
        if undeclared.is_empty() {
            return Ok(());
        }
        undeclared.join(", ")
    };
    supervisor.note_undeclared_command();
    Err(PluginError::new(
        ACTION_FAILED,
        format!("plugin worker registered undeclared command(s): {message}"),
    ))
}

/// Duplicate the worker's stdin write end so the manager can answer host calls
/// while the supervisor owns the primary handle.
#[cfg(unix)]
fn duplicate_stdin(handle: &WorkerHandle) -> Option<File> {
    use std::os::unix::io::{AsRawFd, FromRawFd};
    let stdin = handle.stdin.as_ref()?;
    // Why: dup(2) yields an independent descriptor to the same pipe; closing
    // the copy never disturbs the supervisor's handle.
    let fd = unsafe { libc::dup(stdin.as_raw_fd()) };
    if fd < 0 {
        return None;
    }
    Some(unsafe { File::from_raw_fd(fd) })
}

#[cfg(windows)]
fn duplicate_stdin(handle: &WorkerHandle) -> Option<File> {
    use std::os::windows::io::{AsHandle, BorrowedHandle};
    let stdin = handle.stdin.as_ref()?;
    let owned = BorrowedHandle::try_clone_to_owned(&AsHandle::as_handle(stdin)).ok()?;
    Some(File::from(owned))
}

#[cfg(not(any(unix, windows)))]
fn duplicate_stdin(_handle: &WorkerHandle) -> Option<File> {
    None
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::types::Capability;
    use serde_json::json;
    use std::time::{Duration, Instant};

    /// Fake NDJSON worker used by the manager tests. Behavior is data-driven
    /// through files in its cwd (the plugin root): `worker.pid` is written
    /// before `ready`, and `emit-pump` makes it issue a spontaneous `hostCall`
    /// right after `ready` (for the idle-pump test).
    const FAKE_WORKER: &str = r#"import { createInterface } from 'node:readline';
import { existsSync, readFileSync, writeFileSync } from 'node:fs';

const write = (frame) => process.stdout.write(JSON.stringify(frame) + '\n');
const pending = new Map();
const marker = (name, value) => {
  try { writeFileSync(name, String(value)); } catch {}
};

marker('worker.pid', process.pid);
const extra = existsSync('extra-command') ? [readFileSync('extra-command', 'utf8').trim()] : [];
write({
  type: 'ready',
  commands: ['echo', 'fail', 'undeclared', 'host-call', 'host-call-error', ...extra]
});

if (existsSync('emit-pump')) {
  pending.set(7000, (value) => marker('pump-ok.marker', JSON.stringify(value)));
  write({ type: 'hostCall', callId: 7000, method: 'ping', params: { source: 'idle' } });
}

const rl = createInterface({ input: process.stdin });
rl.on('line', async (line) => {
  let frame;
  try { frame = JSON.parse(line); } catch { return; }
  if (frame.type === 'invokeCommand') {
    if (frame.commandId === 'fail') {
      write({ type: 'commandResult', callId: frame.callId, ok: false, error: 'boom' });
      return;
    }
    if (frame.commandId === 'undeclared') {
      write({
        type: 'commandResult',
        callId: frame.callId,
        ok: false,
        error: `no handler registered for command ${frame.commandId}`
      });
      return;
    }
    if (frame.commandId === 'host-call') {
      const value = await new Promise((resolve) => {
        pending.set(9001, resolve);
        write({ type: 'hostCall', callId: 9001, method: 'storage.get', params: { key: 'k' } });
      });
      write({ type: 'commandResult', callId: frame.callId, ok: true, value: { hostValue: value } });
      return;
    }
    if (frame.commandId === 'host-call-error') {
      try {
        await new Promise((resolve, reject) => {
          pending.set(9002, { resolve, reject });
          write({ type: 'hostCall', callId: 9002, method: 'storage.deny', params: { key: 'denied' } });
        });
        write({ type: 'commandResult', callId: frame.callId, ok: true, value: { unexpected: true } });
      } catch (error) {
        write({
          type: 'commandResult',
          callId: frame.callId,
          ok: false,
          error: `${error.code}:${error.message}`
        });
      }
      return;
    }
    write({ type: 'log', level: 'info', message: `${frame.commandId} handled` });
    write({
      type: 'commandResult',
      callId: frame.callId,
      ok: true,
      value: { command: frame.commandId, args: frame.args ?? null }
    });
    return;
  }
  if (frame.type === 'deliverEvent') {
    write({ type: 'eventAck', eventId: frame.eventId });
    return;
  }
  if (frame.type === 'hostResult') {
    const entry = pending.get(frame.callId);
    if (entry) {
      pending.delete(frame.callId);
      if (typeof entry === 'function') {
        entry(frame.ok ? frame.value : null);
      } else if (frame.ok) {
        entry.resolve(frame.value);
      } else {
        entry.reject(Object.assign(new Error(frame.error ?? 'host call failed'), { code: frame.errorCode }));
      }
    }
    return;
  }
  if (frame.type === 'shutdown') process.exit(0);
});
"#;

    #[derive(Default)]
    struct RecordingHostCall {
        calls: Mutex<Vec<(u64, String, Value)>>,
    }

    impl HostCallHandler for RecordingHostCall {
        fn handle(
            &self,
            _instance_key: &str,
            call_id: u64,
            method: &str,
            params: Value,
        ) -> Result<Value, (String, String)> {
            self.calls.lock().expect("calls lock poisoned").push((
                call_id,
                method.to_owned(),
                params.clone(),
            ));
            match method {
                "storage.deny" => {
                    Err(("capability_denied".to_owned(), "denied by gate".to_owned()))
                }
                "ping" => Ok(json!({ "pong": params["source"] })),
                _ => Ok(json!({ "stored": "k" })),
            }
        }
    }

    #[derive(Default)]
    struct RecordingSink {
        logs: Mutex<Vec<(String, String, String)>>,
        fatals: Mutex<Vec<(String, String)>>,
    }

    impl FrameEventSink for RecordingSink {
        fn on_log(&self, plugin_key: &str, level: &str, message: &str) {
            self.logs.lock().expect("logs lock poisoned").push((
                plugin_key.to_owned(),
                level.to_owned(),
                message.to_owned(),
            ));
        }

        fn on_fatal(&self, plugin_key: &str, error: &str) {
            self.fatals
                .lock()
                .expect("fatals lock poisoned")
                .push((plugin_key.to_owned(), error.to_owned()));
        }
    }

    struct Fixture {
        dir: tempfile::TempDir,
        root: PathBuf,
        entry: PathBuf,
        node: PathBuf,
    }

    impl Fixture {
        fn new() -> Option<Self> {
            let node = resolve_node(None, None).ok()?;
            let dir = tempfile::tempdir().unwrap();
            let root = dir.path().join("plugin-root");
            std::fs::create_dir_all(&root).unwrap();
            let entry = dir.path().join("fake-worker.mjs");
            std::fs::write(&entry, FAKE_WORKER).unwrap();
            Some(Self {
                dir,
                root,
                entry,
                node,
            })
        }

        fn spec(&self, key: &str, revision: &str) -> WorkerSpec {
            WorkerSpec {
                instance_key: format!("global:{key}"),
                plugin_key: key.to_owned(),
                plugin_root: self.root.clone(),
                main_entry: "main.mjs".to_owned(),
                granted_capabilities: vec![Capability::Storage],
                manifest_revision: revision.to_owned(),
                declared_commands: vec![
                    "echo".to_owned(),
                    "fail".to_owned(),
                    "undeclared".to_owned(),
                    "host-call".to_owned(),
                    "host-call-error".to_owned(),
                ],
            }
        }

        fn manager(
            &self,
            max_active: usize,
            host_call: Arc<dyn HostCallHandler>,
            event_sink: Arc<dyn FrameEventSink>,
        ) -> WorkerManager {
            WorkerManager::new(
                WorkerManagerConfig {
                    max_active,
                    host_entry_path: self.entry.clone(),
                    node: self.node.clone(),
                    supervisor: fast_supervisor(),
                },
                host_call,
                event_sink,
            )
        }

        fn pid(&self) -> u32 {
            let path = self.root.join("worker.pid");
            std::fs::read_to_string(&path)
                .unwrap_or_else(|error| panic!("worker.pid missing at {}: {error}", path.display()))
                .trim()
                .parse()
                .expect("worker.pid must be a pid")
        }
    }

    fn fast_supervisor() -> SupervisorConfig {
        SupervisorConfig {
            ready_timeout: Duration::from_millis(500),
            invoke_timeout: Duration::from_millis(500),
            event_timeout: Duration::from_millis(500),
            idle_timeout: Duration::from_millis(200),
            grace: Duration::from_millis(150),
            max_restarts: 3,
            backoff: [
                Duration::from_millis(10),
                Duration::from_millis(20),
                Duration::from_millis(30),
            ],
        }
    }

    fn process_alive(pid: u32) -> bool {
        unsafe { libc::kill(pid as i32, 0) == 0 }
    }

    fn wait_until(deadline: Duration, mut condition: impl FnMut() -> bool) -> bool {
        let stop = Instant::now() + deadline;
        while Instant::now() < stop {
            if condition() {
                return true;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        condition()
    }

    fn assert_dead(pid: u32) {
        assert!(
            wait_until(Duration::from_secs(2), || !process_alive(pid)),
            "worker pid {pid} leaked"
        );
    }

    #[test]
    fn ensure_active_spawns_lazily_and_reaches_running() {
        let Some(fixture) = Fixture::new() else {
            return;
        };
        let mut manager = fixture.manager(
            5,
            Arc::new(RecordingHostCall::default()),
            Arc::new(RecordingSink::default()),
        );
        assert!(
            !fixture.root.join("worker.pid").exists(),
            "construction must not spawn a worker"
        );
        assert_eq!(manager.active_count(), 0);
        assert_eq!(manager.state("global:pub.one"), WorkerState::Inactive);

        manager
            .ensure_active(fixture.spec("pub.one", "rev-1"))
            .unwrap();
        assert!(matches!(
            manager.state("global:pub.one"),
            WorkerState::Running { revision } if revision == "rev-1"
        ));
        assert_eq!(manager.active_count(), 1);
        assert!(fixture.root.join("worker.pid").exists());

        manager
            .ensure_active(fixture.spec("pub.one", "rev-1"))
            .unwrap();
        assert_eq!(manager.active_count(), 1, "same-key activation is a no-op");

        let pid = fixture.pid();
        manager.stop_all();
        assert_eq!(manager.active_count(), 0);
        assert_dead(pid);
    }

    #[test]
    fn slot_pool_caps_new_keys_and_stop_frees_a_slot() {
        let Some(fixture) = Fixture::new() else {
            return;
        };
        let mut manager = fixture.manager(
            5,
            Arc::new(RecordingHostCall::default()),
            Arc::new(RecordingSink::default()),
        );
        for index in 0..5 {
            manager
                .ensure_active(fixture.spec(&format!("pub.p{index}"), "rev-1"))
                .unwrap();
        }
        assert_eq!(manager.active_count(), 5);

        let error = manager
            .ensure_active(fixture.spec("pub.six", "rev-1"))
            .unwrap_err();
        assert_eq!(error.code(), WORKER_LIMIT);
        assert!(error.message().contains("max_active=5"));
        assert_eq!(manager.state("global:pub.six"), WorkerState::Inactive);
        assert_eq!(manager.active_count(), 5);

        manager.stop("global:pub.p0");
        assert_eq!(manager.active_count(), 4);
        manager
            .ensure_active(fixture.spec("pub.six", "rev-1"))
            .unwrap();
        assert_eq!(manager.active_count(), 5);

        manager.stop_all();
        assert_eq!(manager.active_count(), 0);
    }

    #[test]
    fn invoke_command_bridges_results_errors_and_host_calls() {
        let Some(fixture) = Fixture::new() else {
            return;
        };
        let host = Arc::new(RecordingHostCall::default());
        let sink = Arc::new(RecordingSink::default());
        let mut manager = fixture.manager(5, host.clone(), sink.clone());
        manager
            .ensure_active(fixture.spec("pub.invoke", "rev-1"))
            .unwrap();

        let value = manager
            .invoke_command("global:pub.invoke", 1, "echo", Some(json!({ "n": 7 })))
            .unwrap();
        assert_eq!(value, json!({ "command": "echo", "args": { "n": 7 } }));
        {
            let logs = sink.logs.lock().unwrap();
            assert!(
                logs.iter()
                    .any(|(key, level, message)| key == "global:pub.invoke"
                        && level == "info"
                        && message == "echo handled"),
                "log frames must reach the event sink: {logs:?}"
            );
        }

        let error = manager
            .invoke_command("global:pub.invoke", 2, "fail", None)
            .unwrap_err();
        assert_eq!(error.code(), ACTION_FAILED);
        assert_eq!(error.message(), "boom");

        // The worker's host.call is answered by the handler mid-invoke; only
        // then does the command resolve, so the value proves the write-back.
        let value = manager
            .invoke_command("global:pub.invoke", 3, "host-call", None)
            .unwrap();
        assert_eq!(value, json!({ "hostValue": { "stored": "k" } }));
        {
            let calls = host.calls.lock().unwrap();
            assert_eq!(calls.len(), 1);
            assert_eq!(calls[0].0, 9001);
            assert_eq!(calls[0].1, "storage.get");
            assert_eq!(calls[0].2, json!({ "key": "k" }));
        }

        // Handler failures come back with `ok:false` + `errorCode`.
        let error = manager
            .invoke_command("global:pub.invoke", 4, "host-call-error", None)
            .unwrap_err();
        assert_eq!(error.message(), "capability_denied:denied by gate");

        // A declared command whose handler reports the undeclared-command
        // error text is a plain error: worker-controlled text never changes
        // lifecycle state.
        let error = manager
            .invoke_command("global:pub.invoke", 5, "undeclared", None)
            .unwrap_err();
        assert_eq!(error.code(), ACTION_FAILED);
        assert_eq!(
            error.message(),
            "no handler registered for command undeclared"
        );
        assert!(
            matches!(
                manager.state("global:pub.invoke"),
                WorkerState::Running { .. }
            ),
            "declared command error text must not kill the worker"
        );
        assert_eq!(manager.active_count(), 1);

        // A command outside the manifest's declared set is rejected before
        // any frame is sent, and the worker keeps running.
        let error = manager
            .invoke_command("global:pub.invoke", 6, "not-declared", None)
            .unwrap_err();
        assert_eq!(error.code(), UNKNOWN_COMMAND);
        assert!(matches!(
            manager.state("global:pub.invoke"),
            WorkerState::Running { .. }
        ));
        assert_eq!(manager.active_count(), 1);
        manager.stop_all();
    }

    #[test]
    fn ensure_active_rejects_commands_not_in_the_declared_set() {
        let Some(fixture) = Fixture::new() else {
            return;
        };
        std::fs::write(fixture.root.join("extra-command"), "rogue-command").unwrap();
        let mut manager = fixture.manager(
            5,
            Arc::new(RecordingHostCall::default()),
            Arc::new(RecordingSink::default()),
        );
        let error = manager
            .ensure_active(fixture.spec("pub.rogue", "rev-1"))
            .unwrap_err();
        assert_eq!(error.code(), ACTION_FAILED);
        assert!(
            error.message().contains("rogue-command"),
            "error must name the undeclared registration: {}",
            error.message()
        );
        assert!(matches!(
            manager.state("global:pub.rogue"),
            WorkerState::Errored { reason } if reason == "undeclared-command"
        ));
        assert_eq!(manager.active_count(), 0);
        assert!(manager
            .ensure_active(fixture.spec("pub.rogue", "rev-1"))
            .is_err());
    }

    #[test]
    fn deliver_event_awaits_the_ack() {
        let Some(fixture) = Fixture::new() else {
            return;
        };
        let mut manager = fixture.manager(
            5,
            Arc::new(RecordingHostCall::default()),
            Arc::new(RecordingSink::default()),
        );
        manager
            .ensure_active(fixture.spec("pub.events", "rev-1"))
            .unwrap();
        manager
            .deliver_event(
                "global:pub.events",
                3,
                "worktree.created",
                json!({ "worktreeId": "wt-1" }),
            )
            .unwrap();
        assert!(manager
            .deliver_event("global:pub.other", 4, "worktree.created", json!({}))
            .is_err());
        manager.stop_all();
    }

    #[test]
    fn pump_drains_and_answers_idle_host_calls() {
        let Some(fixture) = Fixture::new() else {
            return;
        };
        std::fs::write(fixture.root.join("emit-pump"), "idle").unwrap();
        let host = Arc::new(RecordingHostCall::default());
        let mut manager = fixture.manager(5, host.clone(), Arc::new(RecordingSink::default()));
        manager
            .ensure_active(fixture.spec("pub.pump", "rev-1"))
            .unwrap();
        assert_eq!(host.calls.lock().unwrap().len(), 0);

        // The frame may still be in flight right after `ready`; pump until it
        // arrives (the manager only drains what is already buffered).
        assert!(
            wait_until(Duration::from_secs(2), || {
                manager.pump(None).unwrap();
                !host.calls.lock().unwrap().is_empty()
            }),
            "pump never observed the idle hostCall"
        );
        {
            let calls = host.calls.lock().unwrap();
            assert_eq!(calls.len(), 1);
            assert_eq!(calls[0].0, 7000);
            assert_eq!(calls[0].1, "ping");
            assert_eq!(calls[0].2, json!({ "source": "idle" }));
        }
        // The worker writes the marker once it receives the HostResult.
        let marker = fixture.root.join("pump-ok.marker");
        assert!(
            wait_until(Duration::from_secs(2), || marker.exists()),
            "child never received the HostResult"
        );
        let recorded: Value =
            serde_json::from_str(&std::fs::read_to_string(&marker).unwrap()).unwrap();
        assert_eq!(recorded, json!({ "pong": "idle" }));

        manager.pump(Some("global:pub.pump")).unwrap();
        manager.pump(Some("global:pub.absent")).unwrap();
        manager.stop_all();
    }

    #[test]
    fn revision_change_restarts_and_stop_all_leaves_no_processes() {
        let Some(fixture) = Fixture::new() else {
            return;
        };
        let mut manager = fixture.manager(
            5,
            Arc::new(RecordingHostCall::default()),
            Arc::new(RecordingSink::default()),
        );
        manager
            .ensure_active(fixture.spec("pub.rev", "rev-1"))
            .unwrap();
        let first = fixture.pid();
        assert!(process_alive(first));

        manager
            .ensure_active(fixture.spec("pub.rev", "rev-1"))
            .unwrap();
        assert_eq!(fixture.pid(), first, "same revision must not respawn");

        manager
            .ensure_active(fixture.spec("pub.rev", "rev-2"))
            .unwrap();
        let second = fixture.pid();
        assert_ne!(second, first);
        assert!(matches!(
            manager.state("global:pub.rev"),
            WorkerState::Running { revision } if revision == "rev-2"
        ));
        assert_dead(first);
        assert!(process_alive(second));

        manager.stop_all();
        assert_eq!(manager.active_count(), 0);
        assert_eq!(manager.state("global:pub.rev"), WorkerState::Inactive);
        assert_dead(second);
    }

    #[test]
    fn missing_node_is_a_terminal_node_missing_at_ensure_active() {
        let Some(fixture) = Fixture::new() else {
            return;
        };
        let mut manager = WorkerManager::new(
            WorkerManagerConfig {
                max_active: 5,
                host_entry_path: fixture.entry.clone(),
                node: fixture.dir.path().join("no-such-node"),
                supervisor: fast_supervisor(),
            },
            Arc::new(RecordingHostCall::default()),
            Arc::new(RecordingSink::default()),
        );
        let error = manager
            .ensure_active(fixture.spec("pub.missing", "rev-1"))
            .unwrap_err();
        assert_eq!(error.code(), NODE_MISSING);
        let again = manager
            .ensure_active(fixture.spec("pub.missing", "rev-1"))
            .unwrap_err();
        assert_eq!(again.code(), NODE_MISSING, "terminal until reconfigured");
        assert_eq!(manager.active_count(), 0);
        assert!(!fixture.root.join("worker.pid").exists());
    }
}
