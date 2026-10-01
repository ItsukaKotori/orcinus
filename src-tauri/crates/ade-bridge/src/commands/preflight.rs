//! preflight 域命令：`preflight_refresh_agents`——登录 shell PATH 水合 +
//! agent CLI 探测（claude/codex），供渲染层 agent 选择器接活。
//!
//! 流程（brief 定型）：登录 shell 水合（进程内缓存，进程生命周期最多一次）→
//! `addedPathSegments` = 水合 PATH 段 − app 进程 PATH 段（保持水合序）→
//! `pathSource`/`pathFailureReason` 归类 → `agents = detect_agents(有效PATH)`。
//! 水合起子进程（可达 2s 超时），命令层置于 [`crate::commands::run_blocking`]
//! 内执行；`detect_agents` 与解析均为纯函数（`platform` 参数注入，可测）。

use std::collections::HashSet;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tauri::State;

use crate::commands::run_blocking;
use crate::errors::BridgeError;
use crate::state::{lock, AppState};

/// windows 平台标记（[`hydrate_login_path`] / [`detect_agents`] 的 `platform`
/// 分支判据；其余取值一律按 unix 规则处理，同 `ade-pty::shell` 惯例）。
const PLATFORM_WINDOWS: &str = "windows";

/// 登录 shell PATH 水合超时（brief 定值 2s）。
pub const LOGIN_PATH_HYDRATION_TIMEOUT: Duration = Duration::from_secs(2);

/// 探测的 agent CLI 集合（brief 定值）。
pub const AGENT_PROBES: &[&str] = &["claude", "codex"];

/// windows 探测后缀（PATHEXT 常见集合，按优先序）。
const WINDOWS_PATHEXT: &[&str] = &[".com", ".exe", ".cmd", ".bat"];

/// 契约 `PathSource`（`shell-path-hydration-types.ts`）。字面量本身是
/// snake_case——variant CamelCase 经 `rename_all = "snake_case"` 精确对齐
/// `'shell_hydrate' | 'sync_seed_only'`。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum PathSource {
    ShellHydrate,
    SyncSeedOnly,
}

/// 契约 `ShellHydrationFailureReason`。同上：`'none' | 'no_shell' | 'timeout'
/// | 'spawn_error' | 'empty_path'`。brief 中的水合失败 `FailureKind` 与该契约
/// 枚举一一对应，直接复用本类型（分类 1:1，免二次映射）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum PathFailureReason {
    None,
    NoShell,
    Timeout,
    SpawnError,
    EmptyPath,
}

/// 一次登录 shell PATH 水合的原始结果（探测前的分类输出）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PathHydration {
    pub path: Option<String>,
    pub failure: PathFailureReason,
}

/// 契约 `RefreshAgentsResult`（`preflight-api.ts`）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct RefreshAgentsResult {
    pub agents: Vec<String>,
    pub added_path_segments: Vec<String>,
    pub shell_hydration_ok: bool,
    pub path_source: PathSource,
    pub path_failure_reason: PathFailureReason,
}

/// 契约 `preflight.refreshAgents` 的 `args`（`PreflightRuntimeContext`）：A 的
/// 本机探测不分支远程/WSL——收下忽略，仅为 IPC 形状对齐。
#[derive(Debug, Clone, Default, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct PreflightRuntimeContext {
    #[serde(default)]
    pub wsl_distro: Option<String>,
    #[serde(default)]
    pub wsl_default: Option<bool>,
    #[serde(default)]
    pub project_runtime: Option<crate::json::Json>,
}

/// 登录 shell 水合后的进程内缓存快照：水合 PATH、相对 app 进程 PATH 的新增
/// 段与失败分类。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CachedHydration {
    pub path: Option<String>,
    pub added_segments: Vec<String>,
    pub failure: PathFailureReason,
}

impl CachedHydration {
    /// 由一次原始水合 + app 进程 PATH 组装缓存快照。
    pub fn from_hydration(hydration: &PathHydration, process_path: &str, platform: &str) -> Self {
        let hydrated = hydration.path.as_deref().unwrap_or_default();
        Self {
            path: hydration.path.clone(),
            added_segments: added_path_segments(hydrated, process_path, platform),
            failure: hydration.failure,
        }
    }
}

/// 登录 shell PATH 水合的进程内缓存：进程生命周期内最多水合一次（登录 shell
/// 启动代价高），后续 `preflight_refresh_agents` 直接复用。沿 `state.rs` 的
/// `PtyWorktreeIds` 风格——`Arc<Mutex<Option<_>>>` 新类型。
#[derive(Clone, Default)]
pub struct PathHydrationCache(Arc<Mutex<Option<CachedHydration>>>);

impl PathHydrationCache {
    pub fn get(&self) -> Option<CachedHydration> {
        lock(&self.0).clone()
    }

    pub fn set(&self, value: CachedHydration) {
        *lock(&self.0) = Some(value);
    }
}

/// 路径分隔符：windows `;`，其余 `:`。
fn path_separator(platform: &str) -> char {
    if platform == PLATFORM_WINDOWS {
        ';'
    } else {
        ':'
    }
}

/// 登录 shell PATH 水合：unix 跑 `shell -l -c 'echo $PATH'`，取 stdout
/// **最后一个非空行**且须含路径分隔符（`:` unix / `;` windows）；windows
/// 直接 `(None, no_shell)`。失败分类：起不动→`spawn_error`、超时→`timeout`、
/// 输出无有效行→`empty_path`。阻塞调用——调用方须置于 `run_blocking` 内。
pub fn hydrate_login_path(platform: &str, shell: &str, timeout: Duration) -> PathHydration {
    if platform == PLATFORM_WINDOWS {
        return PathHydration {
            path: None,
            failure: PathFailureReason::NoShell,
        };
    }
    let separator = path_separator(platform);
    // 子进程 + 线程/通道：超时后本函数即返回，孤儿 shell 由进程生命周期
    // 兜底（水合命令有 2s 上限，不会长期挂起）。
    let (sender, receiver) = std::sync::mpsc::channel();
    let program = shell.to_string();
    std::thread::spawn(move || {
        let output = std::process::Command::new(&program)
            .arg("-l")
            .arg("-c")
            .arg("echo $PATH")
            .output();
        let _ = sender.send(output);
    });
    let stdout = match receiver.recv_timeout(timeout) {
        Ok(Ok(output)) => String::from_utf8_lossy(&output.stdout).into_owned(),
        // 起不动（不存在/不可执行）或水合线程 panic → spawn_error。
        Ok(Err(_)) | Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
            return PathHydration {
                path: None,
                failure: PathFailureReason::SpawnError,
            };
        }
        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
            return PathHydration {
                path: None,
                failure: PathFailureReason::Timeout,
            };
        }
    };
    match parse_login_path_output(&stdout, separator) {
        Some(path) => PathHydration {
            path: Some(path),
            failure: PathFailureReason::None,
        },
        None => PathHydration {
            path: None,
            failure: PathFailureReason::EmptyPath,
        },
    }
}

/// 从 shell stdout 取最后一个非空行；该行须含路径分隔符才算有效 PATH
/// （banner / 无关尾巴一律判无效，由调用方归 `empty_path`）。
fn parse_login_path_output(output: &str, separator: char) -> Option<String> {
    output
        .lines()
        .rev()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .filter(|line| line.contains(separator))
        .map(str::to_string)
}

/// 沿 `path_var` 探测 agent CLI：unix 逐目录判 `<dir>/<probe>` 可执行文件
/// （mode & 0o111）；windows 逐目录 × PATHEXT（`.com/.exe/.cmd/.bat`）。
/// 命中去重，PATH 序优先（先命中者胜）。
pub fn detect_agents(path_var: &str, probes: &[&str], platform: &str) -> Vec<String> {
    let windows = platform == PLATFORM_WINDOWS;
    let separator = path_separator(platform);
    let mut agents: Vec<String> = Vec::new();
    for dir in path_var.split(separator).filter(|dir| !dir.is_empty()) {
        for probe in probes {
            if agents.iter().any(|agent| agent == probe) {
                continue;
            }
            let candidate = |extension: &str| Path::new(dir).join(format!("{probe}{extension}"));
            let found = if windows {
                WINDOWS_PATHEXT.iter().any(|ext| candidate(ext).is_file())
            } else {
                is_executable_file(&candidate(""))
            };
            if found {
                agents.push((*probe).to_string());
            }
        }
    }
    agents
}

/// unix 可执行判定：常规文件且任意执行位（owner/group/other）置位。
#[cfg(unix)]
fn is_executable_file(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path)
        .map(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

/// 非 unix 的回退：可执行位语义不可移植，退化为常规文件判定。
#[cfg(not(unix))]
fn is_executable_file(path: &Path) -> bool {
    path.is_file()
}

/// `addedPathSegments`：水合 PATH 段 − app 进程 PATH 段，保持水合序、去重。
pub fn added_path_segments(hydrated: &str, process_path: &str, platform: &str) -> Vec<String> {
    let separator = path_separator(platform);
    let existing: HashSet<&str> = process_path.split(separator).collect();
    let mut added: Vec<String> = Vec::new();
    for segment in hydrated.split(separator).filter(|s| !s.is_empty()) {
        if existing.contains(segment) || added.iter().any(|item| item == segment) {
            continue;
        }
        added.push(segment.to_string());
    }
    added
}

/// 由缓存快照 + app 进程 PATH 组装契约结果：水合成功用登录 PATH 探测
/// （`pathSource: 'shell_hydrate'`），失败回退 app 进程 PATH 探测
/// （`pathSource: 'sync_seed_only'`）。
pub fn assemble_refresh_agents(
    cached: &CachedHydration,
    process_path: &str,
    platform: &str,
) -> RefreshAgentsResult {
    let hydrated_ok = cached.failure == PathFailureReason::None;
    let effective_path = if hydrated_ok {
        cached
            .path
            .clone()
            .unwrap_or_else(|| process_path.to_string())
    } else {
        process_path.to_string()
    };
    RefreshAgentsResult {
        agents: detect_agents(&effective_path, AGENT_PROBES, platform),
        added_path_segments: cached.added_segments.clone(),
        shell_hydration_ok: hydrated_ok,
        path_source: if hydrated_ok {
            PathSource::ShellHydrate
        } else {
            PathSource::SyncSeedOnly
        },
        path_failure_reason: cached.failure,
    }
}

/// `preflight_refresh_agents`：登录 shell PATH 水合（进程内缓存一次）→
/// 与 app 进程 PATH 求差 → 探测 agent CLI。水合 shell 取 `$SHELL`，平台取
/// 编译目标 OS；`args` 契约参数收下忽略。
#[tauri::command]
#[specta::specta]
pub async fn preflight_refresh_agents(
    state: State<'_, AppState>,
    args: Option<PreflightRuntimeContext>,
) -> Result<RefreshAgentsResult, BridgeError> {
    let _ = args;
    let platform = std::env::consts::OS;
    let process_path = std::env::var("PATH").unwrap_or_default();
    let cached = match state.path_hydration_cache.get() {
        Some(cached) => cached,
        None => {
            let shell = std::env::var("SHELL").unwrap_or_default();
            // 水合起子进程（至多 2s），必须离开 async runtime 线程。
            let hydration = run_blocking(move || {
                Ok::<_, BridgeError>(hydrate_login_path(
                    platform,
                    &shell,
                    LOGIN_PATH_HYDRATION_TIMEOUT,
                ))
            })
            .await?;
            let cached = CachedHydration::from_hydration(&hydration, &process_path, platform);
            state.path_hydration_cache.set(cached.clone());
            cached
        }
    };
    Ok(assemble_refresh_agents(&cached, &process_path, platform))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    struct TestDir {
        path: std::path::PathBuf,
    }

    impl TestDir {
        fn new(name: &str) -> Self {
            let path = std::env::temp_dir().join(format!(
                "ade-bridge-preflight-{name}-{}",
                std::process::id()
            ));
            let _ = std::fs::remove_dir_all(&path);
            std::fs::create_dir_all(&path).expect("create test dir");
            Self { path }
        }

        /// 写入一个可执行脚本（unix 加执行位），返回其路径。
        fn write_executable(&self, name: &str, body: &str) -> std::path::PathBuf {
            let path = self.path.join(name);
            std::fs::write(&path, body).expect("write script");
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))
                    .expect("chmod +x");
            }
            path
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }

    // ── 水合解析 ─────────────────────────────────────────────

    #[test]
    fn parse_takes_last_non_empty_line_and_requires_separator() {
        assert_eq!(
            parse_login_path_output("banner line\n/usr/local/bin:/usr/bin\n", ':').as_deref(),
            Some("/usr/local/bin:/usr/bin")
        );
        // 尾行无分隔符 → 无效（调用方归 empty_path）。
        assert_eq!(
            parse_login_path_output("/a:/b\nshell banner text\n", ':'),
            None
        );
        assert_eq!(parse_login_path_output("   \n\n", ':'), None);
        assert_eq!(parse_login_path_output("", ':'), None);
        // windows 分隔符。
        assert_eq!(
            parse_login_path_output("C:\\a;C:\\b", ';').as_deref(),
            Some("C:\\a;C:\\b")
        );
    }

    #[cfg(unix)]
    #[test]
    fn hydration_parses_last_line_of_a_login_shell() {
        let dir = TestDir::new("hydrate-ok");
        let shell = dir.write_executable(
            "fake-shell",
            "#!/bin/sh\necho banner line\necho /opt/tools/bin:/usr/bin\n",
        );
        let hydration =
            hydrate_login_path("macos", shell.to_str().unwrap(), Duration::from_secs(10));
        assert_eq!(hydration.path.as_deref(), Some("/opt/tools/bin:/usr/bin"));
        assert_eq!(hydration.failure, PathFailureReason::None);
    }

    #[cfg(unix)]
    #[test]
    fn hydration_classifies_timeout() {
        let dir = TestDir::new("hydrate-timeout");
        // 假 shell 忽略 `-l -c 'echo $PATH'`，原地 sleep——1ms 级超时即归 timeout。
        let shell = dir.write_executable("slow-shell", "#!/bin/sh\nsleep 5\n");
        let hydration =
            hydrate_login_path("macos", shell.to_str().unwrap(), Duration::from_millis(50));
        assert_eq!(hydration.path, None);
        assert_eq!(hydration.failure, PathFailureReason::Timeout);
    }

    #[test]
    fn hydration_classifies_spawn_error_for_a_missing_shell() {
        let hydration = hydrate_login_path(
            "macos",
            "/nonexistent/ade-probe-shell",
            Duration::from_millis(500),
        );
        assert_eq!(hydration.path, None);
        assert_eq!(hydration.failure, PathFailureReason::SpawnError);
    }

    #[test]
    fn hydration_short_circuits_windows_to_no_shell() {
        let hydration = hydrate_login_path("windows", "/bin/zsh", Duration::from_secs(1));
        assert_eq!(hydration.path, None);
        assert_eq!(hydration.failure, PathFailureReason::NoShell);
    }

    // ── 探测 ─────────────────────────────────────────────────

    #[cfg(unix)]
    #[test]
    fn detect_agents_hits_only_executable_probes() {
        let dir = TestDir::new("detect-exec");
        let bin = dir.path.join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::write(bin.join("claude"), "#!/bin/sh\n").unwrap();
        std::fs::write(bin.join("codex"), "#!/bin/sh\n").unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(bin.join("claude"), std::fs::Permissions::from_mode(0o755))
            .unwrap();

        let path_var = format!("{}:/usr/local/bin", bin.display());
        assert_eq!(
            detect_agents(&path_var, AGENT_PROBES, "macos"),
            vec!["claude".to_string()]
        );
    }

    #[cfg(unix)]
    #[test]
    fn detect_agents_prefers_the_first_dir_and_dedupes() {
        let dir = TestDir::new("detect-order");
        let first = dir.path.join("first");
        let second = dir.path.join("second");
        std::fs::create_dir_all(&first).unwrap();
        std::fs::create_dir_all(&second).unwrap();
        use std::os::unix::fs::PermissionsExt;
        for sub in [&first, &second] {
            std::fs::write(sub.join("claude"), "").unwrap();
            std::fs::set_permissions(sub.join("claude"), std::fs::Permissions::from_mode(0o755))
                .unwrap();
        }
        let path_var = format!("{}:{}", first.display(), second.display());
        assert_eq!(
            detect_agents(&path_var, &["claude", "codex"], "linux"),
            vec!["claude".to_string()]
        );
    }

    #[test]
    fn detect_agents_windows_matches_pathext_candidates() {
        // platform 参数注入：unix 主机上即可覆盖 windows 分支（仅判 is_file）。
        let dir = TestDir::new("detect-windows");
        std::fs::write(dir.path.join("claude.exe"), "MZ").unwrap();
        std::fs::write(dir.path.join("codex.txt"), "").unwrap();
        let path_var = dir.path.to_str().unwrap().to_string();
        assert_eq!(
            detect_agents(&path_var, AGENT_PROBES, "windows"),
            vec!["claude".to_string()]
        );
    }

    #[test]
    fn detect_agents_skips_empty_segments_and_misses_unknown_dirs() {
        assert!(detect_agents(":", AGENT_PROBES, "macos").is_empty());
        assert!(detect_agents("Z:\\nonexistent-ade-probe", AGENT_PROBES, "windows").is_empty());
    }

    // ── addedPathSegments ────────────────────────────────────

    #[test]
    fn added_segments_keep_hydration_order_minus_process_path() {
        let added = added_path_segments(
            "/opt/cli/bin:/usr/local/bin:/opt/cli/bin:/usr/bin",
            "/usr/bin:/bin",
            "macos",
        );
        assert_eq!(
            added,
            vec!["/opt/cli/bin".to_string(), "/usr/local/bin".to_string()]
        );
    }

    // ── 缓存 ─────────────────────────────────────────────────

    #[test]
    fn hydration_cache_stores_the_latest_snapshot() {
        let cache = PathHydrationCache::default();
        assert!(cache.get().is_none());
        cache.set(CachedHydration {
            path: Some("/a:/b".to_string()),
            added_segments: vec!["/a".to_string()],
            failure: PathFailureReason::None,
        });
        let cached = cache.get().expect("cache hit");
        assert_eq!(cached.path.as_deref(), Some("/a:/b"));
        assert_eq!(cached.added_segments, vec!["/a".to_string()]);
    }

    #[test]
    fn cached_hydration_derives_added_segments_from_the_hydrated_path() {
        let hydration = PathHydration {
            path: Some("/opt/cli/bin:/usr/bin".to_string()),
            failure: PathFailureReason::None,
        };
        let cached = CachedHydration::from_hydration(&hydration, "/usr/bin", "macos");
        assert_eq!(cached.added_segments, vec!["/opt/cli/bin".to_string()]);

        let failed = CachedHydration::from_hydration(
            &PathHydration {
                path: None,
                failure: PathFailureReason::NoShell,
            },
            "/usr/bin",
            "windows",
        );
        assert!(failed.added_segments.is_empty());
        assert_eq!(failed.failure, PathFailureReason::NoShell);
    }

    // ── 命令级 serde 形状 ────────────────────────────────────

    #[test]
    fn refresh_result_serializes_the_contract_shape_on_success() {
        let cached = CachedHydration {
            path: Some("/opt/cli/bin:/usr/bin".to_string()),
            added_segments: vec!["/opt/cli/bin".to_string()],
            failure: PathFailureReason::None,
        };
        let value =
            serde_json::to_value(assemble_refresh_agents(&cached, "/usr/bin", "macos")).unwrap();
        assert_eq!(value["agents"], json!([]));
        assert_eq!(value["addedPathSegments"], json!(["/opt/cli/bin"]));
        assert_eq!(value["shellHydrationOk"], json!(true));
        assert_eq!(value["pathSource"], "shell_hydrate");
        assert_eq!(value["pathFailureReason"], "none");
    }

    #[test]
    fn refresh_result_falls_back_to_process_path_and_classifies_failure() {
        let cached = CachedHydration {
            path: None,
            added_segments: vec![],
            failure: PathFailureReason::Timeout,
        };
        // 回退路径取探测必不命中的目录，保证断言确定。
        let value = serde_json::to_value(assemble_refresh_agents(
            &cached,
            "/nonexistent-ade-probe",
            "macos",
        ))
        .unwrap();
        assert_eq!(value["agents"], json!([]));
        assert_eq!(value["addedPathSegments"], json!([]));
        assert_eq!(value["shellHydrationOk"], json!(false));
        assert_eq!(value["pathSource"], "sync_seed_only");
        assert_eq!(value["pathFailureReason"], "timeout");
    }
}
