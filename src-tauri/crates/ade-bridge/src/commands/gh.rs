use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use tauri::State;

use crate::commands::run_blocking;
use crate::errors::BridgeError;
use crate::state::{AppState, GhPathCache};

const DEFAULT_TIMEOUT_MS: u64 = 30_000;
const DEFAULT_MAX_BUFFER: usize = 10 * 1024 * 1024;
const POLL_INTERVAL: Duration = Duration::from_millis(10);

#[derive(Debug, Clone, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct GhExecArgs {
    pub args: Vec<String>,
    #[serde(default)]
    pub cwd: Option<String>,
    #[serde(default)]
    pub timeout_ms: Option<u64>,
    #[serde(default)]
    pub max_buffer: Option<usize>,
}

#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct GhExecResult {
    pub stdout: String,
    pub stderr: String,
    pub code: Option<i32>,
}

#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct GhEnvProbe {
    pub token: Option<String>,
}

/// 生产用附加搜索目录（GUI 应用 PATH 不含 Homebrew）。
fn default_extra_dirs(home: &str) -> Vec<PathBuf> {
    vec![
        PathBuf::from("/opt/homebrew/bin"),
        PathBuf::from("/usr/local/bin"),
        PathBuf::from(home).join(".local/bin"),
        PathBuf::from(home).join("bin"),
    ]
}

fn is_executable_file(path: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::metadata(path)
            .map(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0)
            .unwrap_or(false)
    }
    #[cfg(not(unix))]
    {
        path.is_file()
    }
}

/// 在 `PATH` 目录 + 附加目录中解析 `gh`；成功写缓存，失败每次重试。
pub fn resolve_gh_path_in(cache: &GhPathCache, extra_dirs: &[PathBuf]) -> Result<PathBuf, BridgeError> {
    if let Some(cached) = cache.get() {
        return Ok(cached);
    }
    let mut dirs: Vec<PathBuf> = std::env::var_os("PATH")
        .map(|value| std::env::split_paths(&value).collect())
        .unwrap_or_default();
    dirs.extend(extra_dirs.iter().cloned());
    for dir in dirs {
        let candidate = dir.join("gh");
        if is_executable_file(&candidate) {
            cache.set(candidate.clone());
            return Ok(candidate);
        }
    }
    Err(BridgeError::message("gh: command not found on PATH"))
}

pub fn read_timeout_ms(value: Option<u64>) -> Duration {
    let millis = value
        .or_else(|| {
            std::env::var("ORCA_GH_EXEC_TIMEOUT_MS")
                .ok()
                .and_then(|raw| raw.trim().parse::<u64>().ok())
        })
        .unwrap_or(DEFAULT_TIMEOUT_MS);
    Duration::from_millis(millis.max(1))
}

/// 运行 gh：非零退出是 Ok 结果；spawn/超时/超限是 Err。
pub fn gh_exec_impl(
    gh_path: &Path,
    args: &[String],
    cwd: Option<&str>,
    timeout: Duration,
    max_buffer: usize,
) -> Result<GhExecResult, BridgeError> {
    let mut command = Command::new(gh_path);
    command.args(args);
    if let Some(cwd) = cwd.filter(|value| !value.trim().is_empty()) {
        command.current_dir(cwd);
    }
    if std::env::var_os("GH_PROMPT_DISABLED").is_none() {
        command.env("GH_PROMPT_DISABLED", "1");
    }
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // Why: gh 常是 mise/asdf shim；独立进程组保证超时能连孙进程一起杀。
        command.process_group(0);
    }
    let mut child = command.spawn().map_err(|error| {
        BridgeError::message(format!("gh: command not found (spawn failed: {error})"))
    })?;
    let stdout_pipe = child.stdout.take();
    let stderr_pipe = child.stderr.take();
    let exceeded = Arc::new(AtomicBool::new(false));
    let stdout_handle = stdout_pipe.map(|pipe| spawn_reader(pipe, max_buffer, exceeded.clone()));
    let stderr_handle = stderr_pipe.map(|pipe| spawn_reader(pipe, max_buffer, exceeded.clone()));

    let deadline = Instant::now() + timeout;
    let status = loop {
        if exceeded.load(Ordering::SeqCst) {
            kill_process_group(&mut child);
            return Err(BridgeError::message("gh exec output exceeded max buffer"));
        }
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {}
            Err(error) => {
                kill_process_group(&mut child);
                return Err(BridgeError::message(format!("gh exec wait failed: {error}")));
            }
        }
        if Instant::now() >= deadline {
            kill_process_group(&mut child);
            return Err(BridgeError::message("gh exec timed out"));
        }
        std::thread::sleep(POLL_INTERVAL);
    };

    let stdout = join_reader(stdout_handle)?;
    let stderr = join_reader(stderr_handle)?;
    if exceeded.load(Ordering::SeqCst) {
        return Err(BridgeError::message("gh exec output exceeded max buffer"));
    }
    Ok(GhExecResult { stdout, stderr, code: status.code() })
}

fn spawn_reader(
    mut pipe: impl Read + Send + 'static,
    max_buffer: usize,
    exceeded: Arc<AtomicBool>,
) -> std::thread::JoinHandle<String> {
    std::thread::spawn(move || {
        let mut collected: Vec<u8> = Vec::new();
        let mut buffer = [0u8; 8192];
        loop {
            match pipe.read(&mut buffer) {
                Ok(0) | Err(_) => break,
                Ok(read) => {
                    if collected.len() + read > max_buffer {
                        exceeded.store(true, Ordering::SeqCst);
                        break;
                    }
                    collected.extend_from_slice(&buffer[..read]);
                }
            }
        }
        String::from_utf8_lossy(&collected).into_owned()
    })
}

fn join_reader(handle: Option<std::thread::JoinHandle<String>>) -> Result<String, BridgeError> {
    match handle {
        Some(handle) => handle
            .join()
            .map_err(|_| BridgeError::message("gh exec reader panicked")),
        None => Ok(String::new()),
    }
}

fn kill_process_group(child: &mut std::process::Child) {
    #[cfg(unix)]
    {
        let pid = child.id() as i32;
        // Why: 子进程自成进程组（process_group(0)），负 pid 连组内孙进程一起杀。
        unsafe {
            libc::kill(-pid, libc::SIGKILL);
        }
    }
    #[cfg(not(unix))]
    {
        let _ = child.kill();
    }
    let _ = child.wait();
}

pub fn gh_env_probe_impl() -> GhEnvProbe {
    let token = if std::env::var_os("GH_TOKEN").is_some() {
        Some("GH_TOKEN".to_string())
    } else if std::env::var_os("GITHUB_TOKEN").is_some() {
        Some("GITHUB_TOKEN".to_string())
    } else {
        None
    };
    GhEnvProbe { token }
}

#[tauri::command]
#[specta::specta]
pub async fn gh_exec(state: State<'_, AppState>, args: GhExecArgs) -> Result<GhExecResult, BridgeError> {
    let gate = Arc::clone(&state.gh_gate);
    let cache = state.gh_path_cache.clone();
    let extra_dirs = default_extra_dirs(&state.home);
    run_blocking(move || {
        let _guard = gate.acquire();
        let gh_path = resolve_gh_path_in(&cache, &extra_dirs)?;
        let timeout = read_timeout_ms(args.timeout_ms);
        let max_buffer = args.max_buffer.unwrap_or(DEFAULT_MAX_BUFFER);
        gh_exec_impl(&gh_path, &args.args, args.cwd.as_deref(), timeout, max_buffer)
    })
    .await
}

#[tauri::command]
#[specta::specta]
pub async fn gh_env_probe() -> Result<GhEnvProbe, BridgeError> {
    Ok(gh_env_probe_impl())
}
