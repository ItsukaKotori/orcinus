//! `orcinus open`: launch the Orcinus app. The runtime-wait path was removed
//! with the computer-use trim — nothing serves the runtime RPC anymore — so
//! `open` launches and exits without waiting.
//! Launcher resolution: `ORCINUS_OPEN_COMMAND` (via `sh -c`) >
//! `ORCINUS_APP_EXECUTABLE` > `open -a Orcinus`.

use std::process::{Command, Stdio};

use crate::errors::CliError;

/// Launch the app; success means the launcher process spawned.
pub fn open() -> Result<(), CliError> {
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
        // exit 0 immediately like `true`); spawn success is the only completion
        // signal now that the reachability poll is gone.
        Ok(_child) => Ok(()),
        Err(error) => Err(CliError::new(
            "runtime_open_failed",
            format!("Could not launch Orcinus via {program}: {error}."),
        )),
    }
}
