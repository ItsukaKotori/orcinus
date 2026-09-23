pub mod porcelain;

pub use porcelain::{parse_worktree_list, GitWorktreeEntry};

use ade_core::errors::CoreError;
use std::io::Read;
use std::process::{Child, Command, ExitStatus, Output, Stdio};
use std::time::{Duration, Instant};

const VERSION_TIMEOUT: Duration = Duration::from_millis(1500);
const COMMAND_TIMEOUT: Duration = Duration::from_secs(10);

pub fn is_available() -> bool {
    let mut command = Command::new("git");
    command.arg("--version");
    matches!(
        output_with_timeout(&mut command, VERSION_TIMEOUT),
        Ok(output) if output.status.success()
    )
}

pub fn rev_parse_toplevel(path: &str) -> Result<String, CoreError> {
    let output = run_git(path, &["rev-parse", "--show-toplevel"])?;
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

pub fn is_inside_work_tree(path: &str) -> bool {
    match run_git(path, &["rev-parse", "--is-inside-work-tree"]) {
        Ok(output) => String::from_utf8_lossy(&output.stdout).trim() == "true",
        Err(_) => false,
    }
}

pub fn worktree_list(path: &str) -> Result<Vec<GitWorktreeEntry>, CoreError> {
    let output = run_git(path, &["worktree", "list", "--porcelain", "-z"])?;
    Ok(parse_worktree_list(&output.stdout))
}

fn run_git(path: &str, args: &[&str]) -> Result<Output, CoreError> {
    let mut command = Command::new("git");
    command.arg("-C").arg(path).args(args);
    let output = output_with_timeout(&mut command, COMMAND_TIMEOUT)?;
    if output.status.success() {
        Ok(output)
    } else {
        Err(CoreError::NotAGitRepository(path.to_string()))
    }
}

fn output_with_timeout(command: &mut Command, timeout: Duration) -> std::io::Result<Output> {
    let mut child = command
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let stdout = child.stdout.take().expect("stdout is piped");
    let stderr = child.stderr.take().expect("stderr is piped");
    let stdout_reader = std::thread::spawn(move || read_all(stdout));
    let stderr_reader = std::thread::spawn(move || read_all(stderr));

    let status = match wait_with_timeout(&mut child, timeout) {
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

    Ok(Output {
        status,
        stdout: stdout_reader.join().expect("stdout reader panicked")?,
        stderr: stderr_reader.join().expect("stderr reader panicked")?,
    })
}

fn wait_with_timeout(child: &mut Child, timeout: Duration) -> std::io::Result<Option<ExitStatus>> {
    let deadline = Instant::now() + timeout;
    loop {
        if let Some(status) = child.try_wait()? {
            return Ok(Some(status));
        }
        if Instant::now() >= deadline {
            return Ok(None);
        }
        std::thread::sleep(Duration::from_millis(10));
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

    #[cfg(unix)]
    #[test]
    fn output_with_timeout_kills_slow_command() {
        let mut command = Command::new("sleep");
        command.arg("30");

        let started = Instant::now();
        let error = output_with_timeout(&mut command, Duration::from_millis(200))
            .expect_err("slow command should time out");

        assert_eq!(error.kind(), std::io::ErrorKind::TimedOut);
        assert!(started.elapsed() < Duration::from_secs(5));
    }

    #[cfg(unix)]
    #[test]
    fn output_with_timeout_captures_stdout_and_exit_status() {
        let mut command = Command::new("echo");
        command.arg("hello");

        let output =
            output_with_timeout(&mut command, Duration::from_secs(5)).expect("echo should run");

        assert!(output.status.success());
        assert_eq!(String::from_utf8_lossy(&output.stdout), "hello\n");
    }
}
