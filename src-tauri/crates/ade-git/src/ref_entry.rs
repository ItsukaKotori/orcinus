//! `git for-each-ref` 的分支/标签条目模型（增量 1：IDEA 风格分支树的数据契约）。
//!
//! 与 `GitHistoryItemRef`（commit 图上的 decoration）刻意分开：那个类型服务
//! decoration 渲染，往它塞 `ahead/upstream/remote` 是语义污染（见 spec §6.2）。

use serde::Serialize;

/// ref 的大类。serde 用小写单词，TS 侧 `GitRefCategory` 逐字镜像。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "lowercase")]
pub enum GitRefCategory {
    Local,
    Remote,
    Tag,
}

/// 一条分支/远程分支/标签，带分支树要显示的信息密度。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct GitRefEntry {
    /// 短名：`main` / `origin/main` / `v1.2.0`。
    pub name: String,
    /// 完整 refname：`refs/heads/main`。
    pub full_name: String,
    pub category: GitRefCategory,
    /// 仅 `Remote` 有值（`origin`）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub remote: Option<String>,
    /// commit oid；annotated tag 已 peel 到 commit。
    pub oid: String,
    /// git 报告该 ref 就是当前 HEAD 分支。
    pub is_head: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub upstream: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ahead: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub behind: Option<u32>,
}

/// `git for-each-ref` 的 `%(upstream:track)` 载荷，形如 `[ahead 2, behind 1]`。
/// 空串（无 upstream 或已同步）→ `(None, None)`。未知形状一律降级为 `None`：
/// 计数是装饰，不该让整份 ref 列表失败。
pub fn parse_upstream_track(raw: &str) -> (Option<u32>, Option<u32>) {
    let inner = raw.trim().trim_start_matches('[').trim_end_matches(']');
    let mut ahead = None;
    let mut behind = None;
    for part in inner.split(',') {
        match part.split_whitespace().collect::<Vec<_>>().as_slice() {
            ["ahead", n] => ahead = n.parse().ok(),
            ["behind", n] => behind = n.parse().ok(),
            _ => {}
        }
    }
    (ahead, behind)
}

/// 从 `for-each-ref` 的一行六列构造条目。列序：
/// `refname / objectname / *objectname / HEAD / upstream:short / upstream:track`。
///
/// 后三列允许缺失（git 版本差异），缺失按空串处理 —— 少一列只是少一项装饰。
/// 落在 `refs/{heads,remotes,tags}` 之外的 refname 返回 `None`（调用方跳过）。
pub fn git_ref_entry(columns: [&str; 6]) -> Option<GitRefEntry> {
    let [full_name, oid, peeled, head, upstream, track] = columns;
    if full_name.is_empty() || oid.is_empty() {
        return None;
    }
    let (category, name, remote) = if let Some(branch) = full_name.strip_prefix("refs/heads/") {
        (GitRefCategory::Local, branch.to_string(), None)
    } else if let Some(remote_branch) = full_name.strip_prefix("refs/remotes/") {
        let remote = remote_branch.split_once('/').map(|(r, _)| r.to_string());
        (
            GitRefCategory::Remote,
            remote_branch.to_string(),
            remote,
        )
    } else if let Some(tag) = full_name.strip_prefix("refs/tags/") {
        (GitRefCategory::Tag, tag.to_string(), None)
    } else {
        return None;
    };
    // annotated tag 的 %(objectname) 是 tag object，%(*objectname) 才是 commit。
    let revision = if peeled.is_empty() { oid } else { peeled };
    let (ahead, behind) = parse_upstream_track(track);
    Some(GitRefEntry {
        name,
        full_name: full_name.to_string(),
        category,
        remote,
        oid: revision.to_string(),
        is_head: head == "*",
        upstream: (!upstream.is_empty()).then(|| upstream.to_string()),
        ahead,
        behind,
    })
}
