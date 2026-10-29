//! 网络类 git 操作（`fetch` / `pull`）。
//!
//! 背景：`git_fetch` / `git_pull` 此前**根本不存在** —— `specta_export.rs` 注册的
//! `git_*` 命令里没有它们，`real/git.ts` 的映射也没有，但
//! `runtime-git-sync-client.ts` 的本地分支照旧调 `window.api.git.fetch/pull`，
//! 于是右侧栏的 pull / sync 按钮点了就是 unimplemented。本模块补上后端。
//!
//! 两条与本地只读查询不同的硬约束：
//!  1. **必须 `GIT_TERMINAL_PROMPT=0`**。私有 HTTPS 仓在没有可用凭证 helper 时，
//!     git 会去开 `/dev/tty` 要用户名密码；GUI 应用里那表现为**挂到超时**，
//!     而不是快速失败并给出可操作的错误。
//!  2. **超时比只读查询长**。大仓 fetch 超过 `command::GIT_READ_TIMEOUT` 是常态。

use std::time::Duration;

use ade_core::errors::CoreError;

use crate::runner::{run_git_in_with_env, CancelToken};

/// 网络操作超时。大仓 fetch/pull 走 120s 的读超时容易误杀，这里给 5 分钟。
pub const GIT_NETWORK_TIMEOUT: Duration = Duration::from_secs(300);

/// 传给所有网络子命令的环境：禁止凭证提示，让它快速失败。
pub const GIT_NETWORK_ENVS: &[(&str, &str)] = &[("GIT_TERMINAL_PROMPT", "0")];

/// 安全 remote 名：非空、不带前导 `-`、不含换行（会被拼进 argv，虽然 argv 不经 shell，
/// 但换行/前导 `-` 是 git 侧的常见注入面）。
pub fn is_safe_remote_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 100
        && !name.starts_with('-')
        && !name.chars().any(|ch| ch == '\n' || ch == '\r')
}

/// `git fetch` 的参数。`--prune` 让远端已删分支在本地的跟踪 ref 同步消失 ——
/// 分支树正是靠这些 ref 列出 Remote 组的，不 prune 会一直显示点不动的幽灵分支。
pub fn fetch_args(remote: Option<&str>) -> Vec<String> {
    let mut args = vec!["fetch".to_string(), "--prune".to_string()];
    if let Some(remote) = remote {
        args.push(remote.to_string());
    }
    args
}

/// `git pull` 的参数。**不加** `--ff-only` / `--rebase`：
/// 按 spec §12.2 的决定「不问 merge/rebase 偏好、按 git 默认」，让
/// `pull.ff` / `pull.rebase` 等用户配置生效。
pub fn pull_args() -> Vec<String> {
    vec!["pull".to_string()]
}

/// `git pull --ff-only` 的参数。与 `pull` 的唯一差别：分叉时**硬失败**而不自动
/// merge —— 对应 IDEA “Fast-forward” 语义（只允许快进）。
pub fn fast_forward_args() -> Vec<String> {
    vec!["pull".to_string(), "--ff-only".to_string()]
}

fn run_network(
    worktree_path: &str,
    args: &[String],
    cancel: Option<&CancelToken>,
) -> Result<(), CoreError> {
    let borrowed: Vec<&str> = args.iter().map(String::as_str).collect();
    let output = run_git_in_with_env(
        worktree_path,
        &borrowed,
        GIT_NETWORK_TIMEOUT,
        cancel,
        GIT_NETWORK_ENVS,
    )?;
    if output.status.success() {
        return Ok(());
    }
    // 网络失败的 stderr 本身就是可操作信息（权限、非快进、冲突文件列表），
    // 原样上抛；不做文本匹配分类 —— 那正是 spec §3.8 批评的做法。
    Err(CoreError::GitCommandFailed {
        command: format!("git {}", borrowed.join(" ")),
        stderr: String::from_utf8_lossy(&output.stderr).trim().to_string(),
        exit_code: output.status.code(),
    })
}

/// 从远端拉取 ref（默认所有已配置 remote 里分支所在的那个）。
pub fn fetch(
    worktree_path: &str,
    remote: Option<&str>,
    cancel: Option<&CancelToken>,
) -> Result<(), CoreError> {
    if let Some(remote) = remote.filter(|name| !name.is_empty()) {
        if !is_safe_remote_name(remote) {
            return Err(CoreError::InvalidInput(format!(
                "unsafe remote name: {remote}"
            )));
        }
        return run_network(worktree_path, &fetch_args(Some(remote)), cancel);
    }
    run_network(worktree_path, &fetch_args(None), cancel)
}

/// 拉取并合并当前分支的 upstream。
pub fn pull(worktree_path: &str, cancel: Option<&CancelToken>) -> Result<(), CoreError> {
    run_network(worktree_path, &pull_args(), cancel)
}

/// 快进当前分支到其 upstream；分叉则原样失败（不自动 merge）。
pub fn fast_forward(
    worktree_path: &str,
    cancel: Option<&CancelToken>,
) -> Result<(), CoreError> {
    run_network(worktree_path, &fast_forward_args(), cancel)
}
