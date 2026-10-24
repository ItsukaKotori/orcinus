# 设计：IDEA 风格分支树 + 分支切换（Source Control 头部 / Git Log 共用一棵树）

- 日期：2026-10-24
- 状态：待用户 review
- 落点：`/Users/itsuka/CodeSpace/ade`

## 1. 背景与目标

ade 右侧栏 Source Control 面板头部目前只显示**当前**分支身份：`SourceControlBranchContextRow`
（`src/renderer/src/components/right-sidebar/source-control/panel/branch-context-row.tsx`）
把 HEAD 渲染成 `HeadIdentity` —— 一个不可点的 `span`（同文件 82-122 行），下一行是
`→ <baseRef>` 比较基线。用户在这里**看不到其它分支、也无法切分支**。

Git Log 原生视图另有一份分支列表 `RefPanel`（`src/renderer/src/components/git-log/RefPanel.tsx`），
但它是一个扁平三组（branches / remote branches / tags）的过滤器，语义是「把 Log 的查看 ref 设成谁」，
与「切换工作分支」毫无关系，且和头部各说各话。

**目标**：

1. 在 Source Control 头部提供 IDEA 风格的分支树（搜索 + `HEAD (Current Branch)` / `Local` /
   `Remote` 按 remote 二级分组 / `Tags`），用于**方便地切换分支**。
2. 分支树条目携带信息密度：ahead/behind 精确计数、当前分支标记、被哪个 workspace 检出的标记。
3. 提供 IDEA 的分支动作集（New Branch from… / New Worktree from… / Show Diff with Working Tree /
   Update / Push… / Tracked Branch / Rename / Delete）。
4. 消除两份分支呈现的重复：`RefPanel` 左栏物理删除，全仓库只留**一个**树组件。

## 2. 关键决策记录（已与用户确认）

1. **切换语义 = 跳到已检出该分支的 workspace**（不是原地 `git checkout`）。未被任何 workspace 检出的
   分支 → 走「从该分支新建 workspace」。**不引入 `git checkout` 能力。**
2. **落点范围**：Source Control 头部 popover + 同步重构 Git Log 左侧树。
3. **Git Log 左栏合入右侧栏，只留一份树**：删除 `GitLogView` 的 `RefPanel` 左栏；Git Log 需要按 ref
   过滤时，点工具栏那个 branch chip 弹出**同一个树组件**，此时单击语义 = 设查看 ref。
4. **动作集档位 = 对齐 IDEA 全菜单**（含 create-branch / set-upstream / rename / delete）。
5. **破坏件边界**：`Rename` / `Delete` **只对未被任何 worktree 检出的分支启用**。已检出的置灰并说明原因。
   `Rename` **不同步改写** workspace 元数据 —— 未检出分支本就无 workspace 引用它（见 §6.4 论证）。
6. **契约换型不做兼容**：`GitRefsResult.refs` 换元素类型，不复用 `GitHistoryItemRef`，不保留旧形状。
7. **已检出标记在前端算**（从 store `Worktree[]`），不进后端；但前端标记仅为 advisory，
   **权威判定在后端执行时用 `ade_git::worktree_list` 复核**。
8. **新写命令只做本地路径**：`window.api.git.*`（tauri 命令）。远程 `callRuntimeRpc` 分支保持与
   现有 push/pull 同构的写法，但**不为此新增仓库外 runtime host 能力**（该面当前本就未实现，见 §3.6）。

## 3. 现状事实基线（已逐条核实）

这些事实决定了成本表，任何一条变化都要回到本 spec 重评。

### 3.1 动作集的成本表

| IDEA 菜单项 | ade 落点 | 状态 |
|---|---|---|
| Checkout（切分支） | `setActiveWorktree`（`store/slices/worktrees/session/set-active-worktree.ts`）+ `Worktree.branch` | ✅ 现成 |
| New Worktree from… | `openModal('new-workspace-composer', { initialRepoId, initialBaseBranch })`（`NewWorkspaceComposerModal.tsx` 的 `ComposerModalData`） | ✅ 现成 |
| Show Diff with Working Tree | `git_branch_diff` / `git_branch_compare` + `openBranchAllDiffs` | ✅ 现成 |
| Push… | `git_push` + `pushBranch(worktreeId, worktreePath, …)`（`store/slices/editor/actions/git-remote-push-pull.ts`） | ✅ 现成 |
| Update | `pullRuntimeGit` / `fastForwardRuntimeGit`（`runtime/runtime-git-sync-client.ts`）存在，但签名吃 `RuntimeGitContext`，绑当前 worktree | ⚠️ 需参数化 |
| Delete | 逻辑只存在于 `ade-git/src/worktree_remove.rs:147`（`["branch", flag, "--", branch_name]`）的私有函数 + checked-out 拒绝处理 | ⚠️ 需抽公共 |
| New Branch from… | 只有随 worktree 创建时 `--no-track -b`（`worktree_create.rs`） | ❌ 全新命令 |
| Tracked Branch（设 upstream） | 只有 `ensure_push_auto_setup_remote` / `branch.<b>.base`（`worktree_create.rs`） | ❌ 全新命令 |
| Rename… (F2) | 全仓库搜不到任何 `git branch -m` | ❌ 全新命令 |
| 条目 ahead/behind + 已检出标记 | `list_refs_capped` 只回 `refname + oid`（`history.rs:344`） | ⚠️ 扩契约 |

即：**3 个全新 tauri 写命令 + 1 个契约扩展 + 1 个新共享树组件 + 2 个宿主改造**。

### 3.2 已存在、可直接依赖的事实

- `Worktree` 类型是 `{…} & GitWorktreeInfo`（`src/shared/worktree/types.ts:144`，`GitWorktreeInfo` 定义在 22 行，含 `path/head/branch/isBare/locked/prunable/isMain`）—— store 里每个 workspace **本来就带 git 级 `branch`**。
- `ade_git::worktree_list(path) -> Vec<GitWorktreeEntry>` 已存在（`ade-git/src/lib.rs:47`），porcelain 解析在 `porcelain.rs`。
- `window.api.git.refs({ worktreePath })` 已存在（`src/shared/preload-api/api/git-inspection-api.ts:72`）。
- UI 原语齐备：`components/ui/` 下有 `popover.tsx`、`context-menu.tsx`、`dropdown-menu.tsx`、`badge.tsx`、`collapsible.tsx`、`tooltip.tsx`、`command.tsx`。
- `git_refs` 目前**只有一个消费者**：`src/renderer/src/components/git-log/tauri-repository.ts`。换契约成本可控。

### 3.3 Git Log 的真实布局（与旧记录不符，以本 spec 为准）

`GitLogView.tsx:245-268`：`<div flex min-h-0 flex-1>` 内是 **左 `RefPanel`(w-44) + 中 `GraphList`**，
`CommitDetail` 在 268 行作为**底部通栏**渲染 —— 不存在「右详情栏」。
`Toolbar.tsx:29` 的 `{branch}` 只是一个静态 chip，**不是选择器**。

### 3.4 硬约束：i18n

- locale 文件有 **6 个**：`en / es / fr / ja / ko / zh`（`src/renderer/src/i18n/locales/`）。
- **但不是每个 namespace 都要写满 6 份**。已核实的先例：`auto.components.git-log` 只存在于
  `en.json` 与 `zh.json`，es/fr/ja/ko **根本没有这一节** —— 因为
  `translate(key, fallback)`（`i18n.ts:84-87`）用 `defaultValue: fallback` 天然兜底未收录的 key。
  而更老的 `auto.components.right.sidebar.SourceControl` 则 6 份齐全。
  → 新命名空间 `auto.components.git-branch` **只补 en + zh**，跟 git-log 先例一致；
  英文 fallback 写进代码即兼任其余 4 个 locale。
- `src/renderer/src/i18n/no-top-level-translate.test.ts` 是一道 **AST 门禁**：`translate()` 不得在模块顶层调用。
  → `branch-actions.ts` 的 label 必须是 `() => translate(…)` 惰性求值，不能是模块常量。

### 3.5 硬约束：新增 tauri 命令的镜像面

新命令必须同步以下每一面，否则 `specta_export::tests::bindings_are_fresh` 与
`command_inventory_and_bindings_are_fresh` 会红：

1. `ade-git`：实现函数
2. `ade-bridge/src/commands/git.rs`：`*_impl` + `#[tauri::command] pub async fn`
3. `ade-bridge/src/specta_export.rs`：**`collect_commands![]` 注册**（git 面在 120-141 行区间）
4. 再生 `src/bridge/real/generated/tauri-bindings.ts`
5. `src/bridge/real/git.ts`：包装 `invokeCommand('git_xxx', { args })`
6. `src/shared/preload-api/api/git-operation-api.ts`：类型声明（写操作归此文件，非 inspection）
7. mock 面：`src/bridge/mock/git-api.ts` 当前**未定义** `refs/push/pull/fastForward`，靠
   `withMethodFallback` 兜底为 unimplemented。→ 本设计同样**不补 mock 实现**，但渲染测试须自行桩。

### 3.6 远程 runtime 面当前未实现（降低了新命令的成本）

`src/bridge/create-api.ts` 的 `createRealDomains()`（125 行起）**不含** `runtime` / `runtimeEnvironments`；
它们来自 `createMockDomains()` 的 `./mock/runtime-events-api` + `./mock/runtime-environments-api`，
且后者用 `withMethodFallback('runtime', {…})` 包裹。
→ `callRuntimeRpc(target, 'git.pull', …)` 那条远程分支在当前 Tauri 构建里就是 unimplemented。
新增写命令只需做本地 `window.api.git.*` 一条路，处境与现有 push/pull 完全一致，不引入新的不对称。

### 3.7 已知脆弱点（本设计要避开，不要加剧）

`src/shared/git-branch-delete-refusal.ts` 靠**文本匹配** git stderr 判断拒绝原因
（`worktree_remove.rs:260`：`text.contains("branch") && text.contains("is checked out")`）。
git 版本/locale 文案变化即失效。新路径必须在 Rust 侧分类，见 §8。

## 4. 范围

### 4.1 纳入

- 新目录 `src/renderer/src/components/git-branch/`：树组件、树模型、过滤、动作声明表、菜单、取数 hook。
- 改造 `source-control/panel/branch-context-row.tsx`：`HeadIdentity` 由 `span` → `button` + popover。
- 改造 `git-log/GitLogView.tsx` + `git-log/Toolbar.tsx`：删左栏，branch chip → 同一 popover。
- **物理删除** `git-log/RefPanel.tsx` 及其测试断言（`git-log/git-log-view.test.tsx` 里的 `RefPanel search filters…` 用例）。
  `RefPanel` 独占的 locale key（`auto.components.git-log.filterBranches` / `.current` / `.all`）随文件删除一并清理，
  不留孤儿 key；`refsTruncated` 是唯一例外，迁到 `git-branch` 命名空间后复用（见 §7.5）。
- `ade_git::list_refs` 契约扩展（新 `GitRefEntry`）。
- 3 个新写命令 + `delete_branch` 抽取 + `pull/fastForward` 参数化。
- 新命名空间 `auto.components.git-branch` 的 key（**仅 en + zh**，见 §3.4）。

### 4.2 明确不动（防误伤）

- **不引入 `git checkout` / 原地切分支**。
- 不改 `GitHistoryItemRef`（commit 图 decoration 用，与本设计无关）。
- 不改 `git_log_query` / `history_with` 的取数语义。
- 底部历史条 `git-history-panel` 不动（用户此前明确决定保留）。
- 不补 `runtime` / `runtimeEnvironments` 的 real 实现。
- 不为此新增远程 runtime host 的 RPC 方法。
- `worktree_remove` 的既有删除流程语义不变（只是把分支删除逻辑抽成公共函数供复用）。

## 5. 架构与组件边界

```
src/renderer/src/components/git-branch/
  branch-tree-model.ts     纯函数：refs + worktrees + HEAD → 树节点
  branch-tree-filter.ts    纯函数：搜索匹配（子串 + 路径段）
  branch-ref-tree.tsx      纯呈现：折叠/搜索/选中/键盘/行尾槽
  branch-actions.ts        动作声明表（label 惰性 + 可用性判定 + run）
  branch-menu.tsx          右键与 ⋯ 菜单，按宿主取动作子集
  use-branch-refs.ts       取数 + invalidate()
```

每个单元可独立理解与测试：

| 单元 | 做什么 | 怎么用 | 依赖 |
|---|---|---|---|
| `branch-tree-model` | 把扁平 refs 折成 IDEA 形状节点 | `buildBranchTreeNodes({ refs, worktrees, headBranch, topExtraNodes })` | 无 React、无 IO |
| `branch-tree-filter` | 按查询串裁剪节点树 | `filterBranchTree(nodes, query)` | 无 |
| `branch-ref-tree` | 渲染 + 键盘 + 折叠态 | `<BranchRefTree nodes activeKey onActivate renderTrailing …/>` | model/filter |
| `branch-actions` | 声明每个动作的 label/可用性/执行 | `resolveBranchActions(node, ctx)` | 注入 worktree 上下文 |
| `branch-menu` | 把动作渲染成菜单项 | `<BranchMenu node actions …/>` | actions、`ui/context-menu` |
| `use-branch-refs` | 取 refs、暴露 invalidate | `const { refs, truncated, loading, invalidate } = useBranchRefs(worktreePath)` | `window.api.git.refs` |

**树组件不携带任何业务语义** —— 这是「只留一份树」却不产生语义冲突的关键。两个宿主各自注入：

- **宿主 A：Source Control 头部**（`branch-context-row.tsx`）
  `activate` = 切到检出该分支的 workspace；未检出 → 打开 new-workspace-composer 预填 `initialBaseBranch`。
  动作集 = §7.4 矩阵**去掉** `View in Git Log`。
- **宿主 B：Git Log 工具栏 branch chip**（`Toolbar.tsx`）
  `activate` = `applyFilter({ refs: name })`。
  动作集 = §7.4 矩阵**全量**（切分支在 Git Log 里同样合法，不给它开特例）。

  即：**两处菜单项完全一致，只有 `activate` 语义不同**；`View in Git Log` 是唯一按宿主裁剪的项。
  `RefPanel` 原有的 `Current` / `All` 语义不丢：`HEAD (Current Branch)` 天然是树首节点；
  `All Branches` 由宿主 B 经 `topExtraNodes` prop 注入为合成人物 —— 树组件不为它写特例。

## 6. 数据契约

### 6.1 `list_refs` format 扩列

`ade_git::list_refs_capped`（`history.rs:344`）的 format 串由

```
%(refname)%09%(objectname)%09%(*objectname)
```

扩为

```
%(refname)%09%(objectname)%09%(*objectname)%09%(HEAD)%09%(upstream:short)%09%(upstream:track)
```

- `%(HEAD)` → 当前分支标记（`*` 或空）。
- `%(upstream:short)` → upstream 短名。
- `%(upstream:track)` → `[ahead 2, behind 1]` / `[ahead 2]` / `[behind 1]` / 空。
  解析器必须容忍无括号、只有单边、以及 git 不输出该字段（老版本）的情形 → 落 `None`。
- 仍是**一次** `for-each-ref` 调用，不逐分支起进程；2000 上限（`GIT_REFS_MAX_LIMIT`）与 `truncated` 语义不变。
- 继续跳过 `refs/remotes/*/HEAD` symref；annotated tag 继续用 `%(*objectname)` peel。

### 6.2 新 `GitRefEntry`（换型，不做兼容）

```rust
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct GitRefEntry {
    pub name: String,            // 短名：main / origin/main / v1.2.0
    pub full_name: String,       // refs/heads/main
    pub category: GitRefCategory, // LocalBranch | RemoteBranch | Tag
    pub remote: Option<String>,  // origin（RemoteBranch 才有）
    pub oid: String,             // tag 已 peel 到 commit
    pub is_head: bool,
    pub upstream: Option<String>,
    pub ahead: Option<u32>,
    pub behind: Option<u32>,
}
```

`GitRefsResult { refs: Vec<GitRefEntry>, truncated: bool }`。
TS 侧 `src/shared/preload-api/api/git-inspection-api.ts:28` 的 `GitRefsResult` 同步换型，
新增 `GitRefEntry` / `GitRefCategory` 类型。

**为什么不复用 `GitHistoryItemRef`**：该类型同时服务 commit 图上的 decoration（`GitLogView` 的
`references[{name,type}]`），往里塞 `ahead/upstream/remote` 是语义污染。`git_refs` 只有一个消费者且即将重写，
干净换型比兼容叠加便宜，与仓库既有「不做历史兼容」取向一致。

### 6.3 已检出标记在前端算

`branch-tree-model` 从 store 的 `Worktree[]` 构造 `Map<branch, { worktreeId, displayName, hostId }>`。
不进后端的理由：切换目标是 **ade workspace**，只有 store 知道；后端再扫一遍会与实际 git 状态形成第二份真相。

### 6.4 advisory 与 authoritative 的分工（本设计的核心安全边界）

store 元数据**不是权威**：一条分支可能被 ade 不知道的 git worktree 检出（外部 `git worktree add`）。
因此：

- **前端 `worktree_list` 派生标记 = advisory**，只用于置灰与图标，允许漂移。
- **后端每次执行写命令前用 `ade_git::worktree_list` 复核 = authoritative**，检出即返回类型化拒绝
  `BranchOperationRefusal::CheckedOut { worktree_path }`。

`Rename` 不改写 workspace 元数据的论证：既然 Rename 只对**未被任何 worktree 检出**的分支启用，
而 ade 的 `Worktree.branch` 恒等于其 worktree 检出的分支，则该分支**必然不被任何 workspace 引用**，
无元数据可改。若后端复核发现它其实被检出（advisory 漂移），直接拒绝，不做任何元数据写入。

## 7. 交互规格

### 7.1 树形状与图标语义

```
🔍 Branch or tag
   All Branches              ← 仅宿主 B 经 topExtraNodes 注入
   HEAD (Current Branch)     ← 合成人物，activate = 当前分支范围 / 切到当前 workspace
▾ Local
     🏷 main          ↕ 82    ← 🏷 = is_head
       main  ▣ my-app ↕ 82   ← ▣ = 被某 ade workspace 检出，tooltip 给 displayName + path
▾ Remote
  ▾ 📁 origin
       ★ main                ← ★ = 当前分支的 upstream
▾ Tags
     🏷 v1.2.0
```

图标语义是**本设计新定义的映射**（不是「IDEA 就是这样」的断言）：
`🏷`=git 报告的 HEAD 分支；`▣`=被 ade workspace 检出；`★`=当前分支的 upstream。

### 7.2 排序

- 组序固定：`All Branches`（注入项）→ `HEAD (Current Branch)` → `Local` → `Remote` → `Tags`。
- `Local` 内：当前分支置顶，其余按 `committerdate` 倒序需要额外字段 → **本期按名称字典序**（见 §11 开放问题）。
- `Remote` 二级分组按 remote 名排序，remote 内按去掉 `<remote>/` 前缀后的名称字典序。
- `Tags` 按名称字典序（不做版本语义排序）。

### 7.3 键盘

| 键 | 行为 |
|---|---|
| `↑` `↓` | 在**已展开可见**行间移动 |
| `→` | 展开组；已在叶子则无操作 |
| `←` | 折叠组；已在折叠组则跳到父组 |
| `Enter` | 触发宿主 `activate` |
| `F2` | 对选中行触发 `Rename…`（不可用则无操作） |
| `Esc` | 关闭 popover |
| 打字 | 焦点在搜索框（popover 打开即聚焦，对齐 `RefPanel` 与 IDEA） |

### 7.4 菜单可用性矩阵

| 动作 | Local | Remote | Tag | 可用性判定 |
|---|---|---|---|---|
| Switch to this branch | ✅ | — | — | 存在检出该分支的 workspace |
| New Workspace from Branch… | ✅ | ✅ | ✅ | 恒可用（宿主须有 `repoId`） |
| View in Git Log | ✅ | ✅ | ✅ | 仅宿主 B |
| New Branch from… | ✅ | ✅ | ✅ | 恒可用；`start-point` = 该 ref 的 `oid` |
| Show Diff with Working Tree | ✅ | ✅ | — | 非当前分支 |
| Set Upstream To… | ✅ | — | — | 子菜单列 `refs/remotes/*`，同名优先 |
| Update | ✅ | — | — | 被某 workspace 检出 **且** 有 upstream |
| Push… | ✅ | — | — | 被某 workspace 检出 |
| Rename… | ✅ | — | — | **未被任何 worktree 检出**（advisory 置灰 + 后端复核） |
| Delete | ✅ | — | — | **未被任何 worktree 检出**；`-d` 未合并 → 升级 `-D` 二次确认 |

Remote / Tag 节点不出现 `Rename / Delete / Update / Push / Set Upstream`。

### 7.5 空态与截断

- refs 取数失败或空仓：popover 内显示单行空态，**不抛错、不自动关闭**。
- `truncated === true`：沿用 `RefPanel` 现有文案 key
  `auto.components.git-log.refsTruncated`（"Some refs are not shown (list limit reached)."），迁到 `git-branch` 命名空间。

## 8. 错误处理与并发

### 8.1 类型化拒绝，不复用文本匹配

新命令在 Rust 侧分类，回传结构化错误而非字符串猜测（避开 §3.7 的脆弱点）：

```rust
pub enum BranchOperationRefusal {
    CheckedOut   { branch: String, worktree_path: String },
    NotMergeable { branch: String, unmerged_count: u32 },
    NameTaken    { branch: String },
    UpstreamMissing { branch: String },
}
```

经 `BridgeError` 的 typed payload 变体透出。前端按 `kind` 出不同文案与后续动作
（`CheckedOut` → 提示被哪个 workspace/path 检出并给跳转；`NotMergeable` → 弹 `-D` 强确认）。

`git-branch-delete-refusal.ts` 的既有文本匹配**本期不改**（它服务 worktree 删除流程，改动面外溢）；
新路径不再依赖它。

### 8.2 并发：遵守既有的「单飞」约定，而不是幻想一个互斥锁

**已核实**：`beginRemoteOperation` / `endRemoteOperation`（`store/slices/editor/actions/git-remote-status.ts:38-54`）
只是 `remoteOperationDepth` 计数器 + `isRemoteOperationActive` / `inFlightRemoteOpKind` 标志，
**不提供任何互斥**；源码注释明写 "last-write-wins on the kind; the UI blocks a second user-initiated op" ——
串行是**调用方的责任**，不是通道的性质。

因此本模块的每个动作必须：

1. 发起前检查 `isRemoteOperationActive`，为真则**拒绝发起**（菜单项此时也置灰，两者都要做：
   置灰是体验，检查是防 `F2`/快捷键绕过）。
2. `beginRemoteOperation(kind)` 与 `endRemoteOperation()` **必须 `try/finally` 成对**。
   depth 泄漏会让 `isRemoteOperationActive` 永真，整个 Source Control 面板卡死到重启 ——
   这是本设计引入的新动作最容易造成的最严重故障，须有专项测试（见 §11.2）。
3. 复合动作（如 `Update` = fetch + pull + upstream 刷新）依赖 depth 计数天然嵌套，
   沿用 `git-remote-sync.ts` 的现有写法，不自造第二套 in-flight 状态。

in-flight 期间：该行行尾 spinner（对齐 `branch-context-row.tsx` 已有的 `Loader2` 用法）+ 全部写动作菜单项禁用。

### 8.3 成功后刷新分级

| 情形 | 刷新 |
|---|---|
| 目标分支 == 当前 workspace 分支 | `refreshActiveGitStatusAfterMutation` + `invalidate()` |
| 目标分支 != 当前分支 | 仅 `invalidate()`（refs 变了，当前 workspace 状态没变） |
| `Switch`（切 workspace） | 由 `setActiveWorktree` 自身链路负责，本模块不额外刷新 |

### 8.4 失败可见性

写命令失败统一 `toast.error(...)`，沿用 `resolveRemoteOperationErrorMessage` 的既有出口
（`store/slices/editor/actions/git-remote-push-pull.ts` 已用），不新造错误呈现通道。

## 9. 后端新增命令

| 命令 | git | 校验 |
|---|---|---|
| `git_branch_create` | `git branch -- <name> <start-point>` | 名走 `resolve_create_branch_name` 已有的 `check-ref-format --branch` + `-` 前缀拒绝；`NameTaken` 与 `CheckedOut` 复核 |
| `git_branch_set_upstream` | `git branch --set-upstream-to=<upstream> -- <name>` | upstream 必须存在于 `refs/remotes/*`，否则 `UpstreamMissing` |
| `git_branch_rename` | `git branch -m -- <old> <new>` | 执行前 `worktree_list` 复核 → `CheckedOut`；新名走同一校验 |
| `delete_branch`（抽公共，非新 tauri 命令） | `git branch -d/-D -- <name>` | 从 `worktree_remove.rs:147` 抽出，保留 `-d` 失败时区分「未合并」与「被检出」；`worktree_remove` 改为调用它 |
| `pull` / `fastForward` 参数化 | 无新命令 | `RuntimeGitContext` 允许显式传目标 `worktreeId/worktreePath`；**当前 worktree 行为逐字节不变** |

三个新 tauri 命令须走 §3.5 的 7 面镜像清单。

## 10. 增量切分与门禁

三个增量，每个独立可绿可合。逐任务 TDD，每增量末 CodeReview 门禁，收尾全分支审查。

### 增量 1 — 零新写命令（先让「切分支」可用）

- `GitRefEntry` 契约扩展 + format 扩列 + 解析（`ade-git`）
- `branch-tree-model` / `branch-tree-filter` / `branch-ref-tree` / `use-branch-refs`
- 宿主 A：`HeadIdentity` → `button` + popover，动作集只放
  `Switch` / `New Workspace from Branch…` / `Show Diff with Working Tree` / `View in Git Log`
- 宿主 B：删 `RefPanel` 左栏与 `RefPanel.tsx`，`Toolbar` branch chip → 同一 popover
- 新命名空间 `auto.components.git-branch` 的 key（en + zh）

**门禁**：`cargo test -p ade-git -p ade-bridge` + `pnpm typecheck`
+ `pnpm vitest run src/renderer/src/components/git-branch src/renderer/src/components/git-log src/renderer/src/components/right-sidebar/source-control`

### 增量 2 — 非破坏写命令

- `git_branch_create`、`git_branch_set_upstream`、`pull/fastForward` 参数化
- `branch-actions` 补齐 `New Branch from…` / `Set Upstream To…` / `Update` / `Push…`
- `branch-menu` 右键 + `⋯`
- §3.5 全 7 面镜像 + bindings 再生

**门禁**：增量 1 全部 + `cargo test --workspace`（`bindings_are_fresh` /
`command_inventory_and_bindings_are_fresh` 把关）

### 增量 3 — 破坏性

- 抽公共 `ade_git::delete_branch`，`worktree_remove` 改为复用
- `git_branch_rename`
- `BranchOperationRefusal` 透出 + 前端按 kind 分流
- `Delete` 的 `-d`→`-D` 二次确认（文案带分支名 + 未合并提交数；**不加**「输入分支名」式摩擦）
- `F2` → Rename

**门禁**：`cargo test --workspace` + `pnpm typecheck` + 定向 vitest + **全量 `pnpm test`**

> 隔离 worktree：增量 1 仅删 `RefPanel.tsx` 一个文件，判断为**不需要**独立隔离 worktree
> （该惯例针对大规模删除/架构级破坏）。若三个增量都按惯例隔离，实施前说明即可调整。

## 11. 测试策略

### 11.1 Rust

- `ade-git` unit：`%(upstream:track)` 解析矩阵（空 / `[ahead N]` / `[behind N]` / `[ahead N, behind N]` / 畸形）；
  `GitRefCategory` 归类；`remote` 提取；tag peel 不回归。
- `ade-bridge/tests/git_refs.rs`（真 git 仓集成，已有 `branch feature` fixture）：断言 `isHead`、
  `ahead/behind` 精确值、`upstream` 短名、`refs/remotes/*/HEAD` 被跳过、2000 截断仍生效。
- 新命令集成测试：`worktree_lifecycle.rs` 风格真仓，覆盖 `CheckedOut` / `NameTaken` /
  `UpstreamMissing` / `NotMergeable` 四类拒绝各一条，以及 `delete_branch` 抽取后
  `worktree_remove` 既有测试**全绿不改动断言**（抽取必须行为等价）。

### 11.2 TS

- `branch-tree-model.test.ts`（纯函数）：分组/组序/排序/`is_head`/`★`/`▣` 映射/`topExtraNodes` 注入。
- `branch-tree-filter.test.ts`：子串、路径段（`feature/login` 命中 `log`）、大小写。
- `branch-ref-tree.test.tsx`：折叠/展开、键盘矩阵、行尾槽渲染、空态、`truncated` 提示。
  受控 input 须用原生 value setter 才触发 `onChange`（既有陷阱）。
- `branch-actions.test.ts`：可用性矩阵逐格（含 Remote/Tag 不出现破坏件）；label 惰性求值。
- 宿主 A 集成：点 `HeadIdentity` → popover → `Switch` 调 `setActiveWorktree`；未检出分支 →
  `openModal('new-workspace-composer', { initialBaseBranch })`。
- 宿主 B 集成：Git Log popover 选分支 → `applyFilter({ refs })`；`All Branches` → `refs: 'all'`。
- **depth 泄漏专项测试**：让写命令 reject，断言 `endRemoteOperation` 仍被调用、
  `isRemoteOperationActive` 回 `false`、`remoteOperationDepth` 回 `0`（§8.2 第 2 条的唯一防线）。
- 单飞约定：`isRemoteOperationActive === true` 时发起第二个动作被拒（含经 `F2` 绕过的路径）。
- 时序类断言（in-flight 禁用 → 完成恢复）用 deferred 拆两次 `act`，并验证能复现旧 bug。
- 删除 `git-log-view.test.tsx` 里的 `RefPanel` 用例，其过滤覆盖迁到 `branch-tree-filter.test.ts`。

### 11.3 不做

- 不做 `RefPanel` 的兼容层或旧 key 保留。
- 不为远程 runtime host 补 RPC 方法或测试。

## 12. 开放问题（实施前须定，不阻塞 spec review）

1. **`Local` 按提交时间排序**：需 `for-each-ref` 再加 `%(committerdate)` 一列（仍是同一次调用）。
   本期先按字典序；若你要 IDEA 的时间序，增量 1 内加一列即可，成本极低。
2. **`Update` 的 merge/rebase 偏好**：现有 `pullRuntimeGit` 走 git 默认（`pull.ff` 等 config）。
   IDEA 会问「Merge / Rebase / Checkout」。本期**不问**、按 git 默认；若要问，属增量 2 的独立任务。
3. **`Set Upstream To…` 子菜单在 remote ref 很多时的裁剪**：是否只列同名 remote 的分支（IDEA 行为）
   还是全量 `refs/remotes/*`。本期先同名优先 + 全量可滚动，实施时按真实仓库宽度定。
