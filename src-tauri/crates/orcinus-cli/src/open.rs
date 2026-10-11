//! `orcinus open` (spec §3.6): launch the Orcinus app if the runtime is not
//! already reachable, then poll `status.get` (250ms × 15s) until it answers.
//! Launcher resolution: `ORCINUS_OPEN_COMMAND` (via `sh -c`) >
//! `ORCINUS_APP_EXECUTABLE` > `open -a Orcinus`.

use std::process::{Command, Stdio};
use std::thread::sleep;
use std::time::{Duration, Instant};

use crate::errors::CliError;
use crate::status::{probe_reachable, probe_status, ProbeFailure, StatusSnapshot};

/// Open timeout budget (spec §3.6).
pub const OPEN_TIMEOUT: Duration = Duration::from_secs(15);
/// Delay between reachability probes.
pub const OPEN_POLL_INTERVAL: Duration = Duration::from_millis(250);

/// `open` result: the reachable snapshot plus the runtime id for the envelope.
pub fn open() -> Result<(StatusSnapshot, String), CliError> {
    match probe_status() {
        Ok((snapshot, runtime_id)) => return Ok((snapshot, runtime_id)),
        // Why: launching cannot fix a token the runtime rejects; surface it as
        // runtime_unavailable instead of waiting out the open timeout. The
        // metadata was readable, so keep its runtime id on the failure
        // envelope.
        Err(ProbeFailure::AuthFailed(error)) => {
            let mut mapped = CliError::new(
                "runtime_unavailable",
                format!(
                    "The Orcinus runtime rejected this session's auth token: {}",
                    error.message
                ),
            );
            if let Some(runtime_id) = error.runtime_id {
                mapped = mapped.with_runtime_id(runtime_id);
            }
            return Err(mapped);
        }
        Err(ProbeFailure::Unreachable | ProbeFailure::NotRunning) => {}
    }

    launch_app()?;

    let started = Instant::now();
    while started.elapsed() < OPEN_TIMEOUT {
        if probe_reachable() {
            return match probe_status() {
                Ok(result) => Ok(result),
                Err(_) => Err(CliError::new(
                    "runtime_open_failed",
                    "The Orcinus runtime became unreachable while opening.",
                )),
            };
        }
        sleep(OPEN_POLL_INTERVAL);
    }

    Err(CliError::new(
        "runtime_open_timeout",
        "Timed out waiting for an Orcinus desktop window. Run 'orcinus open' once the app is running, or start Orcinus manually.",
    ))
}

/// Fire-and-forget launch per the env override chain. A spawn failure is
/// `runtime_open_failed`; the launcher's later exit status does not matter
/// because the poll loop decides success.
fn launch_app() -> Result<(), CliError> {
    if let Ok(command) = std::env::var("ORCINUS_OPEN_COMMAND") {
        if !command.trim().is_empty() {
            return spawn_detached("sh", &["-c", command.as_str()]);
        }
    }
    if let Ok(executable) = std::env::var("ORCINUS_APP_EXECUTABLE") {
        if !executable.trim().is_empty() {
            return spawn_detached(&executable, &[]);
        }
    }
    spawn_detached("open", &["-a", "Orcinus"])
}

fn spawn_detached(program: &str, args: &[&str]) -> Result<(), CliError> {
    match Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    {
        // Why: the launched process is expected to outlive this CLI (or to
        // exit 0 immediately like `true`); the reachability poll below is the
        // only success signal, matching the oracle's detached spawn.
        Ok(_child) => Ok(()),
        Err(error) => Err(open_failed(format!(
            "Could not launch Orcinus via {program}: {error}."
        ))),
    }
}

fn open_failed(message: String) -> CliError {
    CliError::new("runtime_open_failed", message)
}
