//! Plugin Git process execution and the primitives the marketplace pipeline
//! is built on.
//!
//! Ported from `orca:src/main/plugins/plugin-git-repository.ts`: argv-only
//! invocation of system Git (no shell, no remote-helper surface),
//! `clone --depth 1 [--branch ref]` for named refs and
//! `init + fetch --depth 1 <sha> + checkout FETCH_HEAD` for exact commits,
//! then `rev-parse HEAD` validated against the lockfile commit shape.
//!
//! Fork specifics: the timeout/cancel machinery mirrors the `ade-git` runner
//! (10ms poll, kill + reap on cancellation and timeout) but reports through
//! [`PluginError`]. URLs pass the same HTTPS/SSH whitelist as the lockfile.

use std::io::Read;
use std::path::Path;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::install::lockfile::{is_plugin_commit, is_plugin_git_url};
use crate::types::{PluginError, INVALID_PARAMS, IO_ERROR};

/// Oracle parity: `PLUGIN_GIT_TIMEOUT_MS = 120_000`.
pub const DEFAULT_PLUGIN_GIT_TIMEOUT: Duration = Duration::from_secs(120);
const POLL_INTERVAL: Duration = Duration::from_millis(10);

/// Plugin Git URL policy. Production accepts exactly the lockfile HTTPS/SSH
/// whitelist; unit tests additionally accept absolute local paths so the
/// marketplace pipeline can run against temporary repositories without
/// network access.
pub(crate) fn is_allowed_git_url(url: &str) -> bool {
    if is_plugin_git_url(url) {
        return true;
    }
    #[cfg(test)]
    if Path::new(url).is_absolute() {
        return true;
    }
    false
}

/// Non-zero exits stay an `Ok` value so callers can inspect stderr; only
/// spawn, timeout, and cancellation failures are `Err`.
#[derive(Debug)]
pub struct GitOutput {
    pub status: ExitStatus,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

impl GitOutput {
    pub fn success(&self) -> bool {
        self.status.success()
    }

    pub fn stdout_trimmed(&self) -> String {
        String::from_utf8_lossy(&self.stdout).trim().to_string()
    }

    pub fn stderr_trimmed(&self) -> String {
        String::from_utf8_lossy(&self.stderr).trim().to_string()
    }
}

#[derive(Clone, Default)]
pub struct CancelToken {
    flag: Arc<AtomicBool>,
}

impl CancelToken {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn cancel(&self) {
        self.flag.store(true, Ordering::SeqCst);
    }

    pub fn is_cancelled(&self) -> bool {
        self.flag.load(Ordering::SeqCst)
    }
}

impl std::fmt::Debug for CancelToken {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("CancelToken")
            .field("cancelled", &self.is_cancelled())
            .finish()
    }
}

/// Runs system Git for the plugin pipeline. Holds the default command timeout;
/// tests build one with [`GitRunner::local_test`].
#[derive(Debug, Clone)]
pub struct GitRunner {
    timeout: Duration,
    allow_local_paths: bool,
}

impl Default for GitRunner {
    fn default() -> Self {
        Self::new(DEFAULT_PLUGIN_GIT_TIMEOUT)
    }
}

impl GitRunner {
    pub fn new(timeout: Duration) -> Self {
        Self {
            timeout,
            allow_local_paths: false,
        }
    }

    /// Runner for tests: absolute local repository paths are accepted in place
    /// of network URLs.
    #[cfg(test)]
    pub(crate) fn local_test() -> Self {
        Self {
            timeout: DEFAULT_PLUGIN_GIT_TIMEOUT,
            allow_local_paths: true,
        }
    }

    pub fn timeout(&self) -> Duration {
        self.timeout
    }

    fn ensure_url(&self, url: &str) -> Result<(), PluginError> {
        if is_plugin_git_url(url) || (self.allow_local_paths && Path::new(url).is_absolute()) {
            return Ok(());
        }
        Err(PluginError::new(
            INVALID_PARAMS,
            "plugin Git URL must use HTTPS or SSH",
        ))
    }

    /// Spawns `git [-C cwd] args`, captures both streams, and polls every 10ms
    /// for completion, cancellation, or the deadline. The child is killed and
    /// reaped on cancellation and timeout.
    pub fn run(
        &self,
        cwd: Option<&Path>,
        args: &[&str],
        timeout: Duration,
        cancel: Option<&CancelToken>,
    ) -> Result<GitOutput, PluginError> {
        let mut command = Command::new("git");
        if let Some(cwd) = cwd {
            command.arg("-C").arg(cwd);
        }
        command.args(args);
        let label = args.first().copied().unwrap_or("command");
        let output = run_process(&mut command, timeout, cancel).map_err(|error| {
            let message = match error.kind() {
                std::io::ErrorKind::TimedOut => format!("git {label} timed out"),
                std::io::ErrorKind::Interrupted => format!("git {label} was cancelled"),
                _ => format!("failed to run git {label}: {error}"),
            };
            PluginError::new(IO_ERROR, message)
        })?;
        Ok(GitOutput {
            status: output.status,
            stdout: output.stdout,
            stderr: output.stderr,
        })
    }

    fn run_default(&self, cwd: Option<&Path>, args: &[&str]) -> Result<GitOutput, PluginError> {
        self.run(cwd, args, self.timeout, None)
    }

    fn run_checked(
        &self,
        cwd: Option<&Path>,
        args: &[&str],
        label: &str,
    ) -> Result<GitOutput, PluginError> {
        let output = self.run_default(cwd, args)?;
        if !output.status.success() {
            let stderr = output.stderr_trimmed();
            let detail = if stderr.is_empty() {
                format!("exit status {}", output.status)
            } else {
                stderr
            };
            return Err(PluginError::new(
                IO_ERROR,
                format!("{label} failed: {detail}"),
            ));
        }
        Ok(output)
    }

    /// Checks out one Git ref into `dest` and returns the exact resolved HEAD.
    ///
    /// A full commit hash is fetched directly (`init + fetch --depth 1 <sha> +
    /// checkout FETCH_HEAD`); any other ref becomes
    /// `clone --depth 1 [--branch ref]`.
    pub fn clone_depth1(
        &self,
        url: &str,
        ref_: Option<&str>,
        dest: &Path,
    ) -> Result<String, PluginError> {
        self.ensure_url(url)?;
        let reference = ref_
            .map(str::trim)
            .filter(|reference| !reference.is_empty());
        if let Some(reference) = reference.filter(|reference| is_plugin_commit(reference)) {
            return self.fetch_commit(url, reference, dest);
        }
        let destination = dest.to_string_lossy().into_owned();
        // `--no-local` keeps `--depth` effective when a test points the runner
        // at a local repository; it is inert for network remotes.
        let mut args = vec!["clone", "--quiet", "--depth", "1", "--no-local"];
        if let Some(reference) = reference {
            args.push("--branch");
            args.push(reference);
        }
        args.push("--");
        args.push(url);
        args.push(&destination);
        self.run_checked(None, &args, "git clone")?;
        self.resolve_head(dest)
    }

    /// Fetches one exact commit into an empty destination
    /// (`init + remote add + fetch --depth 1 <sha> + checkout FETCH_HEAD`) and
    /// returns the resolved HEAD.
    pub fn fetch_commit(&self, url: &str, sha: &str, dest: &Path) -> Result<String, PluginError> {
        self.ensure_url(url)?;
        if !is_plugin_commit(sha) {
            return Err(PluginError::new(
                INVALID_PARAMS,
                "plugin Git ref must be a full commit hash",
            ));
        }
        let destination = dest.to_string_lossy().into_owned();
        self.run_checked(None, &["init", "--quiet", &destination], "git init")?;
        self.run_checked(
            Some(dest),
            &["remote", "add", "origin", url],
            "git remote add",
        )?;
        self.run_checked(
            Some(dest),
            &["fetch", "--quiet", "--depth", "1", "origin", sha],
            "git fetch",
        )?;
        self.run_checked(
            Some(dest),
            &["checkout", "--quiet", "FETCH_HEAD"],
            "git checkout",
        )?;
        self.resolve_head(dest)
    }

    /// `git -C dir rev-parse HEAD`, validated as a full commit identity.
    pub fn resolve_head(&self, dir: &Path) -> Result<String, PluginError> {
        let output = self.run_checked(Some(dir), &["rev-parse", "HEAD"], "git rev-parse")?;
        let head = output.stdout_trimmed();
        if !is_plugin_commit(&head) {
            return Err(PluginError::new(
                IO_ERROR,
                "Git resolved an invalid commit identity",
            ));
        }
        Ok(head)
    }

    /// Resolves `ref_` (branch or tag name) to its commit without cloning.
    /// Missing or unmatched refs return `None`.
    pub fn ls_remote(&self, url: &str, ref_: &str) -> Result<Option<String>, PluginError> {
        self.ensure_url(url)?;
        let reference = ref_.trim();
        if reference.is_empty() {
            return Err(PluginError::new(
                INVALID_PARAMS,
                "marketplace source ref is required",
            ));
        }
        let output =
            self.run_checked(None, &["ls-remote", "--", url, reference], "git ls-remote")?;
        let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
        let branch = format!("refs/heads/{reference}");
        let tag = format!("refs/tags/{reference}");
        let suffix = format!("/{reference}");
        let mut fallback = None;
        for line in stdout.lines() {
            let mut parts = line.split_whitespace();
            let (Some(sha), Some(name)) = (parts.next(), parts.next()) else {
                continue;
            };
            if !is_plugin_commit(sha) {
                continue;
            }
            if name == branch || name == tag {
                return Ok(Some(sha.to_string()));
            }
            if fallback.is_none() && name.ends_with(&suffix) {
                fallback = Some(sha.to_string());
            }
        }
        Ok(fallback)
    }
}

fn run_process(
    command: &mut Command,
    timeout: Duration,
    cancel: Option<&CancelToken>,
) -> std::io::Result<std::process::Output> {
    let mut child = command
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let stdout = child.stdout.take().expect("stdout is piped");
    let stderr = child.stderr.take().expect("stderr is piped");
    let stdout_reader = std::thread::spawn(move || read_all(stdout));
    let stderr_reader = std::thread::spawn(move || read_all(stderr));

    let status = match wait_with_timeout(&mut child, timeout, cancel) {
        Ok(Some(status)) => status,
        Ok(None) => {
            reap(&mut child);
            let _ = stdout_reader.join();
            let _ = stderr_reader.join();
            return Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "git command timed out",
            ));
        }
        Err(error) => {
            reap(&mut child);
            let _ = stdout_reader.join();
            let _ = stderr_reader.join();
            return Err(error);
        }
    };

    Ok(std::process::Output {
        status,
        stdout: stdout_reader.join().expect("stdout reader panicked")?,
        stderr: stderr_reader.join().expect("stderr reader panicked")?,
    })
}

fn wait_with_timeout(
    child: &mut Child,
    timeout: Duration,
    cancel: Option<&CancelToken>,
) -> std::io::Result<Option<ExitStatus>> {
    let deadline = Instant::now() + timeout;
    loop {
        if let Some(status) = child.try_wait()? {
            return Ok(Some(status));
        }
        if cancel.is_some_and(CancelToken::is_cancelled) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::Interrupted,
                "git command cancelled",
            ));
        }
        if Instant::now() >= deadline {
            return Ok(None);
        }
        std::thread::sleep(POLL_INTERVAL);
    }
}

fn reap(child: &mut Child) {
    let _ = child.kill();
    let _ = child.wait();
}

fn read_all(mut reader: impl Read) -> std::io::Result<Vec<u8>> {
    let mut buffer = Vec::new();
    reader.read_to_end(&mut buffer)?;
    Ok(buffer)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;
    use std::process::Command;
    use tempfile::tempdir;

    pub(crate) fn git(cwd: &Path, args: &[&str]) {
        let status = Command::new("git")
            .args(args)
            .current_dir(cwd)
            .status()
            .expect("git runs");
        assert!(status.success(), "git {args:?} failed");
    }

    fn init_repo(dir: &Path) {
        fs::create_dir_all(dir).unwrap();
        git(dir, &["init", "--quiet", "-b", "main"]);
        git(dir, &["config", "user.email", "tests@example.com"]);
        git(dir, &["config", "user.name", "Tests"]);
        git(dir, &["config", "commit.gpgsign", "false"]);
        git(dir, &["config", "tag.gpgsign", "false"]);
        git(dir, &["config", "uploadpack.allowAnySHA1InWant", "true"]);
    }

    fn commit_all(dir: &Path, message: &str) -> String {
        git(dir, &["add", "-A"]);
        git(dir, &["commit", "--quiet", "-m", message]);
        runner().resolve_head(dir).unwrap()
    }

    fn runner() -> GitRunner {
        GitRunner::local_test()
    }

    fn repo_url(path: &Path) -> String {
        path.to_string_lossy().into_owned()
    }

    fn commit_count(dir: &Path) -> usize {
        let output = Command::new("git")
            .args(["rev-list", "--count", "HEAD"])
            .current_dir(dir)
            .output()
            .unwrap();
        String::from_utf8_lossy(&output.stdout)
            .trim()
            .parse()
            .unwrap()
    }

    fn sample_repo(dir: &Path) -> PathBuf {
        init_repo(dir);
        fs::write(dir.join("data.txt"), b"first\n").unwrap();
        commit_all(dir, "first");
        fs::write(dir.join("data.txt"), b"second\n").unwrap();
        commit_all(dir, "second");
        dir.to_path_buf()
    }

    #[test]
    fn git_clone_depth1_of_a_local_repo_resolves_head() {
        let temp = tempdir().unwrap();
        let source = sample_repo(&temp.path().join("source"));
        let expected = runner().resolve_head(&source).unwrap();

        let dest = temp.path().join("clone");
        let head = runner()
            .clone_depth1(&repo_url(&source), Some("main"), &dest)
            .unwrap();

        assert_eq!(head, expected);
        assert_eq!(fs::read(dest.join("data.txt")).unwrap(), b"second\n");
        assert_eq!(commit_count(&dest), 1, "clone must be depth-1");

        let dest_without_ref = temp.path().join("clone-default");
        let head = runner()
            .clone_depth1(&repo_url(&source), None, &dest_without_ref)
            .unwrap();
        assert_eq!(head, expected);
    }

    #[test]
    fn git_fetch_commit_checks_out_the_exact_commit() {
        let temp = tempdir().unwrap();
        let source = temp.path().join("source");
        init_repo(&source);
        fs::write(source.join("data.txt"), b"first\n").unwrap();
        let first = commit_all(&source, "first");
        fs::write(source.join("data.txt"), b"second\n").unwrap();
        let second = commit_all(&source, "second");

        let dest = temp.path().join("fetched");
        let head = runner()
            .fetch_commit(&repo_url(&source), &first, &dest)
            .unwrap();
        assert_eq!(head, first);
        assert_eq!(fs::read(dest.join("data.txt")).unwrap(), b"first\n");

        let via_clone = temp.path().join("by-sha");
        let head = runner()
            .clone_depth1(&repo_url(&source), Some(&second), &via_clone)
            .unwrap();
        assert_eq!(head, second);
        assert_eq!(fs::read(via_clone.join("data.txt")).unwrap(), b"second\n");
    }

    #[test]
    fn git_ls_remote_resolves_known_refs_and_returns_none_for_unknown_ones() {
        let temp = tempdir().unwrap();
        let source = sample_repo(&temp.path().join("source"));
        let head = runner().resolve_head(&source).unwrap();

        assert_eq!(
            runner().ls_remote(&repo_url(&source), "main").unwrap(),
            Some(head)
        );
        assert_eq!(
            runner().ls_remote(&repo_url(&source), "missing").unwrap(),
            None
        );
    }

    #[test]
    fn git_rejects_urls_outside_the_https_ssh_whitelist() {
        let temp = tempdir().unwrap();
        let dest = temp.path().join("clone");
        for url in [
            "http://github.com/example/demo.git",
            "git://github.com/example/demo.git",
            "file:///tmp/demo",
            "ext::sh -c whoami",
            "ssh://user:pass@github.com/example/demo.git",
            "https://user@github.com/example/demo.git",
        ] {
            let error = runner().clone_depth1(url, Some("main"), &dest).unwrap_err();
            assert_eq!(error.code(), INVALID_PARAMS, "expected rejection: {url}");
            assert_eq!(error.message(), "plugin Git URL must use HTTPS or SSH");
        }
        assert!(!dest.exists());

        let absolute = temp.path().join("local-repo");
        assert!(GitRunner::default()
            .clone_depth1(absolute.to_str().unwrap(), Some("main"), &dest)
            .is_err());
        assert!(!dest.exists());
    }

    #[test]
    fn git_cancellation_kills_and_reaps_the_child() {
        let token = CancelToken::new();
        let canceller = token.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(150));
            canceller.cancel();
        });
        let mut command = Command::new("sleep");
        command.arg("30");
        let started = Instant::now();
        let error = run_process(&mut command, Duration::from_secs(10), Some(&token)).unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::Interrupted);
        assert!(started.elapsed() < Duration::from_secs(5));
    }
}
