# 分支创建 / 设上游 / 显差异（增量 3）Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 在常驻分支树的右键/`⋯` 菜单里补上三个**纯本地、非破坏**动作：`Show Diff with Working Tree`（复用已存在的 `git_branch_diff`）、`New Branch from…`（新命令 `git_branch_create`）、`Set Upstream To…`（新命令 `git_branch_set_upstream`，子菜单列 `refs/remotes/*`）。

**Architecture:** 后端照 2A 的 `ade_git::branch_ops` 模式加 `create()` / `set_upstream()`，各走 §3.5 的 7 面镜像；两条命令的拒绝共用一个 `BranchOperationRefusal` 家族（结构化软错误，**不经 `BridgeError`**），增量 4 的 rename/delete 复用同一 enum。前端在 2B 的 `branch-actions`/`branch-menu` 接缝上扩展：加 3 个动作 id + 子菜单能力（Radix `ContextMenuSub`/`DropdownMenuSub`）+ 一个新建分支名字输入框；编排挂在 2C 的 `SourceControlBranchTreeSection`（复用其 ctx/确认框/invalidate 机制）。**本增量无远端网络、无 `beginRemoteOperation`**（那是增量 5）。

**Tech Stack:** Rust（`ade-git`/`ade-bridge` + tauri-specta 绑定再生、真 git 仓集成测试）、TypeScript、React、Radix（子菜单）、Vitest + happy-dom、`cargo test`。

**Spec:** `docs/superpowers/specs/2026-10-24-idea-branch-tree-switcher-design.md` §7.4（动作矩阵）、§8.1（结构化拒绝）/§8.3（刷新分级）、§9（后端命令）、§3.4（i18n）/§3.5（7 面）/§3.6（本地 only）。路线图：`docs/superpowers/plans/2026-10-26-branch-actions-roadmap-increments3-5.md`。模板：`docs/superpowers/plans/2026-10-25-branch-checkout-increment2a.md`（后端 + 7 面）。

## Global Constraints

- **两条新命令各走 §3.5 的 7 面镜像**，否则 `specta_export::tests::bindings_are_fresh` + `command_inventory_and_bindings_are_fresh` 红：ade-git 实现 → `ade-bridge/commands/git.rs` 的 `*_impl` + `#[tauri::command]` → `specta_export.rs` `collect_commands!` 注册 → 再生 `tauri-bindings.ts` → `real/git.ts` `commandCases` → `preload-api` 类型 → mock 面**不补**（渲染测试自桩）。
- **拒绝走结构化软错误**：`git_branch_create` / `git_branch_set_upstream` 返回 `Ok(Outcome{ status, ... })`，业务拒绝在 `status`；`Err(CoreError)` 只留给环境级失败。`BridgeError` 会把变体压成 `{message}` 丢类型（§8.1/§13.5 教训）。新增共享 enum `ade_git::BranchOperationRefusal { NameTaken, CheckedOut, UpstreamMissing }`（增量 4 会加 `NotMergeable`）——**两条命令 + 未来的 rename/delete 都用它**，别各写各的。
- **名字校验复用 `resolve_create_branch_name`**（`ade-git/src/branch.rs:130`）：`-` 前缀拒绝 + `git check-ref-format --branch`。`git_branch_create` 的新名与增量 4 rename 的新名都过它。
- **start-point / upstream 必须是真实存在的 ref，绝不把外部字符串当任意 revision 丢给 git**（照 `branch_ops::resolve` 的 `ref_exists` 前缀校验思路）。create 的 start-point = 该 `GitRefEntry.oid`（§7.4）；upstream 必须落在 `refs/remotes/*`。
- **只做本地**：`runtime`/`runtimeEnvironments` 无 real 实现（§3.6）→ 两命令只加 `window.api.git.*` 本地路，不加 `callRuntimeRpc`。
- **刷新分级（§8.3）**：create / set-upstream **不改当前 workspace 分支** → 成功后**仅 `invalidate()`**（refs 变了），不调 `refreshActiveGitStatusAfterMutation`。Show Diff 是只读，无刷新。
- **i18n**：新文案进 `auto.components.git-branch`，只补 en+zh（§3.4）；`translate` 惰性求值（`no-top-level-translate` AST 门禁）。
- 仓库**无 pre-commit 钩子**；门禁手动：`cargo test -p ade-git -p ade-bridge` + `pnpm typecheck` + 定向 `pnpm vitest run`。
- **不做** Update / Push（远端 op，增量 5）；**不做** Rename / Delete（破坏性，增量 4）。

## 已核实、可直接依赖的事实（本计划撰写前已实测）

| 项 | 结论（文件:行） |
|---|---|
| 新命令模板 | `ade-git/src/branch_ops.rs`：`GitCheckoutTarget`/`GitCheckoutStatus`/`GitCheckoutOutcome` + `resolve`/`ref_exists`(`rev-parse --verify --quiet`)/`run`/`occupant`。create/upstream 照此形状。 |
| 名字校验 | `resolve_create_branch_name(repo_path, override, prefix, name)`（`branch.rs:130`）：override 非空则校验（`-` 前缀拒 + `check-ref-format --branch`），否则 prefix+sanitized 拼接。create 用它校验用户输入的新名。 |
| 7 面 + 手抄类型镜像模板 | 2A 交付：`src/shared/git-checkout-types.ts`（手抄镜像，`specta` 生成件不锁它）、`real/git.ts` `commandCases`、`preload-api/api/git-operation-api.ts`、`runtime/runtime-git-checkout-client.ts` 的 `checkoutRuntimeGit`。新命令各建对应镜像。 |
| git 命令 | create：`git branch -- <name> <startOid>`（不检出，仅建 ref）。set-upstream：`git branch --set-upstream-to=<remoteName> -- <branch>`。二者均非网络（§3.7 死路径只关 fetch/pull/fastForward，与此无关）。 |
| Show Diff 现成 | `git_branch_diff` / `git_branch_compare` 命令已在（§3.1「✅ 现成」），面板 `panel-content.tsx` 已有 `openBranchAllDiffs`。增量 3 只把「Show Diff」接进菜单动作，**无新后端**。 |
| 子菜单能力 | `components/ui/context-menu.tsx` 导出 `ContextMenuSub`/`ContextMenuSubTrigger`/`ContextMenuSubContent`（已核 export）；`dropdown-menu.tsx` 有 `DropdownMenuSub`/`...Trigger`/`...Content`。`branch-menu.tsx` 现只渲染平铺 `ContextMenuItem`/`DropdownMenuItem`——需加子菜单分支。 |
| 上游候选数据源 | `useBranchRefs` 已返回全部 `GitRefEntry[]`（含 `category==='remote'` 项，`name` 形如 `origin/feature`）。Set Upstream 子菜单**直接取已加载的 remote refs**，无需新命令。 |
| 动作表现状 | `branch-actions.ts`：`BranchActionId = 'checkout'|'switch'|'new-workspace'|'view-in-log'`；`BranchAction` 有 `visible`/`enabled?`/`run`；`BranchActionContext` 有 `checkout?`。宿主裁剪 = `allowedActionIds` + 不传回调。`branch-menu.tsx` `BranchRowMenu` 按 `allowedActionIds ∧ visible` 出项。 |
| 常驻区块宿主 A | `source-control-branch-tree-section.tsx`：`ctx`（checkout/switch/new-workspace）+ `executeCheckout` + `pendingCheckout` 确认框 + `invalidate`；`HOST_A_ALLOWED`。新动作挂在此。 |
| 单测/集成测试镜像点 | ade-git 集成：`ade-git/tests/branch_checkout.rs`（真 git 仓，F1/F2/正常/非法/detached 拒绝）。ade-bridge 契约：`ade-bridge/tests/git_checkout.rs`（含 `refusal_serializes_as_discriminated_union`）。新命令各建 `branch_create.rs` / `git_branch_create.rs` 等同风格。 |

---

## 文件结构

**新建（Rust）**
| 文件 | 职责 |
|---|---|
| `ade-git/src/branch_ops.rs`（扩展，非新文件） | 加 `BranchOperationRefusal` 共享 enum + `GitBranchCreateOutcome`/`GitBranchSetUpstreamOutcome` + `create()` + `set_upstream()` |
| `ade-git/tests/branch_create.rs`、`ade-git/tests/branch_set_upstream.rs` | 真 git 仓集成测试（NameTaken / UpstreamMissing / 非法 / 正常） |

**新建（TS）**
| 文件 | 职责 |
|---|---|
| `src/shared/git-branch-ops-types.ts` | `GitBranchCreateOutcome` / `GitBranchSetUpstreamOutcome` 的 TS 手抄镜像（对齐 `git-checkout-types.ts`） |
| `src/renderer/src/runtime/runtime-git-branch-client.ts` | `createRuntimeGitBranch(context, {name, startOid})` / `setRuntimeGitBranchUpstream(context, {branch, upstream})`（仅本地；非 local 显式抛，照 `checkoutRuntimeGit`） |
| `src/renderer/src/components/git-branch/new-branch-dialog.tsx` | 「New Branch from…」名字输入框（照 `discard-dialog`/`ProjectGroupNameDialog` 形状，含空名禁用） |
| 对应 `.test` | 上述各件的测试 |

**修改**
| 文件 | 改动 |
|---|---|
| `ade-git/src/lib.rs` | `pub use` 新类型 |
| `ade-bridge/src/commands/git.rs` | `git_branch_create` + `git_branch_set_upstream` 的 `*_impl` + `#[tauri::command]` |
| `ade-bridge/src/specta_export.rs` | `collect_commands!` 注册两条 + `export_lists_every_command` 花名册（§3.5 附加登记） |
| `src/bridge/real/generated/tauri-bindings.ts` | `export-bindings` 再生 |
| `src/bridge/real/git.ts` | `commandCases` + 映射 `branchCreate`/`branchSetUpstream` |
| `src/shared/preload-api/api/git-operation-api.ts` | 两个方法类型 |
| `git-branch/branch-actions.ts` | `BranchActionId` 加 `show-diff`/`new-branch`/`set-upstream`；`BranchAction` 加可选 `submenu?`；`BranchActionContext` 加 `showDiff?`/`newBranch?`/`setUpstream?` + `remoteRefCandidates?`；`BRANCH_ACTIONS` 加三项 |
| `git-branch/branch-menu.tsx` | `renderItems` 支持子菜单（`ContextMenuSub`/`DropdownMenuSub`） |
| `git-branch/source-control-branch-tree-section.tsx` | ctx 注入三动作 + `HOST_A_ALLOWED` 加三项 + New Branch 走名字框 + 编排（create/upstream + `invalidate`） |
| `i18n/locales/en.json`/`zh.json` | `auto.components.git-branch` 补新文案（en+zh） |

> mock 面 `git-api.ts` **不加** `branchCreate`/`branchSetUpstream`（靠 `withMethodFallback` 兜 unimplemented，与 `checkout`/`refs` 一致，§3.5 第 7 面）。

---

## Task 1: `git_branch_create`（后端 + 7 面 + runtime + TS 契约）

**Files:**
- Modify: `src-tauri/crates/ade-git/src/branch_ops.rs`（加 `BranchOperationRefusal`、`GitBranchCreateOutcome`、`create()`）
- Modify: `ade-git/src/lib.rs`、`ade-bridge/src/commands/git.rs`、`specta_export.rs`
- Create: `ade-git/tests/branch_create.rs`、`ade-bridge/tests/git_branch_create.rs`
- Create: `src/shared/git-branch-ops-types.ts`、`src/renderer/src/runtime/runtime-git-branch-client.ts`（本任务只加 create 半边）
- Modify: `src/bridge/real/git.ts`、`src/bridge/real/generated/tauri-bindings.ts`（再生）、`src/shared/preload-api/api/git-operation-api.ts`
- Test: `src/renderer/src/runtime/runtime-git-branch-client.test.ts`

**Interfaces:**
- Consumes: `resolve_create_branch_name`、`command::run`/`run_text`、`ref_exists`、`worktree_list`
- Produces:
  - Rust：`pub enum BranchOperationRefusal { NameTaken, CheckedOut { worktree_path: String }, UpstreamMissing }`（增量 4 加 `NotMergeable`）；`create(worktree_path, args) -> Result<GitBranchCreateOutcome, CoreError>`。
  - `GitBranchCreateOutcome { status: GitBranchCreateStatus, branch?, message? }`，`GitBranchCreateStatus = Created | { refused: BranchOperationRefusal }`（可判别联合，同 2A `#[serde(rename_all="kebab-case")]` 风格；`Refused` 变体内嵌 enum）。
  - TS：`window.api.git.branchCreate({ worktreePath, args: { name, startOid } })` → `Promise<GitBranchCreateOutcome>`；`createRuntimeGitBranch(context, {name, startOid}): Promise<GitBranchCreateOutcome>`。

- [ ] **Step 1: 写失败集成测试（ade-git，真 git 仓）** — `ade-git/tests/branch_create.rs`

镜像 `ade-git/tests/branch_checkout.rs` 的临时仓 harness（建 bare 仓 + worktree + commit）。用例：
```rust
#[test]
fn creates_branch_at_start_point_without_checking_it_out() {
    let repo = TestRepo::with_commits(); // 复用 branch_checkout.rs 的建仓工具
    let start_oid = repo.head_oid();
    let outcome = branch_ops::create(repo.path(), &branch_ops::GitBranchCreateArgs {
        name: "feature/new-branch".into(), start_point: start_oid.clone(),
    }).unwrap();
    assert!(matches!(outcome.status, GitBranchCreateStatus::Created));
    // 建了 ref 但 HEAD 没动：
    assert!(repo.ref_exists("refs/heads/feature/new-branch"));
    assert_eq!(repo.current_branch(), "main");
}

#[test]
fn rejects_taken_name() { /* 已存在 refs/heads/main → Refused(NameTaken) */ }
#[test]
fn rejects_invalid_name_via_check_ref_format() { /* name 含空格或 `-` 前缀 → Err(InvalidInput) 或 Refused；按 resolve_create_branch_name 语义断 */ }
#[test]
fn rejects_missing_start_point() { /* start_point 解析不到 → Err 或 InvalidStartPoint（择一，与 create 设计一致，见 Step 3） */ }
```
> 关键：`create` 只建 ref，**绝不 checkout**（断 current_branch 不变）。`NameTaken` 用结构化 status，不是 `Err`。

- [ ] **Step 2: 运行确认失败** `cargo test -p ade-git --test branch_create` → 编译失败/红。

- [ ] **Step 3: 实现 `create()`（branch_ops.rs）**

```rust
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "kebab-case")]
pub enum BranchOperationRefusal {
    NameTaken,
    CheckedOut { #[serde(rename = "worktreePath")] worktree_path: String },
    UpstreamMissing,
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
    #[serde(skip_serializing_if = "Option::is_none")] pub branch: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")] pub message: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct GitBranchCreateArgs { pub name: String, pub start_point: String }

pub fn create(worktree_path: &str, args: &GitBranchCreateArgs) -> Result<GitBranchCreateOutcome, CoreError> {
    // 1) 新名校验（复用 resolve_create_branch_name；override 形态：无 prefix）
    let name = crate::resolve_create_branch_name(worktree_path, Some(&args.name), None, "")?;
    // 2) 名未被占用
    if ref_exists(worktree_path, &format!("refs/heads/{name}")) {
        return Ok(GitBranchCreateOutcome { status: GitBranchCreateStatus::Refused(BranchOperationRefusal::NameTaken), branch: Some(name), message: None });
    }
    // 3) start-point 必须是真实可解析的 rev（只信 oid / ref_exists，不拼任意 revision）
    if !ref_exists(worktree_path, &args.start_point) {
        return Err(crate::git_command_failed(&["rev-parse", "--verify", &args.start_point], &/* 合成非零 */ default_failure_output()));
    }
    // 4) git branch -- <name> <start-point>（不检出）
    run(worktree_path, &["branch", "--", name.as_str(), args.start_point.as_str()])?;
    Ok(GitBranchCreateOutcome { status: GitBranchCreateStatus::Created, branch: Some(name), message: None })
}
```
> `ref_exists` 接受 oid 吗？`rev-parse --verify --quiet <oid>` 对合法 commit oid 返回 oid → 非空，成立。若 start_point 是任意串既非 ref 也非 oid，`ref_exists` false → 拒。`default_failure_output()` 若不便，改成 `CoreError::InvalidInput("unknown start point")`（环境不依赖 git stderr）。**执行时按 `command.rs` 现有 API 取最简错误构造，不新造 Output。**

- [ ] **Step 4: 运行 ade-git 集成测试通过** `cargo test -p ade-git --test branch_create`。

- [ ] **Step 5: bridge 命令 + 7 面**

- `ade-bridge/commands/git.rs`：`GitBranchCreateArgs` 复用/透传 + `git_branch_create_impl(...) -> Result<GitBranchCreateOutcome, BridgeError>`（鉴权 worktree + 阻塞调度，照 `git_checkout_impl`）+ `#[tauri::command] pub async fn git_branch_create`。
- `specta_export.rs`：`collect_commands!` 加 `commands::git::git_branch_create`；`export_lists_every_command` 花名册登记。
- 再生绑定：`cargo run -p ade-bridge --bin export-bindings`（或 `pnpm` 里对应脚本）→ 更新 `tauri-bindings.ts`。
- `real/git.ts`：`commandCases` 加 `git_branch_create`（供 `command_inventory` 测试），映射 `branchCreate: (args) => invokeCommand('git_branch_create', { args })`。
- `preload-api/api/git-operation-api.ts`：`branchCreate(args: { worktreePath: string; args: GitBranchCreateArgs }): Promise<GitBranchCreateOutcome>`。
- 手抄镜像 `src/shared/git-branch-ops-types.ts`：`GitBranchCreateStatus`/`GitBranchCreateOutcome`/`GitBranchCreateArgs`/`BranchOperationRefusal`（camelCase 字段、`?: T` 非 `?: T|null`，对齐 `git-checkout-types.ts` 注释理由）。
- `runtime-git-branch-client.ts`：`createRuntimeGitBranch(context, {name, startOid})` 照 `checkoutRuntimeGit`：非 local 显式抛、解析本地 path、`window.api.git.branchCreate({ worktreePath, args: { name, startOid } })`。`runtime-git-client.ts` re-export。
- `ade-bridge/tests/git_branch_create.rs`：契约测试，含 `refusal_serializes_as_discriminated_union`（镜像 2A 那条），锁 `status` 形状 = 手抄镜像对齐。

- [ ] **Step 6: 全 workspace 编译 + bindings 测试绿** `cargo test -p ade-bridge`（`bindings_are_fresh` / `command_inventory_and_bindings_are_fresh`）。

- [ ] **Step 7: runtime client TS 测试** `runtime-git-branch-client.test.ts`：断 `branchCreate` 收到 `{worktreePath, args}`、业务拒绝不被改写成异常、环境级上抛、空路径本地拒、非 local 目标显式抛（照 `runtime-git-checkout-client.test.ts` 逐条）。`pnpm vitest run src/renderer/src/runtime`。

- [ ] **Step 8: 提交** `git add -A && git commit -m "feat(git-branch): git_branch_create（后端 branch_ops::create + 7 面 + runtime + TS 契约 + 集成测试）"`

---

## Task 2: `git_branch_set_upstream`（后端 + 7 面 + runtime + TS 契约）

**Files:** 同 Task 1 布局，命令换 `git_branch_set_upstream`；测试 `ade-git/tests/branch_set_upstream.rs`、`ade-bridge/tests/git_branch_set_upstream.rs`。

**Interfaces:**
- Produces: `set_upstream(worktree_path, args: { branch, upstream }) -> Result<GitBranchSetUpstreamOutcome, CoreError>`；`GitBranchSetUpstreamStatus = Set | Refused(BranchOperationRefusal)`（复用 Task 1 的 enum，填 `UpstreamMissing`）。TS：`window.api.git.branchSetUpstream({ worktreePath, args })`；`setRuntimeGitBranchUpstream(context, {branch, upstream})`。

- [ ] **Step 1: 失败集成测试**（`branch_set_upstream.rs`）
  - 正常：`git branch --set-upstream-to=origin/feature -- feature` 后 `rev-parse --abbrev-ref --symbolic-full-name feature@{upstream}` == `origin/feature`，且分支**未切换**。
  - `UpstreamMissing`：upstream 不在 `refs/remotes/*` → `Refused(UpstreamMissing)`（不是 `Err`）。
  - 分支不存在 → 拒或 `Err`（按实现定；倾向 `Refused`/`InvalidInput`，Step 3 锁定）。
  - 需先建带 remote 的测试仓（`git clone --bare` 或加 fake remote ref；照 `branch_checkout.rs` 的 remote fixture）。

- [ ] **Step 2: 运行失败。**
- [ ] **Step 3: 实现 `set_upstream()`**
```rust
pub fn set_upstream(worktree_path: &str, args: &GitBranchSetUpstreamArgs) -> Result<GitBranchSetUpstreamOutcome, CoreError> {
    let branch = args.branch.trim();
    if branch.is_empty() || !ref_exists(worktree_path, &format!("refs/heads/{branch}")) {
        return Ok(GitBranchSetUpstreamOutcome { status: Refused(BranchOperationRefusal::UpstreamMissing), ..}); // 或专用 InvalidBranch；执行时定
    }
    // upstream 形如 origin/feature，必须落在 refs/remotes/*
    if !ref_exists(worktree_path, &format!("refs/remotes/{}", args.upstream.trim())) {
        return Ok(Refused(BranchOperationRefusal::UpstreamMissing));
    }
    let arg = format!("--set-upstream-to={}", args.upstream.trim());
    run(worktree_path, &["branch", &arg, "--", branch])?;
    Ok(Set)
}
```
> 与 create 共判：`branch`/`upstream` 都过 `ref_exists` 前缀校验，不接受任意串（防注入，同 2A resolve 思路）。

- [ ] **Step 4: `cargo test -p ade-git --test branch_set_upstream` 绿。**
- [ ] **Step 5: bridge 7 面**（同 Task 1 Step 5 的六处登记，命令换 `git_branch_set_upstream`；`git-branch-ops-types.ts` 加 `GitBranchSetUpstreamOutcome`；runtime 加 `setRuntimeGitBranchUpstream`）。
- [ ] **Step 6: `cargo test -p ade-bridge` 绿（bindings fresh + 契约测试）。**
- [ ] **Step 7: runtime 测试 + `pnpm vitest run src/renderer/src/runtime`。**
- [ ] **Step 8: 提交。**

---

## Task 3: 动作表扩展（show-diff / new-branch / set-upstream + 子菜单能力）

**Files:** `git-branch/branch-actions.ts`、`git-branch/branch-menu.tsx`、`branch-actions.test.ts`、`branch-menu.test.tsx`。

**Interfaces:**
- Produces:
  - `BranchActionId` 加 `'show-diff' | 'new-branch' | 'set-upstream'`。
  - `BranchAction.submenu?: (row, ctx) => BranchSubmenuItem[]`；`type BranchSubmenuItem = { id: string; label: () => string; enabled: boolean; run: () => void }`。
  - `BranchActionContext` 加 `showDiff?: (ref: GitRefEntry) => void`、`newBranch?: (ref: GitRefEntry) => void`、`setUpstream?: (branch: string, upstream: string) => void`、`remoteRefCandidates?: (forBranch: string) => GitRefEntry[]`（宿主提供；返回该仓 remote refs，供子菜单）。
  - `BRANCH_ACTIONS` 三项：
    - `new-branch`：`visible` = ref 行（Local/Remote/Tag 全可，start-point 用 oid）；`run` → `ctx.newBranch?.(ref)`。恒 `enabled`。
    - `show-diff`：`visible` = ref 行且 `!ref.isHead`（当前分支无 diff 对象）；`run` → `ctx.showDiff?.(ref)`。
    - `set-upstream`：`visible` = `category==='local'` 且 `ctx.setUpstream` 与 `remoteRefCandidates` 都在；`submenu` = `remoteRefCandidates(ref.name)` 映射成子项（同名优先排前），每项 `run` → `ctx.setUpstream(ref.name, upstream.name)`。
  - `branch-menu.tsx` `renderItems`：动作有 `submenu` 时用 `ContextMenuSub`+`ContextMenuSubTrigger`+`ContextMenuSubContent`（dropdown 版用 `DropdownMenuSub*`）；无 submenu 保持现有 `Item`。子项 `enabled=false` 置灰。

- [ ] **Step 1: `branch-actions.test.ts` 追加失败测试**
  - `new-branch` 对 local/remote/tag 三种 ref 行都 visible，`run` 把 ref 交 `ctx.newBranch`。
  - `show-diff` 对 `isHead` 行不可见，对非 head local/remote 可见。
  - `set-upstream` 仅 local 可见；`submenu` 返回按同名优先排序的 remote 项；宿主不传 `setUpstream`/`remoteRefCandidates` → 不可见（宿主裁剪）。
  - 宿主 B 门禁：这三项在宿主 A 的 `allowedActionIds` 之外时不出现（由 Task 4 的集合控制；此处只测 visible 逻辑）。
- [ ] **Step 2: `branch-menu.test.tsx` 追加失败测试**：mock submenu 原语内联渲染，断 `set-upstream` 行渲染出子菜单项（含 remote 短名）、`show-diff`/`new-branch` 平铺项存在。
- [ ] **Step 3: 运行确认失败。**
- [ ] **Step 4: 实现 branch-actions.ts + branch-menu.tsx（submenu 渲染）**。
- [ ] **Step 5: 两测试文件绿** `pnpm vitest run src/renderer/src/components/git-branch/branch-actions.test.ts src/renderer/src/components/git-branch/branch-menu.test.tsx`。
- [ ] **Step 6: 判别力实证**：临时删 `renderItems` 里 `submenu` 分支 → set-upstream 子菜单测试转红；删 `set-upstream.visible` 的 `local` 判定 → local-only 测试转红。还原。
- [ ] **Step 7: 提交。**

---

## Task 4: 宿主 A 接线（New Branch 名字框 + upstream/creedate 编排 + Show Diff + i18n）

**Files:** 新建 `git-branch/new-branch-dialog.tsx` + 测试；改 `source-control-branch-tree-section.tsx` + 其测试；i18n en/zh。

**Interfaces:**
- Consumes: `createRuntimeGitBranch`/`setRuntimeGitBranchUpstream`（Task 1/2）、`new-branch`/`set-upstream`/`show-diff` 动作（Task 3）、`invalidate`（2C）、`model.activeRepoSettings`、现有 `openBranchAllDiffs`/`git_branch_diff`（Show Diff）。
- Produces: 区块内 `newBranchTarget: GitRefEntry | null` + `NewBranchDialog`；`remoteRefCandidates` 从 `refs`（useBranchRefs 已加载）里 `category==='remote'` 过滤。

- [ ] **Step 1: 写失败测试（扩展 `source-control-branch-tree-section.test.tsx` + 新建 `new-branch-dialog.test.tsx`）**
  - `New Branch from…`：点动作 → 弹名字框（含占位/start-point 说明）；输入合法名点创建 → `window.api.git.branchCreate` 收到 `{worktreePath:'/a', args:{name, startOid: <ref.oid>}}`；成功后 `invalidate`（refs 二次取数）；**不调** refresh（§8.3：非当前分支）。
  - `NameTaken` 拒绝 → 走 i18n 提示（按 `status`，非 message），名字框不关/或给错误。
  - 空名/非法名（`-` 前缀、含空格）→ 创建按钮禁用或键入即校验。
  - `Set Upstream To…`：子菜单出 remote 候选；选 `origin/feature` → `window.api.git.branchSetUpstream` 收到 `{branch:'feature', upstream:'origin/feature'}`；成功后 invalidate。`UpstreamMissing` → 提示。
  - `Show Diff`：点 → 调 `showDiff`(→ 现有 `openBranchAllDiffs`/branch diff 路由)，不写不刷。
  - `HOST_A_ALLOWED` 现含 `{checkout, switch, new-workspace, new-branch, set-upstream, show-diff}`；**不含 view-in-log**（宿主 B 专属）。
  - Git Log 宿主（宿主 B）**不**声明 `newBranch`/`setUpstream`/`showDiff` 也不传 `renderRowMenu` → 这些项在宿主 B 不出现（既有 Toolbar 不受影响）。
- [ ] **Step 2: 运行失败。**
- [ ] **Step 3: 实现 `new-branch-dialog.tsx`**（`Dialog` + `Input` + `onCreate(name)`；空/非法禁用确认；标题惰性 i18n）+ 区块内编排：
  - `ctx.newBranch = (ref) => setNewBranchTarget(ref)`（弹框）；确认 → `createRuntimeGitBranch({settings, worktreeId, worktreePath: worktree.path}, {name, startOid: ref.oid})` → 成功 `invalidate()`+关框，`NameTaken`→`toast` i18n 文案。
  - `ctx.setUpstream = (branch, upstream) => setRuntimeGitBranchUpstream(...)` → 成功 `invalidate()`，`UpstreamMissing`→提示。
  - `ctx.showDiff = (ref) => openBranchAllDiffsForRef(ref)`（复用面板现有；区块内取 store action / 或直接 `window.api.git.branchDiff` + 打开，执行时对齐 panel-content 的现成调用）。
  - `remoteRefCandidates = (name) => refs.filter(r => r.category==='remote')` 同名优先排序。
  - **无 `beginRemoteOperation`**（本地、非远端 op）。
- [ ] **Step 4: 实现 i18n**：`auto.components.git-branch` 补 `newBranchFrom`("New Branch from…"/"从此分支新建分支…")、`showDiff`("Show Diff with Working Tree"/"与工作区显示差异")、`setUpstreamTo`("Set Upstream To…"/"设置上游为…")、`newBranchDialogTitle`/`newBranchCreate`/`newBranchNamePlaceholder`、`createNameTaken`/`setUpstreamMissing`（en+zh 两处）。
- [ ] **Step 5: 绿 + 定向回归** `pnpm vitest run src/renderer/src/components/git-branch`。
- [ ] **Step 6: 全量 typecheck** `pnpm typecheck`。
- [ ] **Step 7: 提交。**

---

## 收尾门禁

- [ ] `cargo test -p ade-git -p ade-bridge`（含 `bindings_are_fresh` / `command_inventory_and_bindings_are_fresh` / 两条契约测试 / 新集成测试）。
- [ ] `pnpm typecheck`。
- [ ] `pnpm vitest run src/renderer/src/components/git-branch src/renderer/src/components/right-sidebar src/renderer/src/runtime src/renderer/src/i18n`（防 2B/2C 那类面板崩桩回归）。
- [ ] 无破坏性 → 本增量**不要求**全量 `pnpm test`（那是增量 4 的门禁）；但广域须绿。
- [ ] 增量末全分支 CodeReview（范围 增量3 全部提交）。
- [ ] 手工验收（`pnpm dev`）：树里右键/`⋯` 出 New Branch/Set Upstream/Show Diff；名字框校验；子菜单列 remote；建分支后树立即出现新分支（invalidate）；HEAD 不被动（create 不检出）。

## 范围外（留增量 4 / 5）
- Rename / Delete / `-D` 二次确认 / F2 → **增量 4**（破坏性，全量 `pnpm test` 门禁）。
- Update / Push… / fastForward/syncFork/rebase（O-5）/ `beginRemoteOperation` 单飞 / 网络防挂起 → **增量 5**（远端）。
- 远程 runtime 路（`runtime`/`runtimeEnvironments` 无 real 实现，§3.6）→ 维持显式拒绝。
