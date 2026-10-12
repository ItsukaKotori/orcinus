//! Worker spawn: locate the system Node runtime, prepare the allowlisted
//! environment, and start `node <host-entry.mjs>` in its own process group so
//! the supervisor can terminate the whole tree (the SIGTERM → grace → SIGKILL
//! shape mirrors `ade-automations/src/precheck.rs`).
//!
//! The child is long-lived and runs third-party plugin code, so it starts from
//! a cleared environment: only [`ALLOWED_ENV`] plus caller-supplied `extra_env`
//! entries are forwarded.

use std::path::{Path, PathBuf};
use std::process::{Child, ChildStderr, ChildStdin, ChildStdout, Command, Stdio};
use std::time::{Duration, Instant};

use crate::types::{Capability, IO_ERROR};
use crate::PluginError;

/// Stable error code for a missing or unusable Node binary (spec §3.2/§5).
pub const NODE_MISSING: &str = "node_missing";

/// Minimum supported Node major version (spec §3.2).
pub const MIN_NODE_MAJOR: u64 = 18;

/// Environment variables forwarded to the worker; every other process env var
/// is cleared (spec §3.2 allowlist).
pub const ALLOWED_ENV: [&str; 7] = ["PATH", "HOME", "LANG", "TZ", "TMPDIR", "TMP", "TEMP"];

/// `ORCINUS_NODE_PATH` names the override consumed by callers via the
/// `env_override` argument; kept here so the bridge and the supervisor agree.
pub const NODE_PATH_ENV: &str = "ORCINUS_NODE_PATH";

const NODE_BINARY: &str = "node";
const POLL_INTERVAL: Duration = Duration::from_millis(10);

fn path_separator() -> char {
    if cfg!(windows) {
        ';'
    } else {
        ':'
    }
}

fn find_node_in(path_var: &str) -> Option<PathBuf> {
    for dir in path_var
        .split(path_separator())
        .filter(|dir| !dir.is_empty())
    {
        let candidate = Path::new(dir).join(NODE_BINARY);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

/// Resolve the system `node` executable: the `ORCINUS_NODE_PATH` override
/// first (it must point at a file), then the login-shell hydrated PATH, then
/// the current process PATH. All misses are `node_missing`.
pub fn resolve_node(
    env_override: Option<&str>,
    hydrated_path: Option<&str>,
) -> Result<PathBuf, PluginError> {
    if let Some(override_path) = env_override {
        let candidate = PathBuf::from(override_path);
        if candidate.is_file() {
            return Ok(candidate);
        }
        return Err(missing_node(format!(
            "node executable not found at {NODE_PATH_ENV} path: {}",
            candidate.display()
        )));
    }
    if let Some(found) = hydrated_path.and_then(find_node_in) {
        return Ok(found);
    }
    if let Some(found) =
        std::env::var_os("PATH").and_then(|path| find_node_in(&path.to_string_lossy()))
    {
        return Ok(found);
    }
    Err(missing_node(format!(
        "node executable not found (install Node.js {MIN_NODE_MAJOR}+ or set {NODE_PATH_ENV})"
    )))
}

fn missing_node(message: String) -> PluginError {
    PluginError::new(NODE_MISSING, message)
}

/// Whether `node --version` output (`vX.Y.Z` or `X.Y.Z`) reports Node 18+.
pub fn node_supports(version_output: &str) -> bool {
    let trimmed = version_output.trim();
    let version = trimmed.strip_prefix('v').unwrap_or(trimmed);
    let mut parts = version.split('.');
    let (Some(major), Some(minor), Some(patch), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return false;
    };
    if minor.is_empty() || patch.is_empty() {
        return false;
    }
    major
        .parse::<u64>()
        .map(|major| major >= MIN_NODE_MAJOR)
        .unwrap_or(false)
}

/// Everything the supervisor needs to start (and re-start) one plugin worker.
#[derive(Debug, Clone)]
pub struct WorkerSpec {
    /// Scope-qualified instance identity (`global:<key>` /
    /// `project:<repoId>:<key>`): the manager's pool key, so a global and a
    /// project instance of the same plugin never collide.
    pub instance_key: String,
    /// Qualified plugin key; logging/audit identity and the child's plugin id.
    pub plugin_key: String,
    pub plugin_root: PathBuf,
    pub main_entry: String,
    pub granted_capabilities: Vec<Capability>,
    pub manifest_revision: String,
    /// Commands the manifest declares. The manager cross-checks the worker's
    /// `ready` list against this set (spec §3.2 undeclared-command policy).
    pub declared_commands: Vec<String>,
}

/// A spawned `node <host-entry.mjs>` child in its own process group. The
/// supervisor owns the stdio ends through the `pub(crate)` fields (or the
/// `take_*` accessors) and terminates the worker with [`WorkerHandle::kill`].
pub struct WorkerHandle {
    pub pid: u32,
    pub(crate) child: Child,
    pub(crate) stdin: Option<ChildStdin>,
    pub(crate) stdout: Option<ChildStdout>,
    pub(crate) stderr: Option<ChildStderr>,
}

impl WorkerHandle {
    pub fn take_stdin(&mut self) -> Option<ChildStdin> {
        self.stdin.take()
    }

    pub fn take_stdout(&mut self) -> Option<ChildStdout> {
        self.stdout.take()
    }

    pub fn take_stderr(&mut self) -> Option<ChildStderr> {
        self.stderr.take()
    }

    /// SIGTERM the worker's process group, wait up to `grace` for the child to
    /// exit, and SIGKILL the group if it is still alive; the child is always
    /// reaped, and surviving group members are SIGKILLed so no descendant can
    /// keep the stdio pipes open.
    pub fn kill(&mut self, grace: Duration) {
        #[cfg(unix)]
        {
            let pid = self.pid as i32;
            signal_process_group(pid, libc::SIGTERM);
            let deadline = Instant::now() + grace;
            let mut reaped = false;
            while Instant::now() < deadline {
                match self.child.try_wait() {
                    Ok(Some(_)) => {
                        reaped = true;
                        break;
                    }
                    Ok(None) => {}
                    Err(_) => break,
                }
                std::thread::sleep(POLL_INTERVAL);
            }
            if !reaped {
                // Why: SIGKILL is uncatchable, so even a TERM-ignoring worker
                // cannot survive past the grace period.
                signal_process_group(pid, libc::SIGKILL);
                let _ = self.child.wait();
                return;
            }
            // Why: a descendant that ignored SIGTERM can outlive the leader
            // while still holding the stdio pipes; take down the group too.
            if process_group_alive(pid) {
                signal_process_group(pid, libc::SIGKILL);
            }
        }
        #[cfg(not(unix))]
        {
            let deadline = Instant::now() + grace;
            loop {
                match self.child.try_wait() {
                    Ok(Some(_)) => return,
                    Ok(None) => {}
                    Err(_) => break,
                }
                if Instant::now() >= deadline {
                    break;
                }
                std::thread::sleep(POLL_INTERVAL);
            }
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

#[cfg(unix)]
fn signal_process_group(pid: i32, signal: libc::c_int) {
    // Why: the child leads its own group (`process_group(0)`); signaling the
    // negative pid reaches every descendant, not just the Node process.
    unsafe {
        libc::kill(-pid, signal);
    }
}

#[cfg(unix)]
fn process_group_alive(pid: i32) -> bool {
    // Signal 0 probes existence without delivering; ESRCH means no members.
    unsafe { libc::kill(-pid, 0) == 0 }
}

/// Spawn `node <host_entry_path>` for `spec`: cwd is the plugin root, stdio is
/// piped, the environment is cleared down to [`ALLOWED_ENV`] + `extra_env`, and
/// the child gets its own process group.
pub fn spawn_worker(
    spec: &WorkerSpec,
    host_entry_path: &Path,
    node: &Path,
    extra_env: &[(String, String)],
) -> Result<WorkerHandle, PluginError> {
    let mut command = Command::new(node);
    command.arg(host_entry_path);
    command.current_dir(&spec.plugin_root);
    command.env_clear();
    for key in ALLOWED_ENV {
        if let Some(value) = std::env::var_os(key) {
            command.env(key, value);
        }
    }
    for (key, value) in extra_env {
        command.env(key, value);
    }
    command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // Why: an own process group lets `kill` reach worker grandchildren.
        command.process_group(0);
    }
    let mut child = command.spawn().map_err(|error| {
        PluginError::new(
            IO_ERROR,
            format!(
                "failed to spawn node worker for {}: {error}",
                spec.plugin_key
            ),
        )
    })?;
    let pid = child.id();
    Ok(WorkerHandle {
        pid,
        stdin: child.stdin.take(),
        stdout: child.stdout.take(),
        stderr: child.stderr.take(),
        child,
    })
}

#[cfg(test)]
#[cfg(unix)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader};
    use std::process::ChildStdout;
    use std::sync::mpsc;
    use std::thread;

    fn write_executable(dir: &Path, name: &str, body: &str) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let path = dir.join(name);
        std::fs::write(&path, body).unwrap();
        let mut permissions = std::fs::metadata(&path).unwrap().permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&path, permissions).unwrap();
        path
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

    fn canonical(path: &Path) -> PathBuf {
        std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
    }

    fn process_alive(pid: u32) -> bool {
        unsafe { libc::kill(pid as i32, 0) == 0 }
    }

    fn wait_group_gone(pid: u32, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            if !process_group_alive(pid as i32) {
                return true;
            }
            thread::sleep(Duration::from_millis(10));
        }
        !process_group_alive(pid as i32)
    }

    fn is_allowed_worker_env(key: &str) -> bool {
        // `/bin/sh` injects its own bookkeeping vars (PWD/SHLVL/_/OLDPWD) on
        // top of the allowlisted environment; nothing else may appear.
        ALLOWED_ENV.contains(&key)
            || matches!(key, "PWD" | "SHLVL" | "_" | "OLDPWD")
            || key == "ADE_PLUGINS_TEST_EXTRA"
    }

    #[test]
    fn resolve_node_prefers_override_and_requires_a_file() {
        let dir = tempfile::tempdir().unwrap();
        let override_node = write_executable(dir.path(), "node-override", "#!/bin/sh\n");
        let hydrated = dir.path().to_str().unwrap();
        assert_eq!(
            resolve_node(Some(override_node.to_str().unwrap()), None).unwrap(),
            override_node
        );

        let missing = dir.path().join("does-not-exist");
        let error = resolve_node(Some(missing.to_str().unwrap()), Some(hydrated)).unwrap_err();
        assert_eq!(error.code(), NODE_MISSING);
    }

    #[test]
    fn resolve_node_searches_hydrated_path_before_process_path() {
        let dir = tempfile::tempdir().unwrap();
        let first = dir.path().join("first");
        let second = dir.path().join("second");
        std::fs::create_dir_all(&first).unwrap();
        std::fs::create_dir_all(&second).unwrap();
        let node = write_executable(&second, "node", "#!/bin/sh\n");
        let hydrated = format!("{}:{}", first.display(), second.display());
        assert_eq!(resolve_node(None, Some(&hydrated)).unwrap(), node);
    }

    #[test]
    fn resolve_node_falls_back_to_process_path() {
        let output = match Command::new("which").arg("node").output() {
            Ok(output) if output.status.success() => output,
            _ => return,
        };
        let which_node = PathBuf::from(String::from_utf8_lossy(&output.stdout).trim());
        if which_node.as_os_str().is_empty() {
            return;
        }
        let resolved = resolve_node(None, None).expect("node on PATH must resolve");
        assert_eq!(canonical(&resolved), canonical(&which_node));
    }

    #[test]
    fn node_supports_requires_major_eighteen() {
        assert!(node_supports("v22.21.0"));
        assert!(node_supports("v18.0.0\n"));
        assert!(node_supports("20.11.1"));
        assert!(!node_supports("v16.9.0"));
        assert!(!node_supports("v17.9.9"));
        assert!(!node_supports("garbage"));
        assert!(!node_supports(""));
        assert!(!node_supports("v18"));
        assert!(!node_supports("v18.0"));
        assert!(!node_supports("v18.0.0.1"));
    }

    /// Fake node that reports its cwd and environment, then stays alive until
    /// killed. The `cat` here-docs flush immediately (unlike shell builtins,
    /// whose output is lost when the shell is killed).
    const FAKE_NODE: &str = r#"#!/bin/sh
cat <<MARKER
CWD:$(pwd)
LEAK:${CARGO_MANIFEST_DIR:-unset}
EXTRA:${ADE_PLUGINS_TEST_EXTRA:-unset}
MARKER
env | sort | sed 's/^/ENV:/'
cat <<MARKER
ENV_END
MARKER
sleep 5
"#;

    fn read_until_marker(stdout: ChildStdout, marker: &'static str) -> String {
        let (sender, receiver) = mpsc::channel();
        thread::spawn(move || {
            let mut reader = BufReader::new(stdout);
            let mut text = String::new();
            loop {
                let mut line = String::new();
                match reader.read_line(&mut line) {
                    Ok(0) | Err(_) => break,
                    Ok(_) => {
                        text.push_str(&line);
                        if line.trim_end() == marker {
                            break;
                        }
                    }
                }
            }
            let _ = sender.send(text);
        });
        receiver
            .recv_timeout(Duration::from_secs(5))
            .expect("fake node must print its markers")
    }

    #[test]
    fn spawn_worker_env_cwd_and_sigterm_kill() {
        let dir = tempfile::tempdir().unwrap();
        let node = write_executable(dir.path(), "fake-node", FAKE_NODE);
        let host_entry = dir.path().join("host-entry.mjs");
        std::fs::write(&host_entry, "// fixture").unwrap();
        let plugin_root = dir.path().join("plugin-root");
        std::fs::create_dir_all(&plugin_root).unwrap();

        let mut handle = spawn_worker(
            &worker_spec(&plugin_root),
            &host_entry,
            &node,
            &[("ADE_PLUGINS_TEST_EXTRA".to_owned(), "kept".to_owned())],
        )
        .expect("spawn fake node");

        assert!(handle.stdin.is_some(), "stdin must be piped");
        assert!(handle.stdout.is_some(), "stdout must be piped");
        assert!(handle.stderr.is_some(), "stderr must be piped");
        assert!(
            process_alive(handle.pid),
            "worker must be alive after spawn"
        );

        let output = read_until_marker(handle.stdout.take().unwrap(), "ENV_END");
        let cwd_line = output
            .lines()
            .find(|line| line.starts_with("CWD:"))
            .expect("fake node must report its cwd");
        assert_eq!(
            canonical(Path::new(cwd_line.trim_start_matches("CWD:"))),
            canonical(&plugin_root),
            "worker must run in the plugin root"
        );
        assert!(output.contains("LEAK:unset"), "parent env leaked: {output}");
        assert!(output.contains("EXTRA:kept"), "extra_env dropped: {output}");
        if std::env::var_os("CARGO_MANIFEST_DIR").is_some() {
            assert!(
                !output.contains("ENV:CARGO_MANIFEST_DIR="),
                "non-allowlisted parent env var leaked: {output}"
            );
        }
        for line in output.lines().filter(|line| line.starts_with("ENV:")) {
            let key = line["ENV:".len()..]
                .split('=')
                .next()
                .expect("env line has a key");
            assert!(
                is_allowed_worker_env(key),
                "unexpected env var in worker: {key}"
            );
        }

        let started = Instant::now();
        handle.kill(Duration::from_millis(200));
        let elapsed = started.elapsed();
        assert!(
            elapsed < Duration::from_secs(2),
            "SIGTERM path must not wait out the grace period, took {elapsed:?}"
        );
        assert!(
            !process_alive(handle.pid),
            "worker must be terminated and reaped"
        );
        assert!(
            wait_group_gone(handle.pid, Duration::from_secs(2)),
            "worker process group leaked"
        );
    }

    /// Fake node that ignores SIGTERM; it reports readiness first so the test
    /// never races the `trap` builtin.
    const STUBBORN_NODE: &str = r#"#!/bin/sh
trap '' TERM
cat <<MARKER
READY
MARKER
sleep 5
"#;

    #[test]
    fn spawn_worker_sigkills_a_term_ignoring_child() {
        let dir = tempfile::tempdir().unwrap();
        let node = write_executable(dir.path(), "stubborn-node", STUBBORN_NODE);
        let host_entry = dir.path().join("host-entry.mjs");
        std::fs::write(&host_entry, "// fixture").unwrap();
        let plugin_root = dir.path().join("plugin-root");
        std::fs::create_dir_all(&plugin_root).unwrap();

        let mut handle = spawn_worker(&worker_spec(&plugin_root), &host_entry, &node, &[])
            .expect("spawn fake node");
        assert!(
            process_alive(handle.pid),
            "worker must be alive after spawn"
        );
        let ready = read_until_marker(handle.stdout.take().unwrap(), "READY");
        assert!(
            ready.contains("READY"),
            "fake node must be trapped: {ready}"
        );

        let started = Instant::now();
        handle.kill(Duration::from_millis(200));
        let elapsed = started.elapsed();
        assert!(
            elapsed >= Duration::from_millis(200),
            "must wait out the grace period before SIGKILL, took {elapsed:?}"
        );
        assert!(
            elapsed < Duration::from_secs(3),
            "SIGKILL path must stay bounded, took {elapsed:?}"
        );
        assert!(
            !process_alive(handle.pid),
            "worker must be killed and reaped"
        );
        assert!(
            wait_group_gone(handle.pid, Duration::from_secs(2)),
            "worker process group leaked"
        );
    }
}
