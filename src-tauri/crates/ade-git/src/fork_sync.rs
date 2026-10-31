//! Fork 同步（移植 `src/shared/git-fork-sync.ts` 的 `syncForkDefaultBranch`）。
//!
//! `git.syncFork` 此前无后端 → `real/git.ts` 不映射 → `withMethodFallback` 拒。TS 编排
//! 需要 `remote get-url`/`ls-remote`/`fetch`/`push` 等**不在 `git_read` 只读白名单**的子命令，
//! 无法复用只读执行器。故这里作为**专用命令**内部直接跑这些 git 步骤（不放宽 git_read、不引入
//! 泛化写执行器）。网络步骤（ls-remote/fetch/push）经 `run_git_in_with_env` + `GIT_NETWORK_ENVS`。
//!
//! 语义：fork 的 `origin` 默认分支落后于 `upstream` 且可快进时，把 upstream 的提交推给 origin。

use ade_core::errors::CoreError;
use serde::{Deserialize, Serialize};

use crate::remote_ops::{GIT_NETWORK_ENVS, GIT_NETWORK_TIMEOUT};
use crate::runner::run_git_in_with_env;

const DEFAULT_ORIGIN_REMOTE: &str = "origin";
const DEFAULT_UPSTREAM_REMOTE: &str = "upstream";
const DEFAULT_BRANCH_FALLBACKS: [&str; 2] = ["main", "master"];
const GITHUB_HOSTS: [&str; 2] = ["github.com", "ssh.github.com"];

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "kebab-case")]
pub enum ForkSyncStatus {
    UpToDate,
    Synced,
    Blocked,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "kebab-case")]
pub enum ForkSyncBlockedReason {
    MissingOrigin,
    MissingUpstream,
    UpstreamMismatch,
    MissingUpstreamDefaultBranch,
    MissingOriginBranch,
    Diverged,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct GitForkSyncResult {
    pub status: ForkSyncStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<ForkSyncBlockedReason>,
    pub origin_remote: String,
    pub upstream_remote: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub branch_name: Option<String>,
    pub ahead: u32,
    pub behind: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct GitForkSyncExpectedUpstream {
    pub owner: String,
    pub repo: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct GitForkSyncInput {
    #[serde(default)]
    pub expected_upstream: Option<GitForkSyncExpectedUpstream>,
}

/// 跑一条 git；成功返回 stdout(trim)，失败/不可达返回 None。`network=true` 附凭证禁用 env。
fn git_text(worktree_path: &str, args: &[&str], network: bool) -> Option<String> {
    let envs: &[(&str, &str)] = if network { GIT_NETWORK_ENVS } else { &[] };
    let output = run_git_in_with_env(worktree_path, args, GIT_NETWORK_TIMEOUT, None, envs).ok()?;
    if output.status.success() {
        Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        None
    }
}

fn remote_exists(worktree_path: &str, remote: &str) -> bool {
    git_text(worktree_path, &["remote"], false)
        .map(|stdout| stdout.lines().any(|line| line.trim() == remote))
        .unwrap_or(false)
}

fn is_github_host(host: &str) -> bool {
    GITHUB_HOSTS.contains(&host.to_lowercase().as_str())
}

/// 把 remote URL 解析成 GitHub `owner/repo`（小写）；非 GitHub / 本地路径 → None。
/// 显式覆盖三形态：`github:owner/repo` 简写、`scheme://[user@]host/path`、scp-like `[user@]host:path`。
/// TS 参考 `git-fork-sync.ts::parseGitHubRemotePath` 同样支持 scp 形态，其测试夹具就是 SSH URL。
fn parse_github_remote_path(url: &str) -> Option<String> {
    let trimmed = url.trim();
    let trimmed = trimmed.strip_prefix("git+").unwrap_or(trimmed);
    let path: &str = if let Some(rest) = trimmed.strip_prefix("github:") {
        rest
    } else if let Some((_, after_proto)) = trimmed.split_once("://") {
        let host_and_rest = after_proto.split('/').next().unwrap_or("");
        let host = host_and_rest.rsplit('@').next().unwrap_or("");
        if !is_github_host(host) {
            return None;
        }
        &after_proto[host_and_rest.len()..]
    } else {
        // scp-like `[user@]host:path`（无 `://` 且含 `:`）。本地绝对路径无冒号 → split_once 得 None。
        let (host_and_user, rest) = trimmed.split_once(':')?;
        let host = host_and_user.rsplit('@').next().unwrap_or("");
        if !is_github_host(host) {
            return None;
        }
        rest
    };
    let cleaned = path.trim_matches('/').trim_end_matches(".git").trim_end_matches('/');
    let parts: Vec<&str> = cleaned.split('/').filter(|p| !p.is_empty()).collect();
    if parts.len() != 2 {
        return None;
    }
    Some(format!("{}/{}", parts[0].to_lowercase(), parts[1].to_lowercase()))
}

fn remote_matches_expected_upstream(worktree_path: &str, remote: &str, expected: &GitForkSyncExpectedUpstream) -> bool {
    let owner = expected.owner.trim().to_lowercase();
    let repo = expected.repo.trim().to_lowercase();
    if owner.is_empty() || repo.is_empty() {
        return false;
    }
    match git_text(worktree_path, &["remote", "get-url", remote], false) {
        Some(url) => parse_github_remote_path(&url).as_deref() == Some(&*format!("{owner}/{repo}")),
        None => false,
    }
}

fn resolve_commit(worktree_path: &str, reference: &str) -> Option<String> {
    let spec = format!("{reference}^{{commit}}");
    git_text(worktree_path, &["rev-parse", "--verify", spec.as_str()], false).filter(|s| !s.is_empty())
}

/// 从 `ls-remote --symref <remote> HEAD` 解析默认分支；不可达时兜底 common 名（离线/stale）。
fn resolve_remote_default_branch(worktree_path: &str, remote: &str) -> Option<String> {
    if let Some(stdout) = git_text(worktree_path, &["ls-remote", "--symref", remote, "HEAD"], true) {
        for line in stdout.lines() {
            // 形如 "ref: refs/heads/main\tHEAD"
            if let Some(rest) = line.trim().strip_prefix("ref: refs/heads/") {
                if let Some((branch, tail)) = rest.split_once('\t') {
                    let _ = tail; // "HEAD"
                    if !branch.is_empty() {
                        return Some(branch.to_string());
                    }
                }
            }
        }
    }
    DEFAULT_BRANCH_FALLBACKS
        .iter()
        .find(|branch| resolve_commit(worktree_path, &format!("refs/remotes/{remote}/{branch}")).is_some())
        .map(|branch| (*branch).to_string())
}

fn fetch_remote_branch(worktree_path: &str, remote: &str, branch: &str) -> bool {
    let refspec = format!("+refs/heads/{branch}:refs/remotes/{remote}/{branch}");
    git_text(worktree_path, &["fetch", "--no-tags", "--prune", remote, refspec.as_str()], true).is_some()
}

fn is_ancestor(worktree_path: &str, ancestor: &str, descendant: &str) -> bool {
    git_text(worktree_path, &["merge-base", "--is-ancestor", ancestor, descendant], false).is_some()
}

/// `git rev-list --left-right --count A...B` → (left, right)。失败视作 (0,0) 由上层判定。
fn ahead_behind(worktree_path: &str, origin_oid: &str, upstream_oid: &str) -> (u32, u32) {
    let spec = format!("{origin_oid}...{upstream_oid}");
    match git_text(worktree_path, &["rev-list", "--left-right", "--count", spec.as_str()], false) {
        Some(stdout) => {
            let mut fields = stdout.split_whitespace();
            let left = fields.next().and_then(|v| v.parse::<u32>().ok()).unwrap_or(0);
            let right = fields.next().and_then(|v| v.parse::<u32>().ok()).unwrap_or(0);
            (left, right)
        }
        None => (0, 0),
    }
}

fn blocked(origin: &str, upstream: &str, reason: ForkSyncBlockedReason, branch_name: Option<String>, ahead: u32, behind: u32) -> GitForkSyncResult {
    GitForkSyncResult {
        status: ForkSyncStatus::Blocked,
        reason: Some(reason),
        origin_remote: origin.to_string(),
        upstream_remote: upstream.to_string(),
        branch_name,
        ahead,
        behind,
    }
}

/// 同步 fork 默认分支：origin 落后且可快进时把 upstream 提交推给 origin。逐步骤对齐 TS 参考实现。
pub fn fork_sync(worktree_path: &str, args: &GitForkSyncInput) -> Result<GitForkSyncResult, CoreError> {
    let origin = DEFAULT_ORIGIN_REMOTE;
    let upstream = DEFAULT_UPSTREAM_REMOTE;

    if !remote_exists(worktree_path, origin) {
        return Ok(blocked(origin, upstream, ForkSyncBlockedReason::MissingOrigin, None, 0, 0));
    }
    if !remote_exists(worktree_path, upstream) {
        return Ok(blocked(origin, upstream, ForkSyncBlockedReason::MissingUpstream, None, 0, 0));
    }
    if let Some(expected) = &args.expected_upstream {
        if !remote_matches_expected_upstream(worktree_path, upstream, expected) {
            return Ok(blocked(origin, upstream, ForkSyncBlockedReason::UpstreamMismatch, None, 0, 0));
        }
    }

    let branch = match resolve_remote_default_branch(worktree_path, upstream) {
        Some(b) => b,
        None => return Ok(blocked(origin, upstream, ForkSyncBlockedReason::MissingUpstreamDefaultBranch, None, 0, 0)),
    };
    let origin_ref = format!("refs/remotes/{origin}/{branch}");
    let upstream_ref = format!("refs/remotes/{upstream}/{branch}");

    if !fetch_remote_branch(worktree_path, upstream, &branch) {
        return Ok(blocked(origin, upstream, ForkSyncBlockedReason::MissingUpstreamDefaultBranch, Some(branch), 0, 0));
    }
    if !fetch_remote_branch(worktree_path, origin, &branch) {
        return Ok(blocked(origin, upstream, ForkSyncBlockedReason::MissingOriginBranch, Some(branch), 0, 0));
    }
    let upstream_oid = match resolve_commit(worktree_path, &upstream_ref) {
        Some(oid) => oid,
        None => return Ok(blocked(origin, upstream, ForkSyncBlockedReason::MissingUpstreamDefaultBranch, Some(branch), 0, 0)),
    };
    let origin_oid = match resolve_commit(worktree_path, &origin_ref) {
        Some(oid) => oid,
        None => return Ok(blocked(origin, upstream, ForkSyncBlockedReason::MissingOriginBranch, Some(branch), 0, 0)),
    };

    let (ahead, behind) = ahead_behind(worktree_path, &origin_oid, &upstream_oid);
    if ahead > 0 || !is_ancestor(worktree_path, &origin_oid, &upstream_oid) {
        return Ok(blocked(origin, upstream, ForkSyncBlockedReason::Diverged, Some(branch), ahead, behind));
    }
    if behind == 0 {
        return Ok(GitForkSyncResult {
            status: ForkSyncStatus::UpToDate,
            reason: None,
            origin_remote: origin.to_string(),
            upstream_remote: upstream.to_string(),
            branch_name: Some(branch),
            ahead,
            behind,
        });
    }
    // origin 落后且可快进：把 upstream 的提交推给 origin 默认分支。push 失败原样上抛 stderr（不虚构）。
    let pushspec = format!("{upstream_oid}:refs/heads/{branch}");
    let push_args = ["push", origin, pushspec.as_str()];
    let push_output =
        run_git_in_with_env(worktree_path, &push_args, GIT_NETWORK_TIMEOUT, None, GIT_NETWORK_ENVS)?;
    if !push_output.status.success() {
        return Err(CoreError::GitCommandFailed {
            command: format!("git {}", push_args.join(" ")),
            stderr: String::from_utf8_lossy(&push_output.stderr).trim().to_string(),
            exit_code: push_output.status.code(),
        });
    }
    fetch_remote_branch(worktree_path, origin, &branch);
    Ok(GitForkSyncResult {
        status: ForkSyncStatus::Synced,
        reason: None,
        origin_remote: origin.to_string(),
        upstream_remote: upstream.to_string(),
        branch_name: Some(branch),
        ahead,
        behind,
    })
}

#[cfg(test)]
mod tests {
    use super::parse_github_remote_path;

    // 六类 URL 边界；尤其锁 scp-like SSH（旧实现误判 None → fork 主流配置恒 upstream-mismatch）。
    #[test]
    fn parses_all_github_url_shapes() {
        for url in [
            "https://github.com/Acme/Widgets.git",
            "https://github.com/acme/widgets",
            "git+https://github.com/acme/widgets.git",
            "ssh://git@github.com/acme/widgets.git",
            "git@github.com:acme/widgets.git", // scp-like —— 修复前为 None
            "github:acme/widgets",
        ] {
            assert_eq!(parse_github_remote_path(url).as_deref(), Some("acme/widgets"), "url={url}");
        }
    }

    #[test]
    fn rejects_non_github_and_local_paths() {
        for url in [
            "/Users/dev/repos/widgets",
            "git://example.com/acme/widgets.git",
            "https://gitlab.com/acme/widgets.git",
            "https://github.com/acme/widgets/extra",
        ] {
            assert_eq!(parse_github_remote_path(url), None, "url={url}");
        }
    }
}
