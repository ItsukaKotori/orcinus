# Phase 4 · Git Log 插件 增量 2a（真实 git 数据接入）设计规格

- 日期：2026-10-18
- 状态：brainstorming 输出（待用户 review 后进入 writing-plans）
- 基线：`main`（已含 phase3c：UI↔worker 通道 + updater 构建规范，`9f4db603`）；工作树 `phase4-git-log-graph`（增量 1 已 rebase 到该 main 并通过门禁，`acbde936`）。
- 上游：`docs/superpowers/specs/2026-10-18-git-log-plugin-design.md`（增量 1，fixture UI）、`docs/superpowers/specs/2026-10-17-plugin-ui-worker-channel-design.md`（Phase 3C）、`docs/phase4-git-log-plugin-record.md`。
- 行为参照：`docs/phase3a-plugin-system-record.md`；IDEA `git4idea` VcsLog 语义（不移植代码）。

## 1. 背景与目标
增量 1 交付了 fixture 驱动的 Git Log 插件 UI。增量 2a 把 UI 的数据源从 fixture 换成**真实当前 worktree 的提交历史**，**复用 Rust `ade-git`** 而非在 worker 里重写 `git` 解析：新增一个 `git:read` capability 与 `git.log`/`git.commitFiles` 两个**只读、panel 可调**的 host 方法，由 host 侧解析 active worktree 路径并调用既有 `ade_git` 函数。UI 侧仅把 `FixtureRepository` 换成经 panel 桥调 host 方法的 `HostRepository`；因快照形状本就是 `GitHistoryResult` 的镜像，**视图/图谱/主题层零改动**。

**2a 边界（明确）**：接现有 `ade_git::history` 的**有界 HEAD‑scoped** 查询（默认 50、上限 200）+ 现有 `commit_diff` 文件列表。**不含** `--all` 全分支、author/date/path 过滤、skip 分页（→ 增量 2b）。

## 2. 范围
### 2.1 实现面（新增为主，ade-git 零改动）
| 面 | 载体 | 说明 |
|---|---|---|
| capability | `src/shared/plugins/plugin-capabilities.ts` + Rust `PLUGIN_CAPABILITY_KINDS` | 新增 kind `git:read`（10→11）+ 描述。只读、审计面小。 |
| host 方法 | `src/shared/plugins/plugin-host-api.ts`（`PLUGIN_HOST_API_V0`）+ Rust `host/methods.rs` dispatch | 新增 `git.log{limit?}`→`GitHistoryResult`、`git.commitFiles{oid}`→`GitBranchChangeEntry[]`；`scope:'active-worktree'`、`panel:true`、`capability:'git:read'`、`mutation:false`。 |
| gate | `plugin-capability-gate.ts` / Rust `gate.rs` | 确认 `git.log`/`git.commitFiles` 属 panel-callable（不入 `panel_forbidden`）。 |
| host service trait | `src-tauri/crates/ade-plugins/src/host/services.rs` | `PluginHostServices` 增 `git_log(&self, limit:Option<u32>)->Result<GitHistoryResultJson,HostServiceError>` + `git_commit_files(&self, oid:&str)->Result<Vec<GitBranchChangeEntryJson>,HostServiceError>`。**`ade-plugins` 不新增 ade-git 依赖**。 |
| host service impl | `src-tauri/crates/ade-bridge/src/plugins_service.rs`（`BridgePluginHostServices`） | 解析 active worktree→repo path→调 `ade_git::history(path,limit,None)` / `ade_git::compare::commit_compare(path, oid)`（取 `.entries`）；结果 serde→JSON。 |
| worktree→path | `ade-bridge/src/plugins_service.rs` `WorktreeContexts` trait | 增 `path_for(worktree_id)->Option<String>`（由现有 `list_all_worktrees` 目录项的 path 实现）；`active_worktree_context` 已有 id，复用同一目录。 |
| 插件 UI 数据层 | `plugins/git-log/src/domain/host-repository.ts`（新） | `HostRepository implements GitLogRepository`：`call('git.log',{limit})` / `call('git.commitFiles',{oid})` 走 panel postMessage（同 `readContext` 桥）。返回体 zod 校验对齐 `GitHistoryResult`。 |
| tab 挂载 | `plugins/git-log/src/ui/tab-main.tsx` | 用 `HostRepository` 替换 `FixtureRepository`。`FixtureRepository` 保留（测试/可选 dev 回退）。 |
| 加载/错误态 | `GitLogView.tsx` | `load`/`loadCommitFiles` 补 `try/catch` + loading/error 呈现（收掉增量 1 deferred 的 M-2；真实调用可 reject）。 |
| manifest | `plugins/git-log/ade-plugin.json` | `capabilities` 追加 `{kind:'git:read'}`；`version` bump。consent = `workspace:open-tab` + `git:read`。 |
| TS 结果类型 | `plugins/git-log/src/domain/types.ts` | `GitLogSnapshot=GitHistoryResult` 不变；新增 `git.log`/`git.commitFiles` 的 result zod（镜像 `plugin-host-api.ts` 现有 git serde 形状）。 |

### 2.2 不做（增量 2a 排除，归 2b/后续）
- 不扩 `ade_git::history` 签名、不加 `--all`/过滤/skip 分页（2b）。
- 不实现 Date/Path filter chips（数据不支持；2b 扩查询时一并）。
- 不做 checkout/cherry-pick/revert/reset/stash/blame 等写操作或新子系统。
- 不改现有 Source Control UI、现有 git bridge 命令（`ade-bridge/commands/git.rs`）、现有能力表既有项语义（只新增）。
- 不引入 worker / `worker:call` / spawn `git` CLI（本路走 host 方法，非 updater 的 worker spawn）。

## 3. Host 能力面（纯新增）
- `git:read` capability：只读列举/查看提交历史与单提交文件；无副作用、无路径下发（path 由 host 内部解析）。scope=`active-worktree`，仅能读调用方所绑定的当前 worktree，不能指定任意路径 → 天然限域。
- `git.log`：params `{ limit?: int 1..200 }`（复用 `GIT_HISTORY_MAX_LIMIT`）；result = `GitHistoryResult`（`items[]` + `currentRef/remoteRef/baseRef/mergeBase/hasIncomingChanges/hasOutgoingChanges/hasMore/limit`）。
- `git.commitFiles`：params `{ oid: string(7..40) }`；result = `GitBranchChangeEntry[]`（`path/status/oldPath?/added?/removed?`，源为 `ade_git::compare::commit_compare(...).entries`）。

## 4. Rust 实现要点
- `host/methods.rs`：`dispatch()` 增两条 match 分支 `git.log`/`git.commitFiles`，校验参数后调 `ctx.services.*`；错误经 `HostServiceError` → 稳定错误码（`no-active-worktree` / `not-a-git-repo` / `unknown-oid` / `unavailable`）。
- `BridgePluginHostServices`（ade-bridge）：`git_log(limit)` → `active_worktree_context()?.worktree_id` → `worktrees.path_for(id)?` → `ade_git::history(path, limit, None)`；`git_commit_files(oid)` → 同路径 → `ade_git::compare::commit_compare(path, oid)` 取 `entries`（内部走 `--name-status`）。path 解析失败/无 active worktree → `HostServiceError`。
- serde：`GitHistoryResult`/`GitBranchChangeEntry` 已带 serde（bridge 已用）；host 结果以 JSON 透传，字段与 TS `GitHistoryResult`/`GitBranchChangeEntry` **逐字段一致**（增量 1 fixture 即按此对齐，故 UI 无感）。
- 两处 `FakeServices`（`ade-plugins` host/methods.rs、service.rs）+ 新增 ade-bridge 集成测试补齐。

## 5. 插件 UI 数据层
- `HostRepository implements GitLogRepository`：`loadSnapshot()`→`git.log{limit}`；`loadCommitFiles(oid)`→`git.commitFiles{oid}`；复用 panel 侧 `orca-panel-action`/`orca-panel-action-result` 协议（同 `launcher.html`/`readContext`）。`{ok:false}` → 抛错，由 `GitLogView` 的 error 态处理。
- `tab-main.tsx` 注入 `new HostRepository()`；其余 UI（GraphList/RefPanel/CommitDetail/select-rows）不改（形状一致）。
- fixture 保留：`FixtureRepository` 仅供 `git-log-view.test.tsx`（经 `GitLogView` 的 `repository` prop 注入）；2a 运行时 `tab-main` **固定用 `HostRepository`**，不设 fixture 运行时回退开关（如需 dev 回退留 2b/后续）。

## 6. 测试与门禁
- Rust：`cargo test -p ade-plugins -p ade-bridge`（host 方法分派 + `BridgePluginHostServices` 用真/桩 worktree 目录跑 `ade_git::history`；错误码覆盖）。
- TS 单测：`HostRepository` 对 mock 桥（resolve `git.log`/`git.commitFiles` 结果）的形状解析 + 错误路径；`git.log` result zod 合法性。增量 1 既有测试（parity/fixture/graph/select-rows/launcher/UI 冒烟）保持绿。
- 自动化门禁：`pnpm typecheck && pnpm build:web && pnpm build:git-log && pnpm test` + `cargo test --workspace`（本增量**有 Rust 改动**，cargo 必跑）。
- 手工（tauri dev + 真仓库）：开插件（consent 含 `git:read`）→ launcher 开 tab → 图谱/refs/详情显示**真实** HEAD 历史；选中提交显示真实文件列表；非 git worktree / 无 active worktree → 显示错误态不崩。

## 7. 隔离 / 安全 / 风险
- **表变更风险已降低**：phase3c 已并入 main，能力/方法表稳定；本增量对两张表**纯新增**（`git:read` + 两方法），与 updater 的 `worker:call`/`session:push` 互不重叠。
- **限域读**：`active-worktree` scope + host 内部解析 path（插件无法指定路径/读别的 worktree）→ 无越权读面。
- **重新 consent**：新增 capability 改变 consent 指纹 → 用户需重新授权 `git:read`（记录/README 说明）。
- **快照形状契约漂移**：TS zod 与 Rust serde 必须逐字段一致；plan 含一个契约对齐测试（同一 fixture/样例 JSON 两侧解析）防漂移。
- **依赖注入**：`git_*` 经 `PluginHostServices` trait 注入，`ade-plugins` 仍不依赖 ade-git（实现落在 ade-bridge）；`WorktreeContexts.path_for` 为小增量扩展。

## 8. 增量归属
- **2a（本规格）**：真实 HEAD‑scoped 历史 + commit files 经 host 方法进 UI；加载/错误态。
- **2b（后续）**：扩 `ade_git` 支持 `--all`/author·date·path 过滤/skip 分页；补 Date/Path filter chips、真实分页；分支选择器接真 ref。
- **增量 3（可选）**：blame、graph dashboard 等，参照 git4idea。
