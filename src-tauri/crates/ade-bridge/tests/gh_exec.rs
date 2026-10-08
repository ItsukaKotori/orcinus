use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, OnceLock};
use std::time::{Duration, Instant};

static ENV_LOCK: Mutex<()> = Mutex::new(());
static ENV_INIT: OnceLock<()> = OnceLock::new();

fn env_lock() -> MutexGuard<'static, ()> {
    let guard = ENV_LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    ENV_INIT.get_or_init(|| ());
    guard
}

struct PathGuard {
    saved: Option<std::ffi::OsString>,
}
impl PathGuard {
    fn set(paths: &[&Path]) -> Self {
        let saved = std::env::var_os("PATH");
        let joined = std::env::join_paths(paths.iter().map(|p| p.as_os_str())).expect("join PATH");
        std::env::set_var("PATH", joined);
        Self { saved }
    }
}
impl Drop for PathGuard {
    fn drop(&mut self) {
        match &self.saved {
            Some(value) => std::env::set_var("PATH", value),
            None => std::env::remove_var("PATH"),
        }
    }
}

struct TestDir { path: PathBuf }
impl TestDir {
    fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "ade-bridge-gh-it-{name}-{}", std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("create test dir");
        Self { path: path.canonicalize().expect("canonicalize") }
    }
    fn file(&self, name: &str) -> PathBuf { self.path.join(name) }
    fn write_executable(&self, name: &str, body: &str) -> PathBuf {
        let path = self.file(name);
        std::fs::write(&path, body).expect("write script");
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).expect("chmod");
        path
    }
}
impl Drop for TestDir {
    fn drop(&mut self) { let _ = std::fs::remove_dir_all(&self.path); }
}

use ade_bridge::commands::gh::{gh_env_probe_impl, gh_exec_impl, read_timeout_ms, resolve_gh_path_in};
use ade_bridge::state::{GhConcurrencyGate, GhPathCache};

#[test]
fn gh_exec_returns_stdout_and_zero_code() {
    let _env = env_lock();
    let dir = TestDir::new("stdout");
    let gh = dir.write_executable("gh", "#!/bin/sh\necho hello\n");
    let result = gh_exec_impl(&gh, &[], None, Duration::from_secs(5), 1024, None).unwrap();
    assert_eq!(result.stdout.trim(), "hello");
    assert_eq!(result.stderr, "");
    assert_eq!(result.code, Some(0));
}

#[test]
fn gh_exec_keeps_nonzero_exit_as_result() {
    let _env = env_lock();
    let dir = TestDir::new("nonzero");
    let gh = dir.write_executable("gh", "#!/bin/sh\necho boom >&2\nexit 3\n");
    let result = gh_exec_impl(&gh, &[], None, Duration::from_secs(5), 1024, None).unwrap();
    assert_eq!(result.code, Some(3));
    assert!(result.stderr.contains("boom"));
}

#[test]
fn gh_exec_kills_process_group_on_timeout() {
    let _env = env_lock();
    let dir = TestDir::new("timeout");
    let pid_file = dir.file("grandchild.pid");
    let script = format!(
        "#!/bin/sh\nsleep 300 &\necho $! > '{}'\nsleep 300\n",
        pid_file.display()
    );
    let gh = dir.write_executable("gh", &script);
    let started = Instant::now();
    let error = gh_exec_impl(&gh, &[], None, Duration::from_millis(400), 1024, None).unwrap_err();
    assert!(started.elapsed() < Duration::from_secs(5), "must not wait for sleep 300");
    assert!(error.to_string().contains("timed out"));
    let pid: i32 = std::fs::read_to_string(&pid_file).unwrap().trim().parse().unwrap();
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        let alive = unsafe { libc::kill(pid, 0) } == 0;
        if !alive { break; }
        assert!(Instant::now() < deadline, "grandchild {pid} survived the group kill");
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[test]
fn gh_exec_enforces_max_buffer() {
    let _env = env_lock();
    let dir = TestDir::new("maxbuffer");
    let gh = dir.write_executable("gh", "#!/bin/sh\nhead -c 100000 /dev/zero | tr '\\0' 'a'\n");
    let error = gh_exec_impl(&gh, &[], None, Duration::from_secs(5), 4096, None).unwrap_err();
    assert!(error.to_string().contains("max buffer"));
}

#[test]
fn gh_exec_injects_prompt_disabled() {
    let _env = env_lock();
    std::env::remove_var("GH_PROMPT_DISABLED");
    let dir = TestDir::new("prompt");
    let gh = dir.write_executable("gh", "#!/bin/sh\nprintf '%s' \"$GH_PROMPT_DISABLED\"\n");
    let result = gh_exec_impl(&gh, &[], None, Duration::from_secs(5), 1024, None).unwrap();
    assert_eq!(result.stdout, "1");
}

#[test]
fn resolve_gh_path_falls_back_to_extra_dirs() {
    let _env = env_lock();
    let dir = TestDir::new("resolve");
    let extra = dir.file("extra");
    std::fs::create_dir_all(&extra).unwrap();
    let gh = extra.join("gh");
    std::fs::write(&gh, "#!/bin/sh\nexit 0\n").unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&gh, std::fs::Permissions::from_mode(0o755)).unwrap();

    let cache = GhPathCache::default();
    let empty = dir.file("empty");
    std::fs::create_dir_all(&empty).unwrap();
    let _guard = PathGuard::set(&[&empty]);
    let resolved = resolve_gh_path_in(&cache, &[extra.clone()]).unwrap();
    assert_eq!(resolved, gh);
    // 第二次命中缓存（删除文件后仍返回缓存路径）
    std::fs::remove_file(&gh).unwrap();
    assert_eq!(resolve_gh_path_in(&cache, &[extra]).unwrap(), gh);
}

#[test]
fn gh_gate_limits_concurrency() {
    let _env = env_lock();
    let dir = TestDir::new("gate");
    let gh = dir.write_executable("gh", "#!/bin/sh\nsleep 0.25\n");
    let gate = std::sync::Arc::new(GhConcurrencyGate::new(4));
    let started = Instant::now();
    let handles: Vec<_> = (0..6)
        .map(|_| {
            let gh = gh.clone();
            let gate = gate.clone();
            std::thread::spawn(move || {
                let _guard = gate.acquire();
                gh_exec_impl(&gh, &[], None, Duration::from_secs(5), 1024, None).unwrap()
            })
        })
        .collect();
    for handle in handles { handle.join().unwrap(); }
    let elapsed = started.elapsed();
    assert!(elapsed >= Duration::from_millis(450), "6 calls / 4 permits must take >=2 batches: {elapsed:?}");
    assert!(elapsed < Duration::from_millis(1500), "gate must not serialize: {elapsed:?}");
}

#[test]
fn read_timeout_ms_clamps_extreme_values() {
    assert_eq!(read_timeout_ms(Some(u64::MAX)), Duration::from_millis(600_000));
    assert_eq!(read_timeout_ms(Some(600_001)), Duration::from_millis(600_000));
    assert_eq!(read_timeout_ms(Some(1_500)), Duration::from_millis(1_500));
}

#[test]
fn gh_env_probe_reports_token() {
    let _env = env_lock();
    std::env::remove_var("GH_TOKEN");
    std::env::remove_var("GITHUB_TOKEN");
    assert_eq!(gh_env_probe_impl().token, None);
    std::env::set_var("GITHUB_TOKEN", "x");
    assert_eq!(gh_env_probe_impl().token.as_deref(), Some("GITHUB_TOKEN"));
    std::env::set_var("GH_TOKEN", "y");
    assert_eq!(gh_env_probe_impl().token.as_deref(), Some("GH_TOKEN"));
    std::env::remove_var("GH_TOKEN");
    std::env::remove_var("GITHUB_TOKEN");
}

#[test]
fn gh_exec_pipes_stdin_to_child() {
    let _env = env_lock();
    let dir = TestDir::new("stdin");
    let gh = dir.write_executable("gh", "#!/bin/sh\ncat\n");
    let result = gh_exec_impl(&gh, &[], None, Duration::from_secs(5), 1024, Some("body text")).unwrap();
    assert_eq!(result.stdout, "body text");
    assert_eq!(result.code, Some(0));
}

#[test]
fn gh_exec_without_stdin_leaves_child_stdin_closed() {
    let _env = env_lock();
    let dir = TestDir::new("no-stdin");
    let gh = dir.write_executable("gh", "#!/bin/sh\ncat\n");
    let result = gh_exec_impl(&gh, &[], None, Duration::from_secs(5), 1024, None).unwrap();
    assert_eq!(result.stdout, "");
}

#[test]
fn gh_exec_reports_missing_binary() {
    let _env = env_lock();
    let cache = GhPathCache::default();
    let dir = TestDir::new("missing");
    // Isolate PATH: the host may have a real gh installed (it does here),
    // which would otherwise make this "not found" assertion resolve.
    let empty = dir.file("empty");
    std::fs::create_dir_all(&empty).unwrap();
    let _guard = PathGuard::set(&[&empty]);
    let error = resolve_gh_path_in(&cache, &[dir.file("nope")]).unwrap_err();
    assert!(error.to_string().contains("gh: command not found"));
}
