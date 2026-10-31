//! Rebase-onto-base 及其回滚（`rebase --abort` / `merge --abort`）。
//!
//! `git_rebase_from_base` 此前**没有后端命令**（`real/git.ts` 不映射 `rebaseFromBase` →
//! `withMethodFallback` 拒），且 rebase 可能把仓库留在半完成态却没有可中止的 `abortRebase`/
//! `abortMerge`（同样是死映射）。本模块一次补齐三者的落地，保证"rebase 冲突不会被锁死"。
//!
//! rebase / abort 都是**本地**操作（不触网），故走 `run_git_in`（无 `GIT_TERMINAL_PROMPT`
//! 需求）；`base_ref` 归一 + 注入守卫照 `remote_ops` 对 remote 名的处理。

use std::time::Duration;

use ade_core::errors::CoreError;

use crate::runner::{run_git_in, CancelToken};

/// 大仓 rebase 可较久；给 3 分钟。abort 走同一超时（本地、通常秒级）。
const REBASE_TIMEOUT: Duration = Duration::from_secs(180);

/// 归一并校验 rebase base ref：拒绝空/前导 `-`/含换行（option 注入面），并剥掉
/// `refs/remotes/`、`remotes/` 前缀（与 TS 侧 `normalizeBaseRef` 对齐）。返回的短名
/// 之后作为**单个 argv** 传给 `git rebase`，绝不经 shell。
fn normalize_base_ref(base_ref: &str) -> Result<String, CoreError> {
    let trimmed = base_ref.trim();
    if trimmed.is_empty() || trimmed.starts_with('-') || trimmed.chars().any(|ch| ch == '\n' || ch == '\r') {
        return Err(CoreError::InvalidInput(format!("invalid rebase base ref: {base_ref:?}")));
    }
    let stripped = trimmed
        .strip_prefix("refs/remotes/")
        .or_else(|| trimmed.strip_prefix("remotes/"))
        .unwrap_or(trimmed);
    if stripped.is_empty() || stripped.starts_with('-') {
        return Err(CoreError::InvalidInput(format!("invalid rebase base ref: {base_ref:?}")));
    }
    Ok(stripped.to_string())
}

fn run_local(worktree_path: &str, args: &[&str], cancel: Option<&CancelToken>) -> Result<(), CoreError> {
    let output = run_git_in(worktree_path, args, REBASE_TIMEOUT, cancel)?;
    if output.status.success() {
        return Ok(());
    }
    // 冲突/无法应用等：stderr 原样上抛（可操作信息），**不做文本匹配分类**（§3.8）。
    // rebase 冲突会非零退出并把仓库留在 rebase 态，由 abort_rebase 回滚。
    Err(CoreError::GitCommandFailed {
        command: format!("git {}", args.join(" ")),
        stderr: String::from_utf8_lossy(&output.stderr).trim().to_string(),
        exit_code: output.status.code(),
    })
}

/// 把当前分支变基到 `base_ref`。base 若为 remote-tracking（`origin/main`）应先由调用方
/// （Update/fetch 流）刷新；本函数只做本地 rebase。冲突时非零退出、留在 rebase 态。
pub fn rebase_from_base(
    worktree_path: &str,
    base_ref: &str,
    cancel: Option<&CancelToken>,
) -> Result<(), CoreError> {
    let base = normalize_base_ref(base_ref)?;
    run_local(worktree_path, &["rebase", base.as_str()], cancel)
}

/// `git rebase --abort`：回到 rebase 前状态（冲突留态的出口）。
pub fn abort_rebase(worktree_path: &str, cancel: Option<&CancelToken>) -> Result<(), CoreError> {
    run_local(worktree_path, &["rebase", "--abort"], cancel)
}

/// `git merge --abort`：回到 merge 前状态。
pub fn abort_merge(worktree_path: &str, cancel: Option<&CancelToken>) -> Result<(), CoreError> {
    run_local(worktree_path, &["merge", "--abort"], cancel)
}
