//! Worker supervision: owns at most one long-lived Node worker for one plugin,
//! drives the `init` → `ready` handshake, correlates request/reply frames, and
//! applies the crash-restart policy (spec §3.2: ready 10s, invoke 30s, event
//! 5min, idle reap 5min, grace 2s, ≤3 restarts with backoff [500,2000,5000]ms).
//!
//! The restart counter mirrors `orca:src/main/plugins/plugin-supervisor.ts`:
//! the first crash of an activation moves the worker to `restarting` with the
//! attempt's backoff, and exceeding `max_restarts` marks it `errored`. A
//! host-initiated `stop` (idle reap, revision change, deactivate) is a fresh
//! activation boundary and clears the counter.
//!
//! Failure policy: [`WorkerSupervisor::start`] drives the restart loop itself
//! (sleeping the backoff before each retry) and only returns once the worker is
//! `running` or terminally `errored`. [`WorkerSupervisor::send`] never retries;
//! a crash, timeout, or protocol error kills the worker, records the failure
//! (arming the next backoff), and surfaces one error to the caller — the next
//! `start` is what respawns. This keeps in-flight commands from being replayed
//! implicitly and leaves invocation-level retry policy to the manager.

use std::io::BufReader;
use std::process::{ChildStdin, ChildStdout};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, TryRecvError};
use std::thread;
use std::time::{Duration, Instant};

use crate::types::{ACTION_FAILED, IO_ERROR, UNAVAILABLE};
use crate::PluginError;

use super::protocol::{read_frame, write_frame, ChildFrame, ParentFrame};
use super::spawn::{WorkerHandle, WorkerSpec};

/// Amount of time between liveness probes while draining a worker.
const POLL_INTERVAL: Duration = Duration::from_millis(10);

/// Correlation key for the reply awaited by [`WorkerSupervisor::send`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReplyKind {
    Command { call_id: u64 },
    Event { event_id: u64 },
}

/// Supervised worker lifecycle state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkerState {
    Inactive,
    Starting,
    Running { revision: String },
    Restarting,
    Errored { reason: String },
}

/// Supervision limits and restart schedule (spec §3.2).
#[derive(Debug, Clone)]
pub struct SupervisorConfig {
    pub ready_timeout: Duration,
    pub invoke_timeout: Duration,
    pub event_timeout: Duration,
    pub idle_timeout: Duration,
    pub grace: Duration,
    pub max_restarts: u32,
    pub backoff: [Duration; 3],
}

impl Default for SupervisorConfig {
    fn default() -> Self {
        Self {
            ready_timeout: Duration::from_secs(10),
            invoke_timeout: Duration::from_secs(30),
            event_timeout: Duration::from_secs(300),
            idle_timeout: Duration::from_secs(300),
            grace: Duration::from_secs(2),
            max_restarts: 3,
            backoff: [
                Duration::from_millis(500),
                Duration::from_millis(2000),
                Duration::from_millis(5000),
            ],
        }
    }
}

/// Sink for frames that do not correlate with the awaited reply (logs, host
/// calls, stale results); the manager installs frame routing here.
pub type FrameSink = Box<dyn FnMut(&ChildFrame) + Send>;

type SpawnFn = Box<dyn FnMut() -> Result<(WorkerHandle, String), PluginError> + Send>;

struct ActiveWorker {
    handle: WorkerHandle,
    stdin: ChildStdin,
    frames: Receiver<Result<ChildFrame, PluginError>>,
}

enum Wait {
    Frame(ChildFrame),
    Exited,
    Timeout,
}

/// Owns at most one worker for a single plugin key for its whole lifetime.
pub struct WorkerSupervisor {
    spec: WorkerSpec,
    config: SupervisorConfig,
    state: WorkerState,
    worker: Option<ActiveWorker>,
    spawner: Option<SpawnFn>,
    sink: Option<FrameSink>,
    restarts: u32,
    pending_backoff: Option<Duration>,
    last_activity: Instant,
    ready_commands: Vec<String>,
}

impl WorkerSupervisor {
    pub fn new(spec: WorkerSpec, config: SupervisorConfig) -> Self {
        Self {
            spec,
            config,
            state: WorkerState::Inactive,
            worker: None,
            spawner: None,
            sink: None,
            restarts: 0,
            pending_backoff: None,
            last_activity: Instant::now(),
            ready_commands: Vec::new(),
        }
    }

    pub fn plugin_key(&self) -> &str {
        &self.spec.plugin_key
    }

    pub fn state(&self) -> WorkerState {
        self.state.clone()
    }

    /// Number of restarts performed in the current activation lifetime; reset
    /// by [`WorkerSupervisor::stop`] (a fresh activation boundary).
    pub fn restart_count(&self) -> u32 {
        self.restarts
    }

    /// Commands the worker registered in the `ready` frame of its current
    /// activation; empty unless `running`.
    pub fn ready_commands(&self) -> &[String] {
        &self.ready_commands
    }

    pub fn set_frame_sink(&mut self, sink: Option<FrameSink>) {
        self.sink = sink;
    }

    /// Spawn and handshake until `running`, retrying crash/timeout failures
    /// with the configured backoff up to `max_restarts`; on exhaustion the
    /// supervisor becomes `errored` and this returns the last failure.
    ///
    /// The closure is stored so [`WorkerSupervisor::set_revision`] can restart
    /// without the manager re-supplying it. It is not called while `running`
    /// (idempotent start) or while `errored` (terminal until `stop`).
    pub fn start(
        &mut self,
        mut spawn: impl FnMut() -> Result<(WorkerHandle, String), PluginError> + Send + 'static,
    ) -> Result<(), PluginError> {
        let result = self.start_with(&mut spawn);
        self.spawner = Some(Box::new(spawn));
        result
    }

    /// Write `frame` and wait for its correlated reply, enforcing the invoke
    /// (commands) or event timeout. Unrelated frames go to the frame sink. Any
    /// IO/crash/timeout failure cleans up the worker, records the failure, and
    /// returns an error; no retry happens here (see module docs).
    pub fn send(
        &mut self,
        frame: &ParentFrame,
        await_reply: ReplyKind,
    ) -> Result<ChildFrame, PluginError> {
        if !matches!(self.state, WorkerState::Running { .. }) {
            return Err(PluginError::new(
                UNAVAILABLE,
                format!("plugin worker {} is not running", self.spec.plugin_key),
            ));
        }
        let timeout = match await_reply {
            ReplyKind::Command { .. } => self.config.invoke_timeout,
            ReplyKind::Event { .. } => self.config.event_timeout,
        };
        let write_error = {
            let active = self.worker.as_mut().expect("running implies a worker");
            write_frame(&mut active.stdin, frame).err()
        };
        if let Some(error) = write_error {
            let error = PluginError::new(
                IO_ERROR,
                format!(
                    "failed to write frame to plugin worker: {}",
                    error.message()
                ),
            );
            return Err(self.fail_active(error));
        }
        let awaited = {
            let active = self.worker.as_mut().expect("running implies a worker");
            await_reply_frame(active, self.sink.as_mut(), timeout, await_reply)
        };
        match awaited {
            Ok(reply) => {
                self.touch();
                Ok(reply)
            }
            Err(error) => Err(self.fail_active(error)),
        }
    }

    /// Non-blockingly drain frames the worker has already sent. Unrelated
    /// frames (host calls, logs, stale results) go to the frame sink; a fatal
    /// frame, a malformed frame, or a closed stream fails the worker exactly
    /// like an in-flight [`WorkerSupervisor::send`] (kill + record failure)
    /// and surfaces one error. Unlike `send`, a drained fatal frame is routed
    /// to the frame sink before failing, so the manager's sink observes it.
    /// No-op unless `running`.
    pub fn drain(&mut self) -> Result<(), PluginError> {
        if !matches!(self.state, WorkerState::Running { .. }) {
            return Ok(());
        }
        enum Drained {
            Frame(ChildFrame),
            Poison(PluginError),
            Empty,
            Closed,
        }
        loop {
            let drained = {
                let active = self.worker.as_mut().expect("running implies a worker");
                match active.frames.try_recv() {
                    Ok(Ok(frame)) => Drained::Frame(frame),
                    Ok(Err(error)) => Drained::Poison(error),
                    Err(TryRecvError::Empty) => Drained::Empty,
                    Err(TryRecvError::Disconnected) => Drained::Closed,
                }
            };
            match drained {
                Drained::Frame(frame) => {
                    let fatal = matches!(frame, ChildFrame::Fatal { .. });
                    if let Some(sink) = self.sink.as_mut() {
                        sink(&frame);
                    }
                    if fatal {
                        let error = match frame {
                            ChildFrame::Fatal { error } => error,
                            _ => unreachable!("fatal matched above"),
                        };
                        return Err(self.fail_active(fatal_error(&error)));
                    }
                    self.touch();
                }
                Drained::Poison(error) => return Err(self.fail_active(error)),
                Drained::Closed => {
                    return Err(
                        self.fail_active(PluginError::new(ACTION_FAILED, "plugin worker exited"))
                    )
                }
                Drained::Empty => return Ok(()),
            }
        }
    }

    /// Send `shutdown`, wait up to `grace` for a clean exit, then terminate the
    /// process group (SIGTERM → SIGKILL via [`WorkerHandle::kill`]) so no
    /// worker or descendant survives. Always ends `inactive` with a cleared
    /// restart history.
    pub fn stop(&mut self, grace: Duration) {
        if let Some(mut active) = self.worker.take() {
            let _ = write_frame(&mut active.stdin, &ParentFrame::Shutdown);
            let deadline = Instant::now() + grace;
            while Instant::now() < deadline {
                match active.handle.child.try_wait() {
                    Ok(Some(_)) => break,
                    Ok(None) => {}
                    Err(_) => break,
                }
                thread::sleep(POLL_INTERVAL);
            }
            active.handle.kill(Duration::ZERO);
        }
        self.state = WorkerState::Inactive;
        self.restarts = 0;
        self.pending_backoff = None;
        self.ready_commands.clear();
        self.touch();
    }

    /// Apply a new manifest revision: a running worker with a different
    /// revision is stopped and restarted through the stored spawn closure; any
    /// other state just records the revision for the next activation.
    pub fn set_revision(&mut self, revision: &str) -> Result<(), PluginError> {
        let changed = matches!(
            &self.state,
            WorkerState::Running { revision: current } if current != revision
        );
        self.spec.manifest_revision = revision.to_owned();
        if !changed {
            return Ok(());
        }
        let grace = self.config.grace;
        self.stop(grace);
        let mut spawner = self.spawner.take().ok_or_else(|| {
            PluginError::new(UNAVAILABLE, "supervisor has no stored spawn closure")
        })?;
        let result = self.start_with(&mut *spawner);
        self.spawner = Some(spawner);
        result
    }

    /// A command arrived that the worker never declared at `ready`: terminate
    /// it and park the supervisor in `errored` (a declaration bug is terminal,
    /// not retryable).
    pub fn note_undeclared_command(&mut self) {
        let grace = self.config.grace;
        self.stop(grace);
        self.state = WorkerState::Errored {
            reason: "undeclared-command".to_owned(),
        };
    }

    /// Mark activity now (called on start and on every successful send).
    pub fn touch(&mut self) {
        self.last_activity = Instant::now();
    }

    /// Whether a running worker has been untouched for at least
    /// `idle_timeout`; only `running` workers are reapable.
    pub fn is_idle(&self, now: Instant) -> bool {
        matches!(self.state, WorkerState::Running { .. })
            && now.saturating_duration_since(self.last_activity) >= self.config.idle_timeout
    }

    fn start_with(
        &mut self,
        spawn: &mut dyn FnMut() -> Result<(WorkerHandle, String), PluginError>,
    ) -> Result<(), PluginError> {
        if matches!(self.state, WorkerState::Running { .. }) {
            return Ok(());
        }
        if let WorkerState::Errored { reason } = &self.state {
            return Err(PluginError::new(
                UNAVAILABLE,
                format!("plugin worker is errored: {reason}"),
            ));
        }
        loop {
            if let Some(delay) = self.pending_backoff.take() {
                if !delay.is_zero() {
                    thread::sleep(delay);
                }
            }
            self.state = WorkerState::Starting;
            match self.start_attempt(spawn) {
                Ok(revision) => {
                    self.state = WorkerState::Running { revision };
                    self.touch();
                    return Ok(());
                }
                Err(error) => {
                    self.discard_worker();
                    let reason = error.message().to_owned();
                    if !self.record_failure(&reason) {
                        return Err(PluginError::new(
                            ACTION_FAILED,
                            format!(
                                "plugin worker errored after {} restarts: {reason}",
                                self.config.max_restarts
                            ),
                        ));
                    }
                }
            }
        }
    }

    fn start_attempt(
        &mut self,
        spawn: &mut dyn FnMut() -> Result<(WorkerHandle, String), PluginError>,
    ) -> Result<String, PluginError> {
        let (mut handle, revision) = spawn()?;
        let grace = self.config.grace;
        let Some(stdin) = handle.take_stdin() else {
            handle.kill(grace);
            return Err(PluginError::new(UNAVAILABLE, "worker handle has no stdin"));
        };
        let Some(stdout) = handle.take_stdout() else {
            handle.kill(grace);
            return Err(PluginError::new(UNAVAILABLE, "worker handle has no stdout"));
        };
        if let Some(mut stderr) = handle.take_stderr() {
            // Why: an undrained stderr pipe fills and stalls the worker.
            thread::spawn(move || {
                let _ = std::io::copy(&mut stderr, &mut std::io::sink());
            });
        }
        let mut active = ActiveWorker {
            handle,
            stdin,
            frames: spawn_reader(stdout),
        };
        let init = ParentFrame::Init {
            plugin_id: self.spec.plugin_key.clone(),
            plugin_root: self.spec.plugin_root.to_string_lossy().into_owned(),
            main_entry: self.spec.main_entry.clone(),
            granted_capabilities: self
                .spec
                .granted_capabilities
                .iter()
                .map(|capability| capability.as_str().to_owned())
                .collect(),
        };
        if let Err(error) = write_frame(&mut active.stdin, &init) {
            active.handle.kill(grace);
            return Err(error);
        }
        let ready_timeout = self.config.ready_timeout;
        let commands = match await_ready(&mut active, self.sink.as_mut(), ready_timeout) {
            Ok(commands) => commands,
            Err(error) => {
                active.handle.kill(grace);
                return Err(error);
            }
        };
        self.worker = Some(active);
        self.ready_commands = commands;
        Ok(revision)
    }

    fn discard_worker(&mut self) {
        if let Some(mut active) = self.worker.take() {
            let grace = self.config.grace;
            active.handle.kill(grace);
        }
        self.ready_commands.clear();
    }

    /// Record a crash/timeout failure: arm the next backoff and move to
    /// `restarting`, or park in `errored` once the budget is spent. Returns
    /// whether a restart is still allowed.
    fn record_failure(&mut self, reason: &str) -> bool {
        if self.restarts >= self.config.max_restarts {
            self.pending_backoff = None;
            self.state = WorkerState::Errored {
                reason: reason.to_owned(),
            };
            false
        } else {
            let index = (self.restarts as usize).min(self.config.backoff.len() - 1);
            self.pending_backoff = Some(self.config.backoff[index]);
            self.restarts += 1;
            self.state = WorkerState::Restarting;
            true
        }
    }

    fn fail_active(&mut self, error: PluginError) -> PluginError {
        self.discard_worker();
        let reason = error.message().to_owned();
        self.record_failure(&reason);
        error
    }
}

impl Drop for WorkerSupervisor {
    fn drop(&mut self) {
        // Why: WorkerHandle has no Drop of its own; the supervisor is the
        // lifecycle owner, so a dropped supervisor must not leak a child.
        self.discard_worker();
    }
}

fn spawn_reader(stdout: ChildStdout) -> Receiver<Result<ChildFrame, PluginError>> {
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        let mut reader = BufReader::new(stdout);
        loop {
            match read_frame::<_, ChildFrame>(&mut reader) {
                Ok(Some(frame)) => {
                    if sender.send(Ok(frame)).is_err() {
                        return;
                    }
                }
                Ok(None) => return,
                Err(error) => {
                    let _ = sender.send(Err(error));
                    return;
                }
            }
        }
    });
    receiver
}

fn wait_frame(active: &mut ActiveWorker, timeout: Duration) -> Result<Wait, PluginError> {
    match active.frames.recv_timeout(timeout) {
        Ok(Ok(frame)) => Ok(Wait::Frame(frame)),
        Ok(Err(error)) => Err(error),
        Err(RecvTimeoutError::Timeout) => Ok(Wait::Timeout),
        Err(RecvTimeoutError::Disconnected) => Ok(Wait::Exited),
    }
}

fn remaining(deadline: Instant) -> Duration {
    deadline.saturating_duration_since(Instant::now())
}

fn fatal_error(error: &str) -> PluginError {
    PluginError::new(
        ACTION_FAILED,
        format!("plugin worker reported a fatal error: {error}"),
    )
}

fn await_ready(
    active: &mut ActiveWorker,
    mut sink: Option<&mut FrameSink>,
    timeout: Duration,
) -> Result<Vec<String>, PluginError> {
    let deadline = Instant::now() + timeout;
    loop {
        let left = remaining(deadline);
        if left.is_zero() {
            return Err(PluginError::new(
                ACTION_FAILED,
                format!("plugin worker did not become ready within {timeout:?}"),
            ));
        }
        match wait_frame(active, left)? {
            Wait::Frame(ChildFrame::Ready { commands }) => return Ok(commands),
            Wait::Frame(ChildFrame::Fatal { error }) => return Err(fatal_error(&error)),
            Wait::Frame(frame) => {
                if let Some(sink) = sink.as_deref_mut() {
                    sink(&frame);
                }
            }
            Wait::Exited => {
                return Err(PluginError::new(
                    ACTION_FAILED,
                    "plugin worker exited before becoming ready",
                ))
            }
            Wait::Timeout => {
                return Err(PluginError::new(
                    ACTION_FAILED,
                    format!("plugin worker did not become ready within {timeout:?}"),
                ))
            }
        }
    }
}

fn await_reply_frame(
    active: &mut ActiveWorker,
    mut sink: Option<&mut FrameSink>,
    timeout: Duration,
    reply: ReplyKind,
) -> Result<ChildFrame, PluginError> {
    let deadline = Instant::now() + timeout;
    loop {
        let left = remaining(deadline);
        if left.is_zero() {
            return Err(reply_timeout_error(reply, timeout));
        }
        match wait_frame(active, left)? {
            Wait::Frame(frame) => {
                if matches_reply(&frame, reply) {
                    return Ok(frame);
                }
                match frame {
                    ChildFrame::Fatal { error } => return Err(fatal_error(&error)),
                    frame => {
                        if let Some(sink) = sink.as_deref_mut() {
                            sink(&frame);
                        }
                    }
                }
            }
            Wait::Exited => {
                return Err(PluginError::new(
                    ACTION_FAILED,
                    "plugin worker exited before replying",
                ))
            }
            Wait::Timeout => return Err(reply_timeout_error(reply, timeout)),
        }
    }
}

fn reply_timeout_error(reply: ReplyKind, timeout: Duration) -> PluginError {
    let awaited = match reply {
        ReplyKind::Command { call_id } => format!("command {call_id}"),
        ReplyKind::Event { event_id } => format!("event {event_id}"),
    };
    PluginError::new(
        ACTION_FAILED,
        format!("plugin worker {awaited} did not reply within {timeout:?}"),
    )
}

fn matches_reply(frame: &ChildFrame, reply: ReplyKind) -> bool {
    match (frame, reply) {
        (ChildFrame::CommandResult { call_id, .. }, ReplyKind::Command { call_id: expected }) => {
            *call_id == expected
        }
        (ChildFrame::EventAck { event_id }, ReplyKind::Event { event_id: expected }) => {
            *event_id == expected
        }
        _ => false,
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::types::Capability;
    use crate::worker::spawn::{resolve_node, spawn_worker};
    use serde_json::json;
    use std::path::{Path, PathBuf};
    use std::sync::{Arc, Mutex};

    /// Fake worker speaking the NDJSON protocol; behavior is selected through
    /// `FAKE_WORKER_MODE` (passing `spawn_worker`'s env allowlist).
    const FAKE_WORKER: &str = r#"import { createInterface } from 'node:readline';
import { writeFileSync } from 'node:fs';

const mode = process.env.FAKE_WORKER_MODE ?? 'happy';
const markerPath = process.env.FAKE_WORKER_MARKER;
const write = (frame) => process.stdout.write(JSON.stringify(frame) + '\n');

if (mode !== 'silent') {
  write({ type: 'ready', commands: ['echo'] });
}

const rl = createInterface({ input: process.stdin });
rl.on('line', (line) => {
  let frame;
  try { frame = JSON.parse(line); } catch { return; }
  if (frame.type === 'invokeCommand') {
    if (mode === 'crash-on-invoke') { process.exit(1); }
    write({ type: 'commandResult', callId: frame.callId, ok: true, value: { command: frame.commandId } });
  } else if (frame.type === 'deliverEvent') {
    write({ type: 'eventAck', eventId: frame.eventId });
  } else if (frame.type === 'shutdown') {
    if (markerPath) { writeFileSync(markerPath, 'shutdown received'); }
    if (mode !== 'stubborn') { process.exit(0); }
  }
});
"#;

    struct Fixture {
        spec: WorkerSpec,
        entry: PathBuf,
        node: PathBuf,
    }

    impl Fixture {
        fn new(root: &Path) -> Option<Self> {
            let node = resolve_node(None, None).ok()?;
            let entry = root.join("host-entry.mjs");
            std::fs::write(&entry, FAKE_WORKER).unwrap();
            Some(Self {
                spec: worker_spec(root),
                entry,
                node,
            })
        }

        /// Build a spawn closure that records every pid and reports the shared
        /// `revision` on each (re)start.
        fn spawn_fn(
            &self,
            mode: &str,
            marker: Option<&Path>,
            pids: Arc<Mutex<Vec<u32>>>,
            revision: Arc<Mutex<String>>,
        ) -> impl FnMut() -> Result<(WorkerHandle, String), PluginError> + 'static {
            let spec = self.spec.clone();
            let entry = self.entry.clone();
            let node = self.node.clone();
            let mut extra = vec![("FAKE_WORKER_MODE".to_owned(), mode.to_owned())];
            if let Some(marker) = marker {
                extra.push((
                    "FAKE_WORKER_MARKER".to_owned(),
                    marker.to_string_lossy().into_owned(),
                ));
            }
            move || {
                let handle = spawn_worker(&spec, &entry, &node, &extra)?;
                pids.lock().unwrap().push(handle.pid);
                Ok((handle, revision.lock().unwrap().clone()))
            }
        }
    }

    fn worker_spec(plugin_root: &Path) -> WorkerSpec {
        WorkerSpec {
            instance_key: "global:pub.test".to_owned(),
            plugin_key: "pub.test".to_owned(),
            plugin_root: plugin_root.to_path_buf(),
            main_entry: "main.mjs".to_owned(),
            granted_capabilities: vec![Capability::Storage],
            manifest_revision: "rev-1".to_owned(),
            declared_commands: vec!["echo".to_owned()],
        }
    }

    fn ms(value: u64) -> Duration {
        Duration::from_millis(value)
    }

    fn revision(value: &str) -> Arc<Mutex<String>> {
        Arc::new(Mutex::new(value.to_owned()))
    }

    fn fast_config(ready_timeout: u64, backoff: [u64; 3]) -> SupervisorConfig {
        SupervisorConfig {
            ready_timeout: ms(ready_timeout),
            invoke_timeout: ms(500),
            event_timeout: ms(500),
            idle_timeout: ms(80),
            grace: ms(100),
            max_restarts: 3,
            backoff: [ms(backoff[0]), ms(backoff[1]), ms(backoff[2])],
        }
    }

    fn process_alive(pid: u32) -> bool {
        unsafe { libc::kill(pid as i32, 0) == 0 }
    }

    #[test]
    fn start_drives_handshake_to_running_and_correlates_frames() {
        let dir = tempfile::tempdir().unwrap();
        let Some(fixture) = Fixture::new(dir.path()) else {
            return;
        };
        let mut supervisor =
            WorkerSupervisor::new(fixture.spec.clone(), fast_config(300, [10, 20, 30]));
        assert_eq!(supervisor.state(), WorkerState::Inactive);

        let pids = Arc::new(Mutex::new(Vec::new()));
        supervisor
            .start(fixture.spawn_fn("happy", None, Arc::clone(&pids), revision("rev-1")))
            .unwrap();
        assert_eq!(
            supervisor.state(),
            WorkerState::Running {
                revision: "rev-1".to_owned()
            }
        );
        assert_eq!(supervisor.restart_count(), 0);

        let reply = supervisor
            .send(
                &ParentFrame::InvokeCommand {
                    call_id: 7,
                    command_id: "echo".to_owned(),
                    args: Some(json!({ "n": 1 })),
                },
                ReplyKind::Command { call_id: 7 },
            )
            .unwrap();
        assert_eq!(
            reply,
            ChildFrame::CommandResult {
                call_id: 7,
                ok: true,
                value: Some(json!({ "command": "echo" })),
                error: None,
            }
        );

        let ack = supervisor
            .send(
                &ParentFrame::DeliverEvent {
                    event_id: 3,
                    event: "worktree.created".to_owned(),
                    payload: json!({ "worktreeId": "wt-1" }),
                },
                ReplyKind::Event { event_id: 3 },
            )
            .unwrap();
        assert_eq!(ack, ChildFrame::EventAck { event_id: 3 });
        assert!(matches!(
            supervisor.state(),
            WorkerState::Running { revision } if revision == "rev-1"
        ));

        let pid = pids.lock().unwrap()[0];
        supervisor.stop(ms(100));
        assert_eq!(supervisor.state(), WorkerState::Inactive);
        assert!(!process_alive(pid), "worker must be reaped by stop");
    }

    #[test]
    fn ready_timeout_restarts_with_backoff_until_errored() {
        let dir = tempfile::tempdir().unwrap();
        let Some(fixture) = Fixture::new(dir.path()) else {
            return;
        };
        let mut supervisor =
            WorkerSupervisor::new(fixture.spec.clone(), fast_config(60, [20, 40, 60]));
        let attempts = Arc::new(Mutex::new(Vec::<Instant>::new()));
        let recorded = Arc::clone(&attempts);
        let pids = Arc::new(Mutex::new(Vec::new()));
        let mut spawn = fixture.spawn_fn("silent", None, Arc::clone(&pids), revision("rev-1"));
        let result = supervisor.start(move || {
            recorded.lock().unwrap().push(Instant::now());
            spawn()
        });

        assert!(result.is_err());
        assert!(matches!(supervisor.state(), WorkerState::Errored { .. }));
        assert_eq!(supervisor.restart_count(), 3);
        {
            let attempts = attempts.lock().unwrap();
            assert_eq!(attempts.len(), 4, "one initial attempt plus max_restarts");
            let gaps: Vec<Duration> = attempts.windows(2).map(|pair| pair[1] - pair[0]).collect();
            let backoff = [ms(20), ms(40), ms(60)];
            for (index, gap) in gaps.iter().enumerate() {
                let floor = backoff[index] + ms(60) - ms(10);
                assert!(
                    *gap >= floor,
                    "attempt {index} restarted after {gap:?}, expected at least {floor:?}"
                );
            }
        }

        // Errored is terminal until an explicit stop: no further spawns.
        let spawn = fixture.spawn_fn("happy", None, Arc::clone(&pids), revision("rev-2"));
        assert!(supervisor.start(spawn).is_err());
        assert_eq!(pids.lock().unwrap().len(), 4);
    }

    #[test]
    fn crash_mid_command_surfaces_error_and_next_start_restarts() {
        let dir = tempfile::tempdir().unwrap();
        let Some(fixture) = Fixture::new(dir.path()) else {
            return;
        };
        let mut supervisor =
            WorkerSupervisor::new(fixture.spec.clone(), fast_config(300, [20, 40, 60]));
        let pids = Arc::new(Mutex::new(Vec::new()));
        let revision = revision("rev-1");
        supervisor
            .start(fixture.spawn_fn(
                "crash-on-invoke",
                None,
                Arc::clone(&pids),
                Arc::clone(&revision),
            ))
            .unwrap();
        let first_pid = pids.lock().unwrap()[0];

        let error = supervisor
            .send(
                &ParentFrame::InvokeCommand {
                    call_id: 1,
                    command_id: "echo".to_owned(),
                    args: None,
                },
                ReplyKind::Command { call_id: 1 },
            )
            .unwrap_err();
        assert_eq!(error.code(), ACTION_FAILED);
        assert!(matches!(supervisor.state(), WorkerState::Restarting));
        assert_eq!(supervisor.restart_count(), 1);
        assert!(!process_alive(first_pid), "crashed worker must be reaped");

        supervisor
            .start(fixture.spawn_fn("happy", None, Arc::clone(&pids), Arc::clone(&revision)))
            .unwrap();
        assert!(matches!(
            supervisor.state(),
            WorkerState::Running { revision } if revision == "rev-1"
        ));
        assert_eq!(
            supervisor.restart_count(),
            1,
            "restart history survives until a fresh activation"
        );
        assert_eq!(pids.lock().unwrap().len(), 2);
        let second_pid = pids.lock().unwrap()[1];
        assert!(process_alive(second_pid), "restarted worker must be alive");
        supervisor.stop(ms(100));
    }

    #[test]
    fn undeclared_command_stops_worker_and_errors() {
        let dir = tempfile::tempdir().unwrap();
        let Some(fixture) = Fixture::new(dir.path()) else {
            return;
        };
        let mut supervisor =
            WorkerSupervisor::new(fixture.spec.clone(), fast_config(300, [10, 20, 30]));
        let pids = Arc::new(Mutex::new(Vec::new()));
        supervisor
            .start(fixture.spawn_fn("happy", None, Arc::clone(&pids), revision("rev-1")))
            .unwrap();
        let pid = pids.lock().unwrap()[0];

        supervisor.note_undeclared_command();
        assert_eq!(
            supervisor.state(),
            WorkerState::Errored {
                reason: "undeclared-command".to_owned()
            }
        );
        assert!(!process_alive(pid), "worker must be reaped");

        let spawn = fixture.spawn_fn("happy", None, Arc::clone(&pids), revision("rev-1"));
        assert!(supervisor.start(spawn).is_err());
        assert_eq!(pids.lock().unwrap().len(), 1, "errored is terminal");
    }

    #[test]
    fn revision_change_stops_old_worker_and_starts_new() {
        let dir = tempfile::tempdir().unwrap();
        let Some(fixture) = Fixture::new(dir.path()) else {
            return;
        };
        let mut supervisor =
            WorkerSupervisor::new(fixture.spec.clone(), fast_config(300, [10, 20, 30]));
        let pids = Arc::new(Mutex::new(Vec::new()));
        let revision = revision("rev-1");
        supervisor
            .start(fixture.spawn_fn("happy", None, Arc::clone(&pids), Arc::clone(&revision)))
            .unwrap();
        let first_pid = pids.lock().unwrap()[0];

        supervisor.set_revision("rev-1").unwrap();
        assert_eq!(
            pids.lock().unwrap().len(),
            1,
            "same revision must not respawn"
        );

        *revision.lock().unwrap() = "rev-2".to_owned();
        supervisor.set_revision("rev-2").unwrap();
        assert_eq!(
            supervisor.state(),
            WorkerState::Running {
                revision: "rev-2".to_owned()
            }
        );
        assert_eq!(pids.lock().unwrap().len(), 2);
        assert!(!process_alive(first_pid), "old worker must be killed");
        let second_pid = pids.lock().unwrap()[1];
        assert!(process_alive(second_pid), "new worker must be alive");
        supervisor.stop(ms(100));
        assert!(!process_alive(second_pid));
        assert_eq!(supervisor.state(), WorkerState::Inactive);
    }

    #[test]
    fn stop_sends_shutdown_then_kills_a_stubborn_worker() {
        let dir = tempfile::tempdir().unwrap();
        let Some(fixture) = Fixture::new(dir.path()) else {
            return;
        };
        let mut config = fast_config(300, [10, 20, 30]);
        config.grace = ms(300);
        let mut supervisor = WorkerSupervisor::new(fixture.spec.clone(), config);
        let pids = Arc::new(Mutex::new(Vec::new()));
        let marker = dir.path().join("shutdown.marker");
        supervisor
            .start(fixture.spawn_fn(
                "stubborn",
                Some(&marker),
                Arc::clone(&pids),
                revision("rev-1"),
            ))
            .unwrap();
        let pid = pids.lock().unwrap()[0];

        let started = Instant::now();
        supervisor.stop(ms(300));
        assert!(
            started.elapsed() < Duration::from_secs(3),
            "stop must stay bounded"
        );
        assert_eq!(supervisor.state(), WorkerState::Inactive);
        assert!(
            marker.exists(),
            "stop must deliver the shutdown frame before killing"
        );
        assert!(!process_alive(pid), "stubborn worker must be killed");
    }

    #[test]
    fn idle_tracking_uses_touch_and_ignores_non_running_states() {
        let dir = tempfile::tempdir().unwrap();
        let Some(fixture) = Fixture::new(dir.path()) else {
            return;
        };
        let mut supervisor =
            WorkerSupervisor::new(fixture.spec.clone(), fast_config(300, [10, 20, 30]));
        assert!(
            !supervisor.is_idle(Instant::now() + ms(1000)),
            "inactive supervisors are never idle"
        );

        let pids = Arc::new(Mutex::new(Vec::new()));
        supervisor
            .start(fixture.spawn_fn("happy", None, Arc::clone(&pids), revision("rev-1")))
            .unwrap();
        let now = Instant::now();
        assert!(!supervisor.is_idle(now), "fresh activity is not idle");
        assert!(
            supervisor.is_idle(now + ms(100)),
            "past idle_timeout the worker is reapable"
        );

        supervisor.touch();
        let now = Instant::now();
        assert!(!supervisor.is_idle(now + ms(50)));
        assert!(supervisor.is_idle(now + ms(100)));

        supervisor.stop(ms(50));
        assert!(!supervisor.is_idle(Instant::now() + ms(1000)));
    }
}
