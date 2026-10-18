# Git Log 插件（增量 1：fixture UI）

IDEA VcsLog 风格的独立主区提交图谱，以**插件**形态交付。增量 1 为 **fixture 驱动**：零后端、零插件能力表改动，数据来自内嵌 fixture，可与宿主同屏对照迭代 UI。

## 本地加载（tauri dev，真实桥）
1. `pnpm build:git-log`（产出 `dist/tab.html`，全内联单文件；`dist` 不入库）。
2. `pnpm dev`（**须真实桥**：mock 桥用固定 fixture，不反映本插件）。
3. 设置 → 插件 → 开发：`devPluginPaths` 指向 `plugins/git-log`。
4. 插件中心启用；consent 仅 `workspace:open-tab`。
5. 右侧栏出现 "Git Log" 启动器 → 点击打开主区 Git Log tab。
6. 改 UI 后重跑 `pnpm build:git-log`，dev watcher 会重新发现热重载。

## 结构
- `src/ui/*` — React 主视图（工具条 / refs 面板 / 虚拟图谱列表 / 详情）。
- `src/graph/git-graph-model.ts` — 只读复用 `src/shared/git-history-graph.ts` 泳道布局。
- `src/fixtures/*` — 确定性 fixture（≥30 提交，含 merge/fork/tag/remote/ahead-behind）。
- `src/domain/types.ts` — `GitLogRepository` 数据缝；`ChannelRepository` 为增量 2 类型占位。
- `launcher.html` — 手写内联启动器，调 `workspace.openTab`。

## 增量 2（后续，待 phase3c 通道入 main）
`ChannelRepository` 经 worker 通道 `spawn git log` 取真实数据；补齐过滤/分页/Date·Path chips；把异步加载补 try/catch 与加载/错误态。本增量不声明 `worker:call`、不接真实数据。
