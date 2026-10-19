# Phase 4 · Git Log 插件 增量 2b（VcsLog 真查询：全分支 / 过滤 / 分页）设计规格

- 日期：2026-10-18
- 状态：brainstorming 输出（待用户 review 后进入 writing-plans）
- 基线：`main`（含增量 1+2a，`86540da3`）；工作树 `phase4b-git-log-query`（`/Users/itsuka/CodeSpace/ade-git-log-2b`，自 `86540da3`）。
- 上游：`docs/superpowers/specs/2026-10-18-git-log-plugin-design.md`（增量 1）、`.../2026-10-18-git-log-plugin-increment2-design.md`（增量 2a，git:read/git.log/git.commitFiles/HostRepository）、`docs/phase4-git-log-plugin-record.md`。

## 1. 背景与目标
增量 2a 让 Git Log 显示当前 worktree 的有界 HEAD 历史。2b 升级为**真·VcsLog 查询**：全分支图、按 author/date/path/message 过滤、offset 分页 + 无限滚动，全部**在后端 `ade_git` 执行**（翻成 `git log` 参数），保持 2a 的隔离约束（只读、`active-worktree` 限域、路径不下发、`ade-plugins` 不依赖 `ade-git`）。

已确认取向：① offset 分页 + 加载更多；② 默认当前分支（HEAD 可达），可切「全部分支」`--all`；③ author/date/path/grep 过滤在后端 git 参数执行。

## 2. 范围
### 2.1 实现面
| 面 | 载体 | 说明 |
|---|---|---|
| 后端查询 | `src-tauri/crates/ade-git/src/history.rs` | 新 `GitHistoryQuery { refs, author?, since?, until?, grep?, path?, skip?, limit? }` + `history_with(path, query)`；现 `history(path,limit,base_ref)` 变薄封装（向后兼容，ade-bridge 现有调用不破）。范围 `<head>`/`--all`/校验后 rev；过滤 `--author` `--since` `--until` `--grep` `-- <path>`；分页 `--skip=N -n{limit+1}`+`hasMore`。**current/remote/merge_base/ahead/behind 仍按 HEAD 算**（与范围无关，保 boundary）。`-z --topo-order --decorate=full` 与 camelCase serde 不变。 |
| ref 列表 | `ade-git/src/history.rs`（或新 `refs.rs`） | `list_refs(path) -> Vec<GitHistoryItemRef>`：`git for-each-ref refs/heads refs/remotes refs/tags`，复用现有 ref 分类/命名逻辑。供分支选择器。 |
| host 方法 | `ade-plugins/host/{api.rs,methods.rs,services.rs}` + `ade-bridge/plugins_service.rs` | 扩 `git.log` params 为 query；新增 `git.refs`（无参→`GitHistoryItemRef[]`）。均 `capability:'git:read'`、`panel:true`、`mutation:false`、`active-worktree`。`git.log` 走 `history_with`；`git.refs` 走 `list_refs`。 |
| TS 能力面 | `src/shared/plugins/plugin-capabilities.ts`（无新 kind）、`plugin-host-api.ts`（扩 git.log params + 加 git.refs）、`plugins/git-log/src/domain/types.ts`（`gitHistoryResultSchema`/`gitRefsSchema` + query 类型） | `git.log` result 形状不变（items 分页 + hasMore）。 |
| 错误码 | `src/shared/plugins/plugin-panel-bridge.ts` | `PluginPanelActionErrorCode` 并入 `no-active-worktree/not-a-git-repo/unknown-oid/unavailable`，使 `git.*` 具体码不再折叠成 `action_failed`；UI 分类文案（至少 unknown-oid）。 |
| 插件 UI | `plugins/git-log/src/ui/{GitLogView,Toolbar,GraphList}.tsx`、`domain/host-repository.ts` | 分支选择器（HEAD/全部/具体 ref，来自 `git.refs`）；Author/Date/Path/Grep chips → 带 query 的 `git.log`；`loadMore()` skip+=limit 无限滚动累加；分页失败保留已加载。每页 limit 默认 100（<768KiB 预算）。 |
| 探测复用 | `ade-bridge/plugins_service.rs` | `is_inside_work_tree` 结果按 worktree path 轻量缓存（`OnceCell`/`LruCache` 或每次仅一次），避免 git.log+git.refs+commitFiles 各 spawn 一次。 |

### 2.2 不做（明确排除）
- 不引入写操作（checkout/merge/rebase/cherry-pick/reset/stash）、不接 blame/图形 dashboard（增量 3）。
- 不改 `git.log`/`git.refs` 的只读性质或 `active-worktree` 限域；不新增第二个 capability（沿用 `git:read`）。
- 不改现有 Source Control UI / 现有 git bridge 命令语义（`git.log` 扩参是新增可选字段，旧调用不破）。
- manifest 命名：2a 已 `ade-plugin.json`＝updater 规范，无需再动。
- 不做服务端聚合分页/游标重构；offset 分页足够 IDEA VcsLog「加载更多」模型。

## 3. host 方法契约
- `git.log` params（全部可选，缺省＝增量 2a 行为：HEAD、limit 200）：`{ refs?: "head"|"all"|string(rev), author?: string, since?: string(ISO date), until?: string, grep?: string, path?: string, skip?: int>=0, limit?: int 1..200 }`。校验：`refs` 为 rev 时限长且字符集（hex/shorthand）；`path` 非选项串（不以 `-` 起）；`skip/limit` 有界。result 不变（`GitHistoryResult`）。
- `git.refs` params `{}`；result `GitHistoryItemRef[]`（含 `id/name/revision/category`）。

## 4. 后端 `history_with` 细则
- 范围：`Head`→ 现有 `head_oid`；`All`→ `--all`；`Rev(r)`→ 先 `resolve_commit(path, r)` 校验得 oid（拒空/`-` 前缀），失败→`HostServiceError("unknown-oid")`（不新增码，与 commitFiles 同码）→UI。
- 过滤追加顺序固定（可测）：`--author=<a>`、`--grep=<g>`、`--since=<s>`、`--until=<u>`，末段 `-- <path>`（若有）。
- 分页：`--skip=<skip>` + `-n{limit+1}`；`has_more = parsed.len() > limit`；`items = take(limit)`。
- metadata（current/remote/merge_base/has_in/out）沿用 HEAD 计算块，不随 `refs` 变。
- 复用现有 `parse_git_history_log`、`GIT_HISTORY_COMMIT_FORMAT`、camelCase `GitHistoryResult`（serde 不变）→ **UI/图谱零形状改动**。

## 5. UI 数据层
- `HostRepository` 扩：`loadSnapshot(query?)`、`loadRefs()`、`loadMore(after)`；`GitLogRepository` 接口相应扩展（`FixtureRepository` 同步实现，测试不破）。分页累加在 `GitLogView`（维护 `rows`/`skip`/`hasMore`/`loadingMore`）。
- Filter chips：Author 输入、Date 起/止、Path 输入、搜索(grep)、分支选择器；变更即 `skip=0` 重查。
- `git.refs` 填充选择器；`git.log refs=Rev(x)` 切分支；`refs="all"` 全分支。
- 每页 `limit=100`；`loadMore` 失败保留已加载并在详情区下方提示。

## 6. 测试与门禁
- `ade-git`：`history_with` 各参数（all/rev/author/grep/since-until/path/skip-limit hasMore）单测（临时 git 仓，多分支/merge/tag/多作者/日期跨天）；`list_refs` 分类断言。
- `ade-plugins`：`git.log` 新参数校验 + `git.refs` dispatch/gate（复用 2a FakeServices 模式，扩桩返回）。
- `ade-bridge`：真实临时仓跑 `history_with`（all + 过滤 + 分页）+ `list_refs` + `unknown-ref`；`is_inside_work_tree` 缓存命中。
- TS：`gitHistoryResultSchema`/`gitRefsSchema` 对 fixture/样例 parse；`host-contract` 扩展；UI 选择器/chips/无限滚动 reducer（node 环境纯函数：把 query→git.log params、分页累加抽出为纯函数测）。
- 门禁：`cargo test --workspace`（先 `pnpm build:orcinus-cli` 产 sidecar，见构建记忆）+ `pnpm typecheck && pnpm build:web && pnpm build:git-log && pnpm test`；plugin 测试全绿。
- 手工（tauri dev + 真多分支仓）：切全部分支图谱正确；author/date/path/grep 过滤生效；滚动加载更多不丢已加载、不重复；ahead/behind 与 HEAD 一致。

## 7. 隔离 / 安全 / 风险
- 能力仍 `git:read`（无新 kind）；`git.log`/`git.refs` 只读、`active-worktree`、路径不下发；`refs=Rev(r)`/`path` 严格校验防参数注入（`--end-of-options`、`--` 分隔、rev 先 resolve 成 oid）。
- `ade-plugins` 仍不依赖 `ade-git`（真实调用在 `ade-bridge`）。
- 分页 offset 在历史剧变时可能跳/重（VcsLog 可接受；记录）。
- `--all` 大图后端开销：靠 offset 分页 + 后端过滤约束；`is_inside_work_tree` 缓存降 probe。
- 错误码并入 union 是 TS 侧词汇表扩张（小评审点），运行时向后兼容（新码本就会被 UI 当字符串读）。
- 与并发分支无冲突：仅扩展 2a 已落地的 `git.*`，不碰 phase3c 文件。

## 8. 阶段
2b（本规格）＝全分支/过滤/分页真查询 + git.refs + 错误码 union + 探测缓存。增量 3（可选）＝blame/dashboard。
