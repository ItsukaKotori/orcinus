//! 原地检出分支（spec §13.5）。
//!
//! 与「跳到已检出该分支的 workspace」（增量 1）是两条不同路径：本模块真的改
//! 当前 worktree 的 HEAD。因此有两个 git 层面的硬约束必须先处理：
//!   F1 分支已被**别的 worktree** 检出时 `git checkout` 直接 fatal —— 必须先复核，
//!      并把拒绝作为**结构化软错误**回传（不是 Err：前端要按 kind 出不同文案与跳转）。
//!   F2 `git checkout origin/main` 进的是 detached HEAD 而非建本地跟踪分支 ——
//!      远程目标必须显式 `checkout --track -b <短名> <远程 ref>`。
//! 两条都在本文件的测试里用真 git 仓钉住。

use ade_core::errors::CoreError;
use serde::Deserialize;
use serde::Serialize;
use std::time::Duration;

use crate::command::run;
use crate::runner::run_git_in;
use crate::{are_worktree_paths_equal, GitWorktreeEntry};

/// 本地分支写操作（-d/-D、-m）用短超时，对齐 worktree_remove 的注册级超时量级。
const BRANCH_MUTATE_TIMEOUT: Duration = Duration::from_secs(30);

const F1_MESSAGE: &str = "branch is already checked out in another workspace";

/// 调用方能表达的目标。**只有 `local` / `remote` 两种** —— tag / commit 的
/// detached checkout 本期不提供（spec §13.5）：ade 的 workspace 模型对 detached HEAD
/// 没有好位置放，`Worktree.branch` 会被刷成 null、头部退化成 DetachedHeadBadge。
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct GitCheckoutTarget {
    /// `"local"` | `"remote"`。字符串而非 Rust enum：前端从 `GitRefEntry.category`
    /// 直接映射，少一层转换；非法值在 [`resolve`] 里统一报 `InvalidTarget`，
    /// 不会变成反序列化失败。
    pub kind: String,
    /// 短名：本地为 `feature`，远程为 `origin/feature`。
    pub name: String,
}

/// 拒绝原因。经 [`GitCheckoutOutcome::status`] 以 `Ok(...)` 透出，**不走**
/// `BridgeError` —— 后者的 `Serialize` 只留 `{ message }`，类型信息全丢。
///
/// 前端按 `status` 自己走 i18n 出文案（spec §3.4 / §13.7）—— 本模块**不**发
/// 用户可读文案，否则会在 en/es/fr/ja/ko 下弹出中文。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "kebab-case")]
pub enum GitCheckoutRefusal {
    /// F1：目标分支已被另一个 worktree 检出。
    CheckedOutElsewhere,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "kebab-case")]
pub enum GitCheckoutStatus {
    Switched,
    /// 目标就是当前分支：不执行任何写操作，直接回。
    AlreadyOnBranch,
    Refused(GitCheckoutRefusal),
    /// 目标解析不到真实 ref，或 `kind` 非法。
    InvalidTarget,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct GitCheckoutOutcome {
    pub status: GitCheckoutStatus,
    /// 成功 / 幂等时是分支短名；`CheckedOutElsewhere` 时是被占用的那个短名。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    /// `CheckedOutElsewhere` 时是占用方 worktree 路径 —— 前端据此给「跳到那个
    /// workspace」的动作。与 `branch` 分两个字段：复用同一字段会让 TS 侧拿到
    /// `string | null` 而分不清语义。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub worktree_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

/// 规范化后的目标：一个本地分支短名 + 可选的「从哪个远程 ref 建并跟踪」。
struct Resolved {
    local_branch: String,
    track: Option<String>,
}

fn invalid_target() -> GitCheckoutOutcome {
    GitCheckoutOutcome {
        status: GitCheckoutStatus::InvalidTarget,
        branch: None,
        worktree_path: None,
        message: Some("branch is not a checkoutable target".to_string()),
    }
}

/// `git rev-parse --verify --quiet <reference>` 是否解析出非空 oid。
fn ref_exists(worktree_path: &str, reference: &str) -> bool {
    matches!(
        crate::command::run_text(worktree_path, &["rev-parse", "--verify", "--quiet", reference]),
        Ok(out) if !out.trim().is_empty()
    )
}

/// 把调用方给的 `(kind, name)` 折成一个**真实存在**的本地分支目标。
///
/// 关键：绝不把外部字符串当 revision 直接丢给 `git checkout`。`ref_exists` 一律
/// 在 `refs/heads/` 或 `refs/remotes/` 前缀之下做全量校验，所以 `main..feature`、
/// `HEAD@{0}`、`--help` 这类都解析失败 → `InvalidTarget`。远程目标必须剥出短名
/// 并显式 `--track`，否则按 F2 会进 detached HEAD。
fn resolve(worktree_path: &str, target: &GitCheckoutTarget) -> Option<Resolved> {
    let name = target.name.trim();
    if name.is_empty() || name.starts_with('-') {
        return None;
    }
    match target.kind.as_str() {
        "local" => {
            let full = format!("refs/heads/{name}");
            ref_exists(worktree_path, &full).then(|| Resolved {
                local_branch: name.to_string(),
                track: None,
            })
        }
        "remote" => {
            // name 形如 `origin/feature`，也形如 `origin/feat/login`。
            // 按**第一个**斜杠切分：git 的 remote 名实际上不带 `/`，而分支名常带，
            // 所以首段是 remote、其余全部是分支短名。
            let (remote, short) = name.split_once('/')?;
            // 短名**可以**含 `/`：`origin/feat/login`、`origin/release/1.2` 是最常见的
            // 分支命名，拒掉它们会让一大类远程分支无法检出（而本地分支没这限制，
            // 行为不对称）。按第一个斜杠切分对「remote 名含 /」与「分支名含 /」
            // 两种解读都拼出同一个 ref 路径、派出同一个本地短名，所以不需要排斥。
            if remote.is_empty() || short.is_empty() {
                return None;
            }
            let full = format!("refs/remotes/{remote}/{short}");
            if !ref_exists(worktree_path, &full) {
                return None;
            }
            // 本地已有同名分支 → 直接检出它，不再 `-b`（否则 git 报 already exists）。
            if ref_exists(worktree_path, &format!("refs/heads/{short}")) {
                return Some(Resolved { local_branch: short.to_string(), track: None });
            }
            Some(Resolved { local_branch: short.to_string(), track: Some(full) })
        }
        _ => None,
    }
}

/// 找出把 `branch` 检出在**其它** worktree 的那一个（F1）。
///
/// `GitWorktreeEntry.branch` 是完整 ref（`porcelain.rs` 原样存 `git worktree list`
/// 的 `branch refs/heads/x`），所以要比完整形态 —— 与增量 1 终审抓到的
/// `Worktree.branch` 同一套事实。
fn occupant(worktrees: &[GitWorktreeEntry], here: &str, branch: &str) -> Option<String> {
    let full = format!("refs/heads/{branch}");
    worktrees
        .iter()
        // 必须走 `are_worktree_paths_equal`，不能裸比字符串：`GitWorktreeEntry.path`
        // 是 git 存的 realpath，而 macOS 上 tempdir 是 `/var/...`、`git worktree list`
        // 报 `/private/var/...`，裸比永远不等 —— 自排除会变成死代码，
        // 一旦 `already_checked_out` 因 git 超时而回落 false，就会把当前 worktree
        // 自己判成占用方，回一条指向自己的拒绝。
        .find(|w| w.branch.as_deref() == Some(full.as_str()) && !are_worktree_paths_equal(&w.path, here))
        .map(|w| w.path.clone())
}

fn already_checked_out(worktree_path: &str, branch: &str) -> bool {
    matches!(
        crate::command::run_text(worktree_path, &["branch", "--show-current"]),
        Ok(current) if current.trim() == branch
    )
}

/// 在当前 worktree 原地检出 `target`。
///
/// `Err(CoreError)` 只留给**环境级失败**：git 不可用、路径不允许、`worktree_list`
/// 失败、以及我们无法分类的 git 非零退出（最典型是 F3「checkout 会覆盖本地改动」）。
/// 业务级拒绝（F1 / 非法目标 / 已在目标分支）一律 `Ok(GitCheckoutOutcome)`。
///
/// F3 不做成软错误的理由：把它从其他失败里分出来只能匹配 git 的 stderr 文本，
/// 而 spec §3.8 点名批评的正是这个做法。F3 不需要分支判定 —— 把 git 自己那句
/// 人话显示出来就够，前端（2B）会先查 status 拦一道确认。
pub fn checkout(
    worktree_path: &str,
    target: &GitCheckoutTarget,
) -> Result<GitCheckoutOutcome, CoreError> {
    let Some(resolved) = resolve(worktree_path, target) else {
        return Ok(invalid_target());
    };

    if already_checked_out(worktree_path, &resolved.local_branch) {
        return Ok(GitCheckoutOutcome {
            status: GitCheckoutStatus::AlreadyOnBranch,
            branch: Some(resolved.local_branch),
            worktree_path: None,
            message: None,
        });
    }

    let worktrees = crate::worktree_list(worktree_path)?;
    if let Some(path) = occupant(&worktrees, worktree_path, &resolved.local_branch) {
        return Ok(GitCheckoutOutcome {
            status: GitCheckoutStatus::Refused(GitCheckoutRefusal::CheckedOutElsewhere),
            branch: Some(resolved.local_branch),
            worktree_path: Some(path),
            message: Some(F1_MESSAGE.to_string()),
        });
    }

    // 先算完整 argv 再统一执行。
    //
    // `--track` 看起来是多余的（start point 是远程跟踪 ref 时，
    // `branch.autoSetupMerge` 默认就会建跟踪 —— 实测拿掉它 F2 行为不变），
    // 但它改变的是**失败时的响度**：remote 未配置时，带 `--track` 会 loud-fatal
    // （`cannot set up tracking information`），不带则静默建出一个不跟踪的分支。
    // 守这个差异的是 `remote_target_without_configured_remote_is_an_error_not_silent_tracking_loss`，
    // **不是** F2 那条 —— 删掉 `--track` 时 F2 不会红，别拿它当守卫。
    let argv: Vec<&str> = match &resolved.track {
        Some(full) => vec![
            "checkout",
            "--track",
            "-b",
            resolved.local_branch.as_str(),
            full.as_str(),
        ],
        None => vec!["checkout", resolved.local_branch.as_str()],
    };
    // `command::run` 已经把非零退出折成 `CoreError::GitCommandFailed`，所以这里
    // 拿不到 Output，也不要再包一层退出码判断。
    run(worktree_path, &argv)?;

    Ok(GitCheckoutOutcome {
        status: GitCheckoutStatus::Switched,
        branch: Some(resolved.local_branch),
        worktree_path: None,
        message: None,
    })
}

/// 分支写操作（create / set-upstream / rename / delete）共用的**结构化拒绝**。
/// 经各命令 `Outcome.status` 的 `Refused(...)` 以 `Ok(...)` 透出，**不走 `BridgeError`**
/// （后者 `Serialize` 只留 `{ message }`，类型全丢，spec §8.1）。前端按变体走 i18n 出文案。
/// 用量：create→`NameTaken`；set-upstream→`UpstreamMissing`；rename/delete→`CheckedOut`（+rename `NameTaken`）；
/// delete `-d` 未合并→`NotMergeable`。与 spec §8.1 示意略差：`branch` 名由各 `Outcome.branch`/`oldName`/`newName` 承载而非变体字段。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "kebab-case")]
pub enum BranchOperationRefusal {
    /// 目标分支名已存在于 `refs/heads/*`。
    NameTaken,
    /// 分支正被某个 worktree 检出（携带其路径供前端跳转）。
    CheckedOut {
        #[serde(rename = "worktreePath")]
        worktree_path: String,
    },
    /// 期望的上游 ref 不在 `refs/remotes/*`。
    UpstreamMissing,
    /// `git branch -d` 因未完全合并而拒；携带未合入 HEAD 的提交数供二次确认。
    NotMergeable {
        #[serde(rename = "unmergedCount")]
        unmerged_count: u32,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "kebab-case")]
pub enum GitBranchCreateStatus {
    Created,
    Refused(BranchOperationRefusal),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct GitBranchCreateOutcome {
    pub status: GitBranchCreateStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct GitBranchCreateInput {
    /// 新分支短名（可含 `/`）。经 `resolve_create_branch_name` 校验。
    pub name: String,
    /// start-point：一个能解析成 commit 的真实 rev（生产调用方恒传 `GitRefEntry.oid`）。
    /// 不约束到特定命名空间，但必须是可解析对象（`ref_exists`）；非 commit 表达式会被 `git branch` 拒且无副作用。
    pub start_point: String,
}

/// 在 `start_point` 建一个新分支 ref，**不检出**（HEAD 保持不动，区别于 [`checkout`]）。
///
/// 业务拒绝（名已占用）以 `Ok(Refused(NameTaken))` 透出；环境级失败（git 不可用、
/// 非法名、start-point 解析不到）才是 `Err`。三条都**无副作用**：拒绝/失败不得留下半成品 ref。
pub fn create(
    worktree_path: &str,
    args: &GitBranchCreateInput,
) -> Result<GitBranchCreateOutcome, CoreError> {
    let name = args.name.trim();
    if name.is_empty() {
        return Err(CoreError::InvalidInput("branch name must not be empty".to_string()));
    }
    // 复用 worktree 创建的名字校验：`-` 前缀拒绝 + `git check-ref-format --branch`。
    // override 形态（无 prefix、name 作 override）→ 校验并原样保留（含 `/`）。
    let name = crate::branch::resolve_create_branch_name(worktree_path, Some(name), None, "")?;

    if ref_exists(worktree_path, &format!("refs/heads/{name}")) {
        return Ok(GitBranchCreateOutcome {
            status: GitBranchCreateStatus::Refused(BranchOperationRefusal::NameTaken),
            branch: Some(name),
            message: None,
        });
    }

    // start-point 必须是能解析成 commit 的真实 rev（oid 或 ref）；绝不当任意 revision
    // 只接受能解析成对象的 start-point（`ref_exists` = `rev-parse --verify --quiet`），
    // 而非任意不存在的 revision；且作为独立 argv 传递（`--` 后不会被当 option）。
    let start = args.start_point.trim();
    if start.is_empty() || !ref_exists(worktree_path, start) {
        return Err(CoreError::InvalidInput(format!("unknown start point: {start}")));
    }

    // 只建 ref，不切换工作区。
    run(worktree_path, &["branch", "--", name.as_str(), start])?;

    Ok(GitBranchCreateOutcome {
        status: GitBranchCreateStatus::Created,
        branch: Some(name),
        message: None,
    })
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "kebab-case")]
pub enum GitBranchSetUpstreamStatus {
    Set,
    Refused(BranchOperationRefusal),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct GitBranchSetUpstreamOutcome {
    pub status: GitBranchSetUpstreamStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub upstream: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct GitBranchSetUpstreamInput {
    /// 目标本地分支短名（必须存在于 `refs/heads/*`）。
    pub branch: String,
    /// 上游，形如 `origin/feature`（必须存在于 `refs/remotes/*`）。
    pub upstream: String,
}

/// 给一个已存在的本地分支设置 upstream：`git branch --set-upstream-to=<upstream> -- <branch>`。
///
/// upstream 不在 `refs/remotes/*` → `Ok(Refused(UpstreamMissing))`（前端子菜单可能过时）；
/// 本地分支不存在 → `Err`（环境级，调用方只会从真实本地分支发起）。**不改 HEAD**、不切分支。
/// 二者都不接受任意 revision：`branch`/`upstream` 各自经 `ref_exists` 前缀校验，
/// 且作为独立 argv 传递。
pub fn set_upstream(
    worktree_path: &str,
    args: &GitBranchSetUpstreamInput,
) -> Result<GitBranchSetUpstreamOutcome, CoreError> {
    let branch = args.branch.trim();
    if branch.is_empty() || branch.starts_with('-') {
        return Err(CoreError::InvalidInput("invalid branch name".to_string()));
    }
    if !ref_exists(worktree_path, &format!("refs/heads/{branch}")) {
        return Err(CoreError::InvalidInput(format!("unknown local branch: {branch}")));
    }
    let upstream = args.upstream.trim();
    if upstream.is_empty() || upstream.starts_with('-') {
        return Ok(GitBranchSetUpstreamOutcome {
            status: GitBranchSetUpstreamStatus::Refused(BranchOperationRefusal::UpstreamMissing),
            branch: Some(branch.to_string()),
            upstream: None,
            message: None,
        });
    }
    if !ref_exists(worktree_path, &format!("refs/remotes/{upstream}")) {
        return Ok(GitBranchSetUpstreamOutcome {
            status: GitBranchSetUpstreamStatus::Refused(BranchOperationRefusal::UpstreamMissing),
            branch: Some(branch.to_string()),
            upstream: Some(upstream.to_string()),
            message: None,
        });
    }
    let set_arg = format!("--set-upstream-to={upstream}");
    run(worktree_path, &["branch", &set_arg, "--", branch])?;
    Ok(GitBranchSetUpstreamOutcome {
        status: GitBranchSetUpstreamStatus::Set,
        branch: Some(branch.to_string()),
        upstream: Some(upstream.to_string()),
        message: None,
    })
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "kebab-case")]
pub enum GitBranchDeleteStatus {
    Deleted,
    Refused(BranchOperationRefusal),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct GitBranchDeleteOutcome {
    pub status: GitBranchDeleteStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct GitBranchDeleteInput {
    pub branch: String,
    /// true → `-D`（强删未合并）；false → `-d`（未合并则结构化拒绝 NotMergeable）。
    pub force: bool,
}

/// 任一 worktree（含当前）检出 `branch` 时返回其路径（删除占用方必拒）。
fn checked_out_worktree_path(worktree_path: &str, branch: &str) -> Result<Option<String>, CoreError> {
    let full = format!("refs/heads/{branch}");
    Ok(crate::worktree_list(worktree_path)?
        .into_iter()
        .find(|entry| entry.branch.as_deref() == Some(full.as_str()))
        .map(|entry| entry.path))
}

/// 未合入 HEAD 的提交数（`git rev-list --count HEAD..<branch>`）；解析失败回落 0。
fn unmerged_count(worktree_path: &str, branch: &str) -> u32 {
    let spec = format!("HEAD..{branch}");
    crate::command::run_text(worktree_path, &["rev-list", "--count", spec.as_str()])
        .ok()
        .and_then(|text| text.trim().parse::<u32>().ok())
        .unwrap_or(0)
}

/// 删除本地分支。非 force 时 `git branch -d`，未合并 → `Ok(Refused(NotMergeable{count}))`
/// （不删、无副作用）；被任一 worktree 检出 → `Ok(Refused(CheckedOut{worktreePath}))`（用
/// `worktree_list` 结构化复核，不靠 stderr 文本，避 §3.8）；force 时 `-D`。
/// 占用/未合并都是业务拒绝（Ok）；只有非法名/未知分支/git 环境级失败才 `Err`。
/// 绝不动 HEAD（删除的不是当前分支，isHead 已被前端挡）。
pub fn delete(
    worktree_path: &str,
    args: &GitBranchDeleteInput,
) -> Result<GitBranchDeleteOutcome, CoreError> {
    let branch = args.branch.trim();
    if branch.is_empty() {
        return Err(CoreError::InvalidInput("branch name must not be empty".to_string()));
    }
    if !ref_exists(worktree_path, &format!("refs/heads/{branch}")) {
        return Err(CoreError::InvalidInput(format!("unknown local branch: {branch}")));
    }
    if let Some(path) = checked_out_worktree_path(worktree_path, branch)? {
        return Ok(GitBranchDeleteOutcome {
            status: GitBranchDeleteStatus::Refused(BranchOperationRefusal::CheckedOut {
                worktree_path: path,
            }),
            branch: Some(branch.to_string()),
            message: None,
        });
    }
    let flag = if args.force { "-D" } else { "-d" };
    let output = run_git_in(worktree_path, &["branch", flag, "--", branch], BRANCH_MUTATE_TIMEOUT, None)?;
    if output.status.success() {
        return Ok(GitBranchDeleteOutcome {
            status: GitBranchDeleteStatus::Deleted,
            branch: Some(branch.to_string()),
            message: None,
        });
    }
    // force(-D) 失败不可能是“未合并”，归为环境级 Err；非 force 的失败已在上面排除占用 → 未合并。
    if args.force {
        return Err(crate::git_command_failed(
            &["branch", flag, "--", branch],
            &output,
        ));
    }
    Ok(GitBranchDeleteOutcome {
        status: GitBranchDeleteStatus::Refused(BranchOperationRefusal::NotMergeable {
            unmerged_count: unmerged_count(worktree_path, branch),
        }),
        branch: Some(branch.to_string()),
        message: None,
    })
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "kebab-case")]
pub enum GitBranchRenameStatus {
    Renamed,
    Refused(BranchOperationRefusal),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct GitBranchRenameOutcome {
    pub status: GitBranchRenameStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub old_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub new_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct GitBranchRenameInput {
    pub old_name: String,
    pub new_name: String,
}

/// 重命名本地分支：`git branch -m -- <old> <new>`。仅未检出分支（前端 advisory 置灰 +
/// 此处 `worktree_list` 复核）。新名过 `resolve_create_branch_name`；已占用 → NameTaken；
/// old 被任一 worktree 检出 → CheckedOut。**不动 HEAD**（未检出分支不可能是当前 HEAD）。
/// ade 不同步改写 workspace 元数据/config（决策 5：`git branch -m` 不迁自定义 config、未检出分支无 workspace）。
pub fn rename(
    worktree_path: &str,
    args: &GitBranchRenameInput,
) -> Result<GitBranchRenameOutcome, CoreError> {
    let old = args.old_name.trim();
    if old.is_empty() {
        return Err(CoreError::InvalidInput("source branch name must not be empty".to_string()));
    }
    if !ref_exists(worktree_path, &format!("refs/heads/{old}")) {
        return Err(CoreError::InvalidInput(format!("unknown local branch: {old}")));
    }
    let raw_new = args.new_name.trim();
    if raw_new.is_empty() {
        return Err(CoreError::InvalidInput("branch name must not be empty".to_string()));
    }
    let new = crate::branch::resolve_create_branch_name(worktree_path, Some(raw_new), None, "")?;

    if new == old {
        // 改名到同名：幂等无副作用（不跑 git）。
        return Ok(GitBranchRenameOutcome { status: GitBranchRenameStatus::Renamed, old_name: Some(old.to_string()), new_name: Some(new), message: None });
    }
    if ref_exists(worktree_path, &format!("refs/heads/{new}")) {
        return Ok(GitBranchRenameOutcome {
            status: GitBranchRenameStatus::Refused(BranchOperationRefusal::NameTaken),
            old_name: Some(old.to_string()),
            new_name: Some(new),
            message: None,
        });
    }
    if let Some(path) = checked_out_worktree_path(worktree_path, old)? {
        return Ok(GitBranchRenameOutcome {
            status: GitBranchRenameStatus::Refused(BranchOperationRefusal::CheckedOut { worktree_path: path }),
            old_name: Some(old.to_string()),
            new_name: Some(new),
            message: None,
        });
    }
    run(worktree_path, &["branch", "-m", "--", old, new.as_str()])?;
    Ok(GitBranchRenameOutcome {
        status: GitBranchRenameStatus::Renamed,
        old_name: Some(old.to_string()),
        new_name: Some(new),
        message: None,
    })
}
