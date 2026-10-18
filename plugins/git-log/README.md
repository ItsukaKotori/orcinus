# Git Log 插件（增量 1 fixture UI → 增量 2a 真实 HEAD 数据）

IDEA VcsLog 风格的独立主区提交图谱，以**插件**形态交付。增量 1 先建 fixture 驱动 UI（可与宿主同屏对照迭代）；增量 2a 已接**真实 HEAD 历史**（新增只读 `git:read` capability + host `git.log`/`git.commitFiles`，复用 Rust `ade-git`）。数据源详见下方「数据源」。

## 本地加载（tauri dev，真实桥）
1. `pnpm build:git-log`（产出 `dist/tab.html`，全内联单文件；`dist` 不入库）。
2. `pnpm dev`（**须真实桥**：mock 桥用固定 fixture，不反映本插件）。
3. 设置 → 插件：**先打开「插件系统」总开关**（`pluginSystemEnabled`，默认 `false`；不开则面板/tab 一律返回 `disabled_system`），再配 `devPluginPaths` 指向 `plugins/git-log`。
4. 插件中心启用本插件（`enabledPlugins`，与上面的总开关是两个维度）；consent = `workspace:open-tab` + `git:read`。
5. 右侧栏出现 "Git Log" 启动器 → 点击打开主区 Git Log tab。
6. 改 UI 后重跑 `pnpm build:git-log`，dev watcher 会重新发现热重载。

## 结构
- `src/ui/*` — React 主视图（工具条 / refs 面板 / 虚拟图谱列表 / 详情）。
- `src/graph/git-graph-model.ts` — 只读复用 `src/shared/git-history-graph.ts` 泳道布局。
- `src/fixtures/*` — 确定性 fixture（≥30 提交，含 merge/fork/tag/remote/ahead-behind）。
- `src/domain/types.ts` — `GitLogRepository` 数据缝；`ChannelRepository` 为增量 2 类型占位。
- `launcher.html` — 手写内联启动器，调 `workspace.openTab`。

## 数据源
- **增量 2a（已落）**：`HostRepository` 经 host `git.log`/`git.commitFiles`（新增只读 `git:read` capability；Rust 侧 `ade-bridge` 解析 active worktree 路径→调 `ade_git`），tab 显示**真实 HEAD 历史**。
- **增量 2b（后续）**：扩 `ade_git` 支持 `--all`/author·date·path 过滤/skip 分页；补 Date/Path filter chips、分支选择器接真 ref。
- `FixtureRepository` 保留给 `git-log-view.test.tsx`（测试）；运行时 `tab-main` 固定 `HostRepository`。
