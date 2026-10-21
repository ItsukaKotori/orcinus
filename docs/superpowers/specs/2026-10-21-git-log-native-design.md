# Git Log 原生视图（增量 3）设计

日期：2026-10-21。分支 `phase5-git-log-native`（自 main `eda81f4f`，含增量 1–2e）。
前置：git-log 插件（增量 1–2e，沙箱 iframe、只读）；主应用已有 Source Control（右侧栏：commit/listing/sync/panel）。

## 1. 背景与目标

用户反馈 git-log 插件与 IDEA 的 Git 工具窗差距大：无提交面板、文件变更看不了 diff、位置（侧栏插件）对「项目级 git 工具」没意义、左侧分支区弱。核查发现：**主应用 Source Control 已有提交/diff/写操作全套**（`git_commit/git_stage/git_push/git_diff/git_commit_diff` + `commit-area` + `listing` 打开 diff + `sync/git-history-panel` 底部历史条），插件是这些能力的**弱化重做**。

**目标**：把 git-log 插件独有价值（完整 Log 布局、分支范围、后端过滤、offset 分页）迁入**主应用原生 Git Log 视图**，落在**主编辑器区全宽 tab**；提交/diff/写操作复用主应用现成能力；迁移达平后**删除插件**。

**非目标**：不改主应用既有提交面板/变更树/diff 编辑器；不做交互式 rebase、blame、GitHub PR（已有别处）；不在本轮加新写操作（stage/commit/push 已存在）。

## 2. 关键决策

- **落点 = 主编辑器区全宽 tab**（新增 `TabContentType: 'git-log'`）。右侧栏默认 400px（最小 220、可拖但会把主编辑器挤到 320 下限），承载不了「左分支树 + 中图谱 + 右详情/diff」三栏，故不放右侧栏。
- **Log 与 Commit 分工**：Git Log = 主区 tab（浏览历史/看 diff/选范围/过滤）；Commit/变更 = 右侧栏 Source Control（已全套）。对应 IDEA 的 Log 工具窗 + Commit 工具窗。
- **复用后端**：`ade_git::history_with`/`list_refs`（2b–2e 已建、已测）包成新 tauri 命令给主应用；不新造 git 逻辑。
- **组件迁移**：插件的 React 组件（仅依赖 React + `@shared`）搬进主应用，数据层从 `callHost`（iframe 桥）换成直连 tauri 命令。
- **取代底部历史条**：新 Log 视图成为历史入口；`sync/git-history-panel` 底部条是否移除见 §6。

## 3. 架构

### 3.1 后端命令（`ade-bridge/commands/git.rs`）
- `git_log_query(worktree_id, query)` → `ade_git::history::history_with`；`query` 结构 `{refs: 'head'|'all'|string|null, author?, since?, until?, grep?, path?, skip?, limit?}`（复用 2b 的 `GitHistoryQuery` 语义 + 注入防护：rev 先 `resolve_commit`、path `--` 分隔拒 `-`、日期形状由前端保证或命令内校验）。返回 `GitHistoryResult`（camelCase）。
- `git_refs(worktree_id)` → `ade_git::history::list_refs`，返回 `GitRefsResult{refs, truncated}`。
- 复用现有 `git_commit_diff`（提交内文件 diff）、`git_commit_compare`（提交文件列表）。
- 命令走既有 worktree 授权/路径校验（`require_authorized_worktree`、`is_allowed_git_read_args`）。
- 注册进 tauri `invoke_handler`；前端 `window.api.git.*` 增对应封装（沿用现有 git api 命名/错误约定）。

### 3.2 tab 扩展（`src/shared/tab-types.ts` + tabs store + 渲染）
- `TabContentType` 加 `'git-log'`；`toVisibleTabType` 归入 `'editor'`（编辑器族，参与 MRU/预览）。
- `Tab` 加可选 `gitLog?: { worktreeId: string; initialRefs?: string }` 绑定（类比 `pluginTab`），承载去重/水合身份（同 worktree 复用同一 Log tab）。
- 新增 open action `openGitLogTab(worktreeId)`（类比 `openPluginTab`）+ 渲染分派：主区 tab 内容按 `contentType==='git-log'` 渲染 `<GitLogView/>`。

### 3.3 前端视图（`components/git-log/`，主应用原生）
三栏 + 顶栏，迁移插件 `Toolbar/RefPanel/GraphList/CommitDetail`：
- 顶栏：range 选择（Current/All/分支）、过滤 chips（author/date/path/grep）、客户端 search（subject 即时）、refresh。
- 左：分支/范围树（分组 branches/remote branches/tags + 搜索 + 选为范围），迁移并增强 `RefPanel`（IDEA 式）。
- 中：图谱 + 提交列表（虚拟化 + offset 无限滚动），迁移 `GraphList`（已复用主应用 `GitHistoryGraphSvg`）。
- 右/下：提交详情（message + 变更文件树）；**双击文件 → 调 `git_commit_diff` 经主应用 diff 编辑器打开**（复用 `listing` 现有 diff-open 路径）。
- 数据层：`GitLogRepository` 接口的实现从「callHost」改为「tauri 命令」（`window.api.git.logQuery/refs/commitFiles`）；`query.ts`（logParams/mergePages/PAGE_SIZE）直接搬。

### 3.4 diff 打开
复用主应用既有能力：Source Control `listing/use-row-opening` + `split-open` 已把「文件 → diff tab」打通；Git Log 的提交文件双击走同一 open-diff 动作（传 commit 上下文 → `git_commit_diff`）。不新造 diff UI。

### 3.5 入口
- 右侧栏 Source Control 头部/工具栏加「Git Log」按钮 → `openGitLogTab(activeWorktreeId)`。
- 活动栏（`activity-bar-buttons`）可选加入口（与 Source Control 同区，避免左侧再立一个 git 图标；默认只在 Source Control 暴露）。
- 命令面板动作 `git.log.open`。

### 3.6 与底部历史条
`sync/git-history-panel`（右侧栏底部可折叠条）与新全宽 Log 功能重叠。**决策**：保留底部条作「快速历史」，但加「在 Git Log 中打开」跳转到全宽视图；不在本轮删除底部条（避免牵动 Source Control 现有测试/交互）。若后续确认冗余，另计。

## 4. 数据契约
- 复用 `GitHistoryResult`/`GitHistoryItem`/`GitHistoryItemRef`/`GitRefsResult`（camelCase serde，`@shared/git-history*`）。
- `git_log_query` 的 query 字段名与前端 `logParams` 逐字一致（refs/author/since/until/grep/path/skip/limit）。
- 日期：前端 `<input type=date>` 产 `YYYY-MM-DD`；命令侧对 since/until 做形状校验（复用 2c/2e 的 `is_date_prefix` 逻辑，或前端 zod）。

## 5. 错误/空态/大仓
- 非 git 工作区 / 未授权 worktree：视图内联提示（复用主应用 Source Control 的 not-a-repo 文案），不整屏崩。
- refs 截断：`truncated=true` 时分支树顶部提示（迁移插件文案）。
- 分页失败：保留已加载 + 重试（迁移插件策略）。
- 大仓：PAGE_SIZE=100、refs 上限 2000（已建）。

## 6. i18n
新 UI 文案走主应用 `translate('auto.components.git-log.*', '默认英文')` + 各 locale（en/zh/ja/ko/fr/es）补键；沿用仓库现有 i18n 约定。

## 7. 插件退役（迁移达平后，3d）
- 删 `plugins/git-log/`（含 dist/launcher/manifest/测试）、`package.json` 的 `build:git-log` 脚本与 typecheck 条目、相关 docs。
- host 方法 `git.log/git.refs/git.commitFiles` + `git:read` capability：**确认无其它调用方后**移除（`plugin-host-api.ts`、`ade-plugins/host/api.rs` SPECS、`ade-bridge` 实现、gate/methods 测试、`PluginPanelActionErrorCode` 中 git 专属码若仅服务这些方法则保留通用码）。`ade_git::history_with/list_refs` **保留**（改由 §3.1 命令使用）。
- 退役单独成步，先确认主应用视图达平再删，避免能力真空。

## 8. 测试与门禁
- 后端：`git_log_query`/`git_refs` 命令单测（range/过滤/分页/refs 截断/非 git 授权）；ade-git 已有 `history_with/list_refs` 测试复用。
- 前端：Git Log 视图渲染/过滤/分页/选范围/diff-open 单测（node/happy-dom，遵循「虚拟列表抽纯函数」惯例）；tab 扩展 open/去重测试。
- 门禁：`pnpm build:orcinus-cli && cargo test --workspace`、`pnpm typecheck`、`pnpm build:web`、`pnpm test`（新视图 + 受影响面）。
- 手工：全宽 Log 三栏、双击文件出 diff、range/过滤/分页、Source Control 入口。

## 9. 分阶段
- **3a** 后端命令 `git_log_query`/`git_refs`（+ 前端 api 封装 + 测试）。
- **3b** tab 扩展 `'git-log'` + Git Log 视图骨架（三栏 + 图谱/列表/详情 + diff-open + 数据层直连命令），只读跑通。
- **3c** 分支树 + 过滤 chips + offset 分页对齐 IDEA（迁移插件逻辑）。
- **3d** 入口（Source Control/命令面板）+ i18n + 退役插件（§7）。

## 10. 开放点/假设
- 假设主应用 diff-open 路径可被 Git Log 复用（3b 前置核实 `use-row-opening`/`split-open` 的入参形态）。
- `toVisibleTabType('git-log')→'editor'` 是否符合预期（Ctrl+Tab 分组）；若需独立可见类型另议。
- 底部历史条保留（§3.6）；若你要直接替换，3b 调整。
