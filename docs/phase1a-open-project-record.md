# Phase 1 子项目 A：打开项目纵切 收尾记录

- 分支：`phase1a-open-project`（base `main@d23f696`）；本批次全程未 push、未 merge，合并/PR/保留由用户在合并前决定
- 日期：2026-09-24（macOS 26.6.2 darwin-arm64；Node v22.21.0，pnpm 12.4.2）
- 计划与 spec：`docs/superpowers/plans/2026-09-23-phase1a-open-project.md`；`docs/superpowers/specs/2026-09-23-phase1a-open-project-design.md`
- 证据目录（gitignored，不随仓库分发）：`.superpowers/sdd/2026-09-23-phase1a-open-project/`
  - 终局全量测试：`final-test.log`（exit 0）
  - GUI 启动冒烟：`dev-smoke.log`（真实模式）、`dev-smoke-mock.log`（`VITE_ADE_BRIDGE=mock`）
  - 实现台账：`progress.md`；任务报告：`task-0-report.md` … `task-11-report.md`

## 1. 批次与提交序（Task 0–12）

| Task | 提交 | 主题 |
|---|---|---|
| 0 | （分支建于 `d23f696`，无代码提交） | 隔离分支与基线核验；crates.io 可达 |
| 0 | `c01bb4c` | 控制器修正计划（`cargo test` 覆盖整个 workspace，Ruling 7） |
| 1 | `7705338` | `ade-core` 骨架（id/路径归一/错误/默认值生成） |
| 1 | `3c9009f` | 补强默认值替换与平台分支回归测试（审查修复） |
| 2 | `9254c70` | JSON 原子持久化与三存储（settings/ui/projects） |
| 2 | `8ddb75c` | 裁定 `schemaVersion` 口径与写盘节流归属（审查，Ruling 8/9） |
| 3 | `93af8c1` | 最小 git CLI 封装与 worktree porcelain 解析 |
| 4 | `72cf649` | 路径授权、读写与回收站删除 |
| 4 | `6bd1937` | rename 身份校验、比较器大小写与授权硬化（审查修复） |
| 5 | `d702257` | `listFiles` 与 markdown 文档遍历（ignore 语义） |
| 5 | `673243b` | 取消接线测试与目录 symlink 过滤（审查修复） |
| 6 | `3bb4e1c` | 内嵌搜索（rg 语义对齐） |
| 6 | `9ef48ef` | 恢复全局 gitignore 语义并统一 UTF-16 显示偏移（审查修复） |
| 7 | `50dcf86` | 文件监听（聚合/引用计数/overflow 语义） |
| 7 | `b83ffef` | 收窄监听删除降级并前置忽略过滤（审查修复） |
| 8 | `bcb5a4c` | 计划补全 `fs` 命令归属（Ruling 10） |
| 8 | `068e88a` | Tauri 命令骨架、bootstrap 注入与 settings/ui/platform 域 |
| 8 | `82f8429` | 快照同步、watch 阻塞化与启动授权（审查修复） |
| 9 | `4352ce5` | repos 注册表与最小 worktree 投影 |
| 9 | `d8e7556` | worktree 投影作用域、字段清除与去重（审查修复） |
| 10 | `b64fd29` | projectGroups 与 folderWorkspaces 注册表 |
| 11 | `a1ebc74` | 真实 IPC 适配层与按域组装（含 mock 回退） |
| 11 | `301f078` | `ui_set_with_ack` 与契约测试（审查修复） |
| 12 | 本提交 | 全绿门禁、手工验收（自动化部分）与收尾记录 |

截至 Task 11：分支 diff 93 文件，+21925/−54（`git diff --shortstat main...301f078`）。Task 1–11 均经独立审查，共 **19 份** review 包（`review-*.diff` 存于证据目录，`review-c01bb4c..7705338.diff` … `review-a1ebc74..301f078.diff`）；Task 0 为纯 setup、按 Ruling 6 不派任务审查；本记录提交时 Task 12 审查尚未开始（其后生成 `review-301f078..cec2ab4.diff`）。fix round 提交即上表「审查修复」行。

## 2. Crate 与命令面

### 2.1 Rust crate

| crate | 内容 | 测试 |
|---|---|---|
| `ade-core` | id 生成、路径归一/比较、错误类型、默认值（由 TS 生成 JSON 内嵌）、repo/group/folder 模型 | 44 |
| `ade-store` | JSON 原子持久化（tmp+rename+轮换）与 settings/ui/projects 三存储、合并、只读键、main-owned 写 | 44 |
| `ade-git` | 最小 git CLI（`--version`、`rev-parse`、`worktree list --porcelain -z`，1.5s 预算） | 13 |
| `ade-fs` | 授权 `FsService`、读写/rename/copy/trash、`listFiles`、`search`、`FsWatcher` | 121 |
| `ade-bridge` | 51 命令 + 事件、bootstrap、错误映射 `{message}`、写调度（1000/5000ms）、bindings 导出/新鲜度 | 110 |
| `orcinus-app` | 启动引导（init script 注入 + 主窗口）、`invoke_handler` 接线（改造既有 crate） | 0（接线） |
| `orcinus-pty` | 未改动 | 3 |

### 2.2 命令面（51，`specta_export::bridge_builder()` 单一登记点）

| 域 | 数 | 命令 |
|---|---|---|
| fs | 18 | `read_dir read_file write_file create_file create_dir rename copy delete_path stat path_exists paths_exist list_files cancel_list_files search watch_worktree unwatch_worktree list_markdown_documents authorize_external_path` |
| repos | 10 | `list add update remove reorder_for_host pick_folder pick_folders pick_directory is_git_available get_default_create_project_parent` |
| project_groups | 8 | `list create update delete move_project scan_nested cancel_nested_scan import_nested` |
| folder_workspaces | 5 | `list create update delete get_path_status` |
| ui | 4 | `get set set_with_ack record_feature_interaction` |
| settings | 2 | `get set` |
| worktrees | 2 | `list list_all` |
| platform / app | 1 / 1 | `get` / `get_identity` |

- 事件：`settings:changed`（变更键 + 新值）、`ui:stateChanged`（完整对象 + 发起者）、`fs:changed`（FsChangedPayload）。
- TS 侧：`src/bridge/real/*` 承接 10 域（fs/repos/projects(投影)/projectGroups/folderWorkspaces/settings/ui/worktrees/platform/app），其余 29 个命名空间维持 Phase 0 mock 的响亮未实现；`mode` 默认 `real`，`VITE_ADE_BRIDGE=mock` 全量回退。生成 bindings checked-in（`src/bridge/real/generated/tauri-bindings.ts`），`bindings_are_fresh` cargo 测试防漂移。

## 3. 对齐语义与偏差

### 3.1 spec §10 四条调整（已按此实现）

1. **最小 git CLI**：为与 oracle `git worktree list --porcelain -z` 1:1（多 worktree、完整 ref、输出序、`isMainWorktree` 次序），引入 `ade-git` 而非读 `.git/HEAD`；代价是依赖系统 git（`isGitAvailable` 已有前提）。
2. **specta 范围**：Rust 负载类型派生并生成 bindings 供参考/新鲜度锁定；桥接适配层以既有 TS 契约为准（迁移期 TS 是契约源），避免同形状双源。
3. **默认值源**：以 TS 为源生成 Rust 内嵌 JSON（非 Rust 手写 198 字段）；与上游 spec §6.4「默认值固化进 ade-core」为有意偏差（单源、防漂移）。
4. **projects 投影留 TS**：Rust 无 projects 命令，桥接层用 `repos_list` + shared 投影函数组装；`projects.update` 仅回显 `localWindowsRuntimePreference`、不落盘（Windows 后置项）。

### 3.2 已知近似与语义注记

| 项 | 现状 | 位置/测试 |
|---|---|---|
| 比较器标点近似 | 大小写 tie 已对齐 TS（小写在前）；标点权重（如 `a_b` vs `a-b`）为码位序近似，与 `Intl.Collator('en')` 不完全一致；渲染端 `sortDirEntries` 重排兜底 | spec §5.1 readDir 注；`ade-fs` 比较器测试 |
| 搜索每文件上限语义 | 按「匹配条目」100 计，而非 `rg --max-count` 的「行」100；单行大量命中比 oracle 少报（更保守） | `PER_FILE_MAX_MATCHES`；Task 6 report |
| FsWatcher macOS 注记 | FSEvents 会把 delete 与 create/modify 位合并，flush 时对 create/update 做 stat 探测、`NotFound` 降级 delete（对齐 parcel）；FSEvents 上报 canonical 路径（`/private/var/...`）不重写；忽略过滤前置于 notify 回调，`node_modules` 风暴不打爆 5000 overflow 上限 | Task 7 report；`ade-fs/tests/watch.rs` |
| `ui.setWithAck` 失败语义 | 写盘失败 reject 且不广播 `ui:stateChanged`（内存已合并；后续变更会再次触发写与广播）；flush 在命令线程同步阻塞；调度器 `last_error` 为全局 | `ade-bridge` 失败注入测试；Task 11 report |
| `workspaceCleanup` 合并深度 | 实现按绑定 spec §5.4 深合并 `workspaceCleanup`（`ade-store` `DEEP_MERGE_KEYS`）；oracle 为顶层浅合并 `{ ...current, ...incoming }`（`orca/src/shared/workspace-cleanup-ui-state.ts` 的 `mergeWorkspaceCleanupUIState`，调用点 `orca/src/main/persistence/applying-settings/ui-state-update.ts:101`）。spec 为准，差异记录 | `ade-store` `set_deep_merges_workspace_cleanup` |
| `projects.update` 不落盘 | 仅回显 Windows 运行偏好字段（spec §10.4） | `src/bridge/real/projects.ts` |
| 项目默认父目录 ≠ `workspaceDir` | `workspaceDir` 默认仍为 `{{HOME}}/orca/workspaces`（TS 默认源 `src/shared/constants.ts`），而 `repos_get_default_create_project_parent` 在未改默认时返回 `{{HOME}}/orcinus/projects`（Orcinus 品牌，spec §10.3）。二者有意不同且用户可见：新建项目默认落在 `orcinus/projects`，工作区仍在 `orca/workspaces` | `ade-bridge` `default_parent_*` 测试；`src/bridge/real/repos.ts` |
| `platform.osRelease` 前缀 | `uname -sr` 结果带 `Darwin ` 前缀；Windows osRelease 为空字符串；arch 为 Rust 词表 | Task 8 report；`ade-bridge` platform 测试 |
| `app.getIdentity` 超集 | 契约 7 字段外多一个 `version`（`CARGO_PKG_VERSION`），TS 结构类型无害 | `app.test.ts` 以 `toMatchObject` 断言 |
| `schemaVersion` 口径 | 仅 `projects.json` 顶层；settings/ui 为纯领域对象（Ruling 8） | spec §4.1 |
| 写盘节流归属 | 1000/5000ms 节流与 main-owned 写路径在 `ade-bridge` 写调度层，`ade-store` 保持同步原语（Ruling 9） | `ade-bridge` scheduler 测试 |

## 4. 测试与门禁证据（Task 12 终局）

### 4.1 Rust

`cargo test --manifest-path src-tauri/Cargo.toml --workspace`：**335 passed / 0 failed**（19 个测试目标 + 7 个 doctest，均 0 failed）。

| 目标 | 通过 | 目标 | 通过 |
|---|---|---|---|
| ade-bridge（unit/catalogs/repos） | 85+12+13=110 | ade-fs（unit/fs_basic/list_files/search/watch） | 60+23+9+24+5=121 |
| ade-core | 44 | ade-git（unit/cli_integration） | 8+5=13 |
| ade-store | 44 | orcinus-pty（unit/throughput） | 2+1=3 |

### 4.2 前端冷门禁

`rm -f tsconfig.tsbuildinfo && pnpm typecheck && pnpm build:web` → exit 0；`tsc --noEmit` 无输出；`vite build ✓ built in 3.22s`，仅既有 chunk-size 警告。

### 4.3 全量前端测试

`pnpm test`（`final-test.log`）：**3839 passed | 8 skipped (3847 files)；34086 passed | 122 skipped (34208 tests)；0 failed**，388.11s；`grep -cE '^\s*FAIL'` = 0。

- 较 Phase 1 债务清偿收尾（3825 文件 / 33929 通过）：**+14 个测试文件、+157 个通过测试**。
- `set: --: invalid option` 为仓库既有测试噪声（Task 11 已记录），非失败；8 个 skipped 文件为既有条件跳过（非本批次隔离）。
- `src/bridge` 契约/parity 测试 198 pass（命令名、`{args}` 包裹、`{message}`→`Error`、事件退订、real/mock parity、未实现对同拒）。

## 5. 手工验收结果（自动化部分）与待用户复核

### 5.1 启动冒烟（本任务执行）

真实模式（`dev-smoke.log`，日志逐字）：

```
ROLLDOWN-VITE v7.3.1  ready in 169 ms
➜  Local:   http://127.0.0.1:1420/
Finished `dev` profile [unoptimized + debuginfo] target(s) in 13.52s
Running `target/debug/orcinus-app`
```

- 进程存活为 Foreground app，WebKit Networking XPC 已拉起（WebView 初始化）；CoreGraphics 窗口列表：`owner=orcinus-app layer=0 name=Orcinus bounds 1440×809`（宽度与 builder 1440 一致）。
- 启动后日志 **0 error / 0 panic**；终止后端口 1420 释放、无残留进程（已核对）。

Mock 回退模式（`dev-smoke-mock.log`）：

```
ROLLDOWN-VITE v7.3.1  ready in 196 ms
➜  Local:   http://127.0.0.1:1420/
Running `target/debug/orcinus-app`
```

- 窗口同样存在（CGWindowList matching=1），0 error / 0 panic；证明 mock 模式下应用可正常启动（Ruling 5：不做 OS 级 UI 自动化）。

### 5.2 9 项验收清单

| # | 项目 | 自动化证据（本次） | 结论 |
|---|---|---|---|
| 1 | pickFolder 加本地 git 仓库（侧栏出现项目/主工作树）+ folder kind | repos 注册表/投影测试、`repos_add` 授权与去重测试 | **待用户复核**（交互） |
| 2 | 新建项目分组并移入；新建文件夹工作区 | projectGroups/folderWorkspaces 注册表测试 | **待用户复核** |
| 3 | 文件树展开、Monaco 打开、编辑保存；外部 `git status` 确已修改 | fs write/rename 测试、`fs_write_file` 落盘测试；未做端到端 UI | **待用户复核** |
| 4 | 外部编辑同一文件 → 文件树/编辑器刷新 | `fs:changed` 广播 + watch 集成测试（真实 FSEvents） | **待用户复核** |
| 5 | 全局搜索（include/exclude/正则）与 quick open 命中真实文件 | `ade-fs` search 24 项集成 + 桥接契约测试 | **待用户复核** |
| 6 | 设置改动的 `settings.json`；布局改动的 `ui-state.json` | store 持久化 + 节流调度测试 | **待用户复核** |
| 7 | 重启后项目/分组/文件夹工作区/选中与布局恢复 | 启动加载 + 持久化 repo 预授权测试 | **待用户复核** |
| 8 | `authorizeExternalPath` 之外路径读取被拒（文案正确） | `PathAccessDenied` `{message}` 单测/集成 | **待用户复核** |
| 9 | `VITE_ADE_BRIDGE=mock pnpm dev` 回退 mock 行为正常 | `create-api.test.ts`（默认 real / env mock / 显式 mode）+ 本次 mock 启动冒烟 | **启动已验证；交互行为待用户复核** |

补充待复核项（终审记录，用户交互验收时执行）：

- 大 payload `fs.read_file` 经真实 Tauri IPC 的往返：文本至 ~50 MiB、图片 base64（大文件/大图在 WebView 与命令线程间的序列化/内存表现）；自动化目前只覆盖小文件与契约层。

## 6. 延后项

### 6.1 子项目边界内显式延后（spec §2.2/§2.3）

| 项 | 内容 | 跟踪 |
|---|---|---|
| 子项目 B | git 面板与 status/diff/commit、真实 worktree 创建/切换/删除 | spec §2.3；`repos.clone/create*` 等保持响亮未实现 |
| 子项目 C | PTY/agent | spec §2.3；`orcinus-pty` 未接线 |
| 下载/日志尾随/拖入导入 | `fs.download*`（7）、`readLocalLogTail/startLocalLogTail/stopLocalLogTail/on*`、`importExternalPaths/stageExternalPathsForRuntimeUpload/resolveDroppedPathsForAgent` | spec §2.2 |
| 远端/host-setup/托盘/跨端 | `projects.createHostSetup/setupExistingFolder/...`、repos 远端命令、ui 托盘事件 | spec §2.2 |
| Windows | `notify`/路径大小写/`git worktree` 输出差异、`projects.update` 落盘、osRelease、Windows 全量测试与启动 | spec §8.6 |
| mock 残渣清理 | 29 个未接真命名空间仍 mock；mock 加固遗留（Phase 0 backlog 第 3 项） | spec §9；Phase 4 |

### 6.2 逐任务审查 minor（汇总）

| Task | 主要延后项 |
|---|---|
| 1 | `path_compare` 未覆盖尾斜杠/`//`折叠/WSL UNC 大小写 |
| 2 | 内存态先于落盘、失败不回滚需文档化；只读键剥离依赖写路径（Task 8 已接） |
| 3 | 非零退出统一映射 `NotAGitRepository`（丢 stderr）；stdin 未置 null；僵尸/超时边界未测 |
| 4 | 错误消息前缀与 oracle 不同；revoke 后 stale canonical；mtime 负值钳 0 |
| 5 | 单段 exclude 前缀过剪（bug-for-bug parity）；markdown 排序/无上限 |
| 6 | 取消集成测试理论时序依赖；空查询回退长度 1 |
| 7 | install 在锁内可阻塞他 root；30s 宽限线程不可取消；每 flush 8 线程较重 |
| 8 | `getSync` 不随 `settings:changed` 刷新（单窗口暂可）；`subscribeToEvent` 静默吞 listen 失败；`{message}` 对象非 Error（Task 11 已映射） |
| 9 | git 失败 fail-fast vs oracle 降级 `[]`；未知 kind 反序列化失败；registry fsync 与写调度不一致 |
| 10 | import 在 async runtime + 锁内跑 git 子进程；`next_tab_order` 极端值回绕；`is_ignored_nested_repo_directory` 空段潜在 panic |
| 11 | parity 矩阵缺 projectGroups/folderWorkspaces 深形状；`toRendererError` 非字符串 message 有损 |
| 12 | （本次）无新增 minor |

完整条目与理由见证据目录 `progress.md` 与各任务报告。

**子项目 B 对接需知（从仅存于 gitignored 任务报告的偏差升格，一行一项）：**

- **T9 worktree git 失败**：`worktrees_list/list_all` fail-fast（reject），oracle 降级返回 `[]`——B 接入前需决定保持或改降级。
- **T6 每文件搜索上限**：按匹配条目计 100（`PER_FILE_MAX_MATCHES`），rg `--max-count` 按行计；单行多命中会少报。
- **T7 macOS FSEvents**：delete 位与 create/modify 合并时按 stat 探测降级 delete；事件路径为 canonical 路径（如 `/private/var/...`）不重写。
- **T10 import/NestedRepo**：`project_groups_import_nested` 在 async runtime 与 projects 锁内跑 git 子进程（阻塞 IO，有界非死锁）；zod 校验存在宽松点（`repoIcon` 等未深校验、空 id 口径不一致）。

## 7. 收尾状态

- 门禁：cargo 335/0、冷 `typecheck`+`build:web` exit 0、`pnpm test` 0 failed（3839 文件 / 34086 测试）、启动冒烟（真实 + mock）通过。
- 本提交后 `git status` clean；分支未 push、未 merge。
- 交付选项（用户决定）：合并到 `main` / 开 PR / 保留分支。
