# Phase 1 子项目 A：打开项目纵切 设计规格

- 日期：2026-09-23
- 状态：brainstorming 输出，待用户审阅
- 基线：`main@a41d891`（债务清偿完成、套件全绿 3825 文件/33929 测试）
- 行为 oracle：`/Users/itsuka/CodeSpace/orca`（Electron 版，只读参照）
- 上游规格：`docs/superpowers/specs/2026-09-14-ade-design.md`（§5 crate 职责、§8 Phase 1）

## 1. 背景与目标

Phase 1（核心工作流闭环）拆为三个子项目：**A 打开项目纵切**（本规格）、B worktree + git（status/diff/commit、worktree 创建/切换/删除）、C 终端/PTY + agent。A 的目标是把渲染层从「全 mock bridge」推进到「真实 Rust 后端」，交付可日常使用的项目打开与文件编辑闭环：

**验收（自动 + 手工）**：`pnpm dev` 下 —— 选择本地目录/仓库添加为项目 → 项目与分组持久化 → 重启后恢复 → 文件树浏览 → Monaco 打开/编辑/保存真实文件 → 外部修改触发文件树/编辑器刷新 → 全局搜索与 quick open 命中真实文件 → 文件夹工作区与项目分组可用 → 设置页少数分组写入 `settings.json` 并在重启后生效。自动化门禁：`pnpm test`（既有 3825 文件套件）全绿 + 新增 Rust/契约测试全绿 + `cargo test` 全绿 + 冷 `typecheck/build:web` exit 0。

## 2. 范围

### 2.1 A 实现（真实 IPC）

| 域 | 方法 |
|---|---|
| fs | `readDir readFile writeFile createFile createDir rename copy deletePath stat pathExists pathsExist listFiles cancelListFiles search watchWorktree unwatchWorktree onFsChanged listMarkdownDocuments authorizeExternalPath` |
| repos | `list add update remove reorderForHost pickFolder pickFolders pickDirectory isGitAvailable getDefaultCreateProjectParent onChanged` |
| projects | `list listHostSetups`（TS 侧由 repos 投影，复用 shared 投影函数）`update`（回显 `localWindowsRuntimePreference`，不落盘；Windows 后置项） |
| projectGroups | 全部 9：`list create update delete moveProject scanNested cancelNestedScan importNested onNestedScanProgress` |
| folderWorkspaces | 全部 5：`list create update delete getPathStatus` |
| settings | `get getSync set onChanged` |
| ui | `get set setWithAck recordFeatureInteraction onStateChanged` |
| worktrees（最小） | `list listAll onChanged`（主工作树投影） |
| platform | `get`（bootstrap 同步） |
| app | `getIdentity` |

### 2.2 A 响亮未实现（保持 `UnimplementedBridgeError`，后续子项目接真）

- fs：`download*`（7）、`readLocalLogTail/startLocalLogTail/stopLocalLogTail/onLocalLogTailChanged`、`importExternalPaths/stageExternalPathsForRuntimeUpload/resolveDroppedPathsForAgent`
- repos：`clone cloneRemote create createRemote addRemote cloneAbort getGitUsername getBaseRefDefault searchBaseRefs searchBaseRefDetails onCloneProgress`
- projects：`createHostSetup setupExistingFolder updateHostSetup deleteHostSetup`（远程/host-setup 面）
- settings：`setActiveRuntimeEnvironmentPreference updatePRBotAuthorOverride listFonts previewGhosttyImport previewWarpThemeImport`
- ui：托盘/菜单/快捷键类事件订阅（`onOpenSettings onToggle* onOpenMarkdownFiles onRequestTabCreate` 等；A 无托盘）
- app：除 `getIdentity` 外全部
- 其余域（pty/browser/ssh/runtime/git 面板数据域等）：保持现状 mock

### 2.3 明确不做

git 面板与 git status/diff/commit（B）、真实 worktree 创建/切换/删除（B）、PTY/agent（C）、新建仓库（`repos.create`，需 git init）、下载/日志尾随/拖入导入、SSH/远端、托盘、跨端 UI 状态权威合并、Windows 同步验证（记录跟踪）。

## 3. 架构

### 3.1 Rust crate

```
src-tauri/crates/
├── ade-core/     # 领域模型与规则：Repo/ProjectGroup/FolderWorkspace/Worktree 投影、id 规则、
│                 # 路径比较归一、settings/ui-state 默认值（内嵌生成 JSON）、分域错误
├── ade-store/    # JSON 持久化：原子写、schemaVersion、备份轮换、加载期归一
├── ade-fs/       # 路径授权、读写、遍历（listFiles）、搜索、watch
├── ade-git/      # 最小 git CLI 封装：--version / rev-parse --show-toplevel|--is-inside-work-tree /
│                 # worktree list --porcelain -z（B 扩展 status/diff/commit）
└── ade-bridge/   # Tauri commands、事件、错误映射、specta 导出、AppState
src-tauri/src/    # orcinus-app：setup 构建主窗口（init script）、manage state、generate_handler
```

A 不建 ade-store 的 SQLite 面（Phase 2）、不建 ade-pty/agents/browser/plugins/shell。

### 3.2 命令面

- 每方法一个 Tauri command，命名 `<域>_<方法 snake_case>`（如 `fs_read_dir`、`repos_pick_folder`、`settings_get`）。命令名是契约的一部分，由契约测试锁定。
- Rust 侧结构体一律 `#[serde(rename_all = "camelCase")]`，字段与 `src/shared/preload-api/api/*.ts` 的 TS 形状逐字对齐。
- `ade-bridge` 为 Rust 原生类型派生 `specta::Type`，`cargo test -p ade-bridge` 内含「重新生成 bindings 并与 checked-in 文件比对」的新鲜度测试；生成的 `src/bridge/real/generated/tauri-bindings.ts` 仅供新类型/负载参考，桥接适配层仍以既有 TS 契约为准（见 §10 偏差 2）。
- `projects` 域**没有 Rust 命令**：TS 侧 `src/bridge/real/projects.ts` 调 `repos_list` 后复用 shared 的 `projectHostSetupProjectionFromRepos`（`src/shared/project-host-setup-projection.ts`）投影出 `Project/ProjectHostSetup`，避免在 Rust 重写身份分组逻辑。
- 阻塞 IO（文件、git、搜索、watch 安装）在 `tauri::async_runtime::spawn_blocking` 中执行；AppState 为 `Arc` + `Mutex/RwLock` 组合，命令返回 `Result<T, BridgeError>`。

### 3.3 启动引导（bootstrap）

- `orcinus-app` 在 setup 中用 `WebviewWindowBuilder::initialization_script` 注入
  `window.__ADE_BOOTSTRAP__ = { settings, platform, schemaVersion }`（在文档解析前执行）。
- `settings.getSync()` 与 `platform.get()` 同步读该对象（Tauri 2.11 无同步 IPC，已核实）；`settings.set` 走异步命令 + `settings:changed` 事件。
- 主窗口从 `tauri.conf.json` 的声明式窗口改为 Rust 构建（保留现有尺寸/标题配置），以支持 init script。

### 3.4 前端桥接

- `src/bridge/real/<domain>.ts`：薄封装（`invoke('<域>_<方法>', args)` → 契约类型），每个域导出 `createXxxRealApi(): PreloadApi['xxx']`。
- `src/bridge/create-api.ts` 按域组装：real 域优先，未接真域沿用 `src/bridge/mock/*`；`VITE_ADE_BRIDGE=mock` 时全量回退 mock（开发/测试用）。
- 未接真方法保持 `UnimplementedBridgeError('<域>.<方法>')`（real 域内未实现方法由 `withMethodFallback` 包裹）。
- 事件订阅封装：`listen('<event>') → unsub`，载荷映射为契约 payload。

## 4. 持久化

### 4.1 文件布局（app data dir）

| 文件 | 内容 | 写入者 |
|---|---|---|
| `settings.json` | `GlobalSettings`（整对象，可手改） | Rust（浅合并 partial set） |
| `ui-state.json` | `PersistedUIState`（整对象） | Rust（整对象 set） |
| `projects.json` | `{ schemaVersion, repos, projectGroups, folderWorkspaces }` | Rust（注册表变更） |

- `projects.json` 顶层带 `schemaVersion: 1`；`settings.json`/`ui-state.json` 为**纯领域对象**（与 TS 契约 `GlobalSettings`/`PersistedUIState` 逐字一致，不额外包裹），版本演进靠加载期字段归一。
- 所有文件加载期归一（缺字段回默认、类型不符丢弃并记录），未知字段保留（前向兼容）。
- 原子写：同目录 temp 文件 + `fsync` + `rename`；写入前轮换备份（`<file>.bak1/.bak2`），损坏时按备份顺序回退。
- 写盘节流（1000ms 防抖 + 5000ms 最大等待，对齐 orca 的写调度）由 `ade-bridge` 的写调度层实现（Task 8）；`ade-store` 保持同步落盘原语。

### 4.2 默认值（单一源 + 生成）

- 默认值源保持 TS：`getDefaultSettings(homedir)`（`src/shared/constants.ts:162`）与 `getDefaultUIState()`（`src/shared/constants.ts:223`）。
- 生成脚本 `config/scripts/generate-ade-defaults.mjs` 输出 `src-tauri/crates/ade-core/src/defaults/ade-defaults.generated.json`（home 用 `{{HOME}}` 占位，Rust 加载时以平台 home 替换）；`config/scripts/**/*.test.mjs` 增加新鲜度测试（重新生成并比对 checked-in 文件）。
- Rust 加载：`defaults ∪ stored`；`settings.set(partial)` 浅合并（`notifications`/`telemetry`/`worktreeVisibilityDefaults` 深合并照抄 orca；渲染端不可写键 `pluginConsents/disabledPlugins/activeRuntimeEnvironmentId/floatingTerminalTrustedCwds` 剔除）。

### 4.3 ID 规则（照抄 oracle）

- `Repo/ProjectGroup/FolderWorkspace`：UUID v4。
- worktree id：`${repoId}::${path}`；folder 仓库主 workspace 同格式（`getFolderWorkspaceRootId(repo) = repo.id::repo.path`）。
- 去重：`add` 按「路径比较归一（NFC + 反斜杠折叠 + Windows 小写）」判重，重复返回既有 repo（非错误）+ 广播。

## 5. 域语义规范

### 5.1 fs（逐条对齐 oracle）

**路径授权**：allowed roots = 所有 repo.path（git/folder）∪ folderWorkspace.folderPath ∪ `authorizeExternalPath` 显式登记（canonicalize 后前缀匹配，段边界）；拒绝时错误文案逐字：
`Access denied: path resolves outside allowed directories. If this blocks a legitimate workflow, please file a GitHub issue.`

**读取**：`readFile → {content, isBinary, isImage?, mimeType?}`（二进制检测 NUL、图片按扩展名/MIME）；`readDir → DirEntry[]`（`{name,isDirectory,isSymlink}`，**目录在前、自然序**，与 shared `sortDirEntries`（`src/shared/file-name-sort.ts`）一致；渲染端亦会再排序）；`stat → {size,isDirectory,mtime}`；`pathExists/pathsExist`（批量上限 128，逐项 `{exists}|{error}`）。

**写入**：`writeFile/createFile/createDir/rename/copy/deletePath` 原子写 + 授权校验；`deletePath` 走**系统回收站**（对齐 oracle 的 `shell.trashItem`；Rust 用 `trash` crate，删除 root 自身拒绝）。

**listFiles（quick open）**：返回 **root 相对、`/` 分隔**路径数组；两趟（primary 尊重 gitignore + ignored 趟含被 ignore 文件）；hidden 文件包含但修剪隐藏目录黑名单（`.git node_modules .next .nuxt .cache .vscode .idea .yarn .pnpm-store .terraform .docker .husky .npm .gvfs` 等，目录形 glob 才剪枝）；不 follow symlink；`excludePaths` 段边界排除、越界静默丢弃；`maxResults` 由调用方给定（渲染端 20001）；`requestToken` 取消（按 token abort，重复 token 先 abort 旧请求）；本地**忽略 `searchQuery`**（过滤在渲染端）。

**search**：ignore + grep-searcher/grep-regex 内嵌实现，复刻 oracle 语义：
- 遵循 `.gitignore/.ignore/.rgignore`；包含隐藏文件但排 `.git`；不 follow symlink；单文件 >5 MiB 跳过；每文件最多 100 匹配；总量 `maxResults`（默认/上限 2000）；15s 超时 → `truncated=true`；**恰好达到 maxResults 也置 truncated**。
- `line` 1-based；`column = UTF-8 字节偏移 + 1`（bug-for-bug）；`matchLength` 字节数；`lineContent` 去 `\n`、>500 字符窗口截断加 `…`；非 UTF-8 行 `lineContent=''`。
- `includePattern/excludePattern`：逗号分隔 glob（支持 `\` 转义），无 `/` 的 glob 自动加 `**/` 前缀；include→正向、exclude→`!` 反向。
- 结果**不排序**（遍历序）；同 root 新搜索取消旧搜索。

**watch**：`notify` 实现；尾沿 150ms、首事件起最大 500ms 强制 flush；单批 >5000 事件、watcher 错误、订阅中断 → 单条 `overflow`；`delete→create` 保留两条、`create→delete` 抵消；rename 折叠为 delete+create（本地不发 rename）；忽略目录表 `.git node_modules dist build .next .cache target .venv __pycache__`；按 root 引用计数共享，最后一个订阅者离开后 30s 宽限再卸载；失败 root 负缓存；`isDirectory` 对 create/update 探测、delete 留空。

**listMarkdownDocuments**：root 下 `.md` 收集 → `MarkdownDocument{filePath,relativePath,basename,name}`（遍历规则同 listFiles）。

### 5.2 repos / projects / projectGroups / folderWorkspaces

- `repos.add({path,kind?,displayName?})`：kind 默认 `git`；git kind 以 `git rev-parse --show-toplevel` 解析根（失败→`{error:'Not a valid git repository: <path>'}`）；folder kind **不做 realpath**；`displayName = trim || basename(去 .git)`；新 Repo：`badgeColor=默认色、addedAt=now、kind`、其余可选字段缺省。重复 add 返回 `{repo, alreadyExisted:true}`。
- `repos.update`：允许字段同契约 Pick 列表（A 覆盖 `displayName/badgeColor/repoIcon/worktreeBaseRef/worktreeBasePath/kind/issueSourcePreference/projectGroupId/projectGroupOrder/sourceControlAi/externalWorktree*` 等；SSH 专属字段忽略）。`remove` 删 repo，**不级联**删除分组/文件夹工作区（对齐 oracle：由渲染端决定后续）。
- `reorderForHost({orderedIds,hostId})`：写 `projectGroupOrder`/manual order 并返回 `{status:'applied'}`。
- `pickFolder/pickFolders/pickDirectory`：Rust 侧用 `rfd` crate 弹系统目录选择（不经 JS dialog 插件，命令直接返回路径）；取消返回 `null`/`[]`。
- `isGitAvailable`：`git --version`，1.5s 超时。
- `getDefaultCreateProjectParent`：`settings.defaultWorktreeLocation` 未改默认时回 `{{HOME}}/orcinus/projects`，否则返回 configured（照抄 oracle 语义，品牌名替换）。
- `projects.list`：TS 侧调 `repos_list` 后 `projectHostSetupProjectionFromRepos(repos)` 投影；`listHostSetups` 同源；`update` 仅接受 `localWindowsRuntimePreference`，返回应用后的投影对象（不落盘，Windows 后置项，记录在案）。
- `projectGroups.*`：id UUID；`create` 默认 `isCollapsed=false/color=null/tabOrder=max+1`；`scanNested` 有界遍历（maxDepth/maxRepos/timeout，进度事件 `project-groups:scan-nested-progress`，结果 `NestedRepoScanResult`）；`importNested` 将扫描结果导入为 repo+分组（`ProjectGroupImportResult`）。
- `folderWorkspaces.*`：`create` 必带存在的 `projectGroupId`，路径可用性预检（`getPathStatus` 语义），默认 `name=normalize(name, '<group> workspace')`、`sortOrder=now`、`creatorProvenance={kind:'host'}`；`update` 部分字段；`delete → boolean`。
- **事件**：以上全部变更统一广播 `repos:changed`（空 payload）；`worktrees.onChanged` 发 `{repoId}`。

### 5.3 worktrees（最小投影）

- `list({repoId})`：
  - git repo：`git worktree list --porcelain -z`（输出序；首条即主工作树）→ 每项 `Worktree`：`id=repoId::path`、`displayName=branchShort || repo.displayName || basename(path)`、`head=oid`、`branch=完整 ref`、`isMainWorktree=首条`、`isBare=false`、`comment=''`、`linked*=null`、`isArchived/isUnread/isPinned=false`、`sortOrder=0`、`lastActivityAt=0`、`workspaceStatus='in-progress'`、`displayNameMode='automatic'`。
  - folder repo：主 workspace `id=repoId::repo.path`、`path=repo.path`、`head=''/branch=''`、`isMainWorktree=true`、`displayName=repo.displayName`；其余 folderWorkspaces 按 `lastActivityAt` 倒序追加（`head=''/branch=''`、`isMainWorktree=false`）。
- `listAll()`：全部 repo 的 list 合并。
- A 不做 external worktree 可见性过滤（B 处理）；`prunable` 条目在 A 丢弃。

### 5.4 settings / ui

- settings：见 §4.2；`get` 返回完整对象（defaults ∪ stored）；`set` 返回完整对象并广播变更键；`getSync` 读 bootstrap 快照（`set` 后快照同步更新内存副本）。
- ui：`get` 返回 defaults ∪ stored；`set` 整对象浅合并（例外：`contextualToursSeenIds` 并集、`featureInteractions` 逐 id 合并、`workspaceCleanup` 深合并、数组整体替换），返回 `void`；`setWithAck` 同 set 但在落盘失败时 reject；`recordFeatureInteraction(id)` 更新并返回完整对象；`onStateChanged` 广播完整对象（含发起者）。

### 5.5 platform / app

- `platform.get()`：`{platform, osRelease, arch, shell, displayServer}`（bootstrap 注入，Rust 以 `std::env::consts`/`uname` 计算；`displayServer` 仅 Linux，ade 不支持 Linux 时返回 null）。
- `app.getIdentity()`：`{name:'Orcinus', version, ...}`（版本取 `CARGO_PKG_VERSION`/tauri 配置）。

### 5.6 语义基准（normative shared 模块）

Rust 重写必须与以下 fork 内既有实现保持一致（作为可执行语义基准；这些模块及其测试保持全绿）：

- `src/shared/file-name-sort.ts`（readDir 排序）
- `src/shared/text-search.ts` / `text-search-glob-patterns.ts` / `text-search-match-accumulator.ts`（搜索参数、glob 与列/长度语义）
- `src/shared/quick-open-filter.ts` / `quick-open-listing-limits.ts`（listFiles 遍历、黑名单、上限）
- `src/shared/filesystem-entry-types.ts`（DirEntry / FsChangeEvent / FsChangedPayload）
- `src/shared/code-search-types.ts`（SearchOptions / SearchResult）
- `src/shared/project-host-setup-projection.ts`（projects 投影，TS 侧直接复用）
- `src/shared/constants.ts` 的 `getDefaultSettings` / `getDefaultUIState`（默认值生成源）
- `src/shared/persisted-ui-state-types.ts` / `global-settings-types.ts`（持久化 schema）

## 6. 错误处理

- `ade-core`/`ade-fs`/`ade-store` 各自 `thiserror` 枚举；`ade-bridge` 映射：
  - 契约错误联合（`repos.add/create` 等返回 `{error: string}` 的方法）：Rust 命令返回 `Ok(Err(String))` 或 `Err(BridgeError)` 由适配层转 `{error}`；文案对齐 oracle（如 `Not a valid git repository: <path>`、路径拒绝文案逐字）。
  - 其余命令：`Err(BridgeError)` → TS 侧 reject 为 `Error(message)`；`UnimplementedBridgeError` 仍由 bridge 侧生成。
- 事件载荷错误不抛出，写日志（`tracing`/`log` crate）并继续。

## 7. 测试策略

- **Rust**
  - `ade-store`：原子写、备份轮换/损坏回退、schemaVersion、partial 合并、深合并例外、默认值占位替换。
  - `ade-core`：id 规则、路径比较归一、投影（Repo→Project/ProjectHostSetup）、worktree 投影字段。
  - `ade-fs`：临时目录集成测试——授权前缀/段边界、readDir/write/rename/copy/delete、listFiles（hidden/ignore/excludePaths/上限/取消）、search（gitignore、include/exclude glob、5MiB、每文件 100、总量 truncated、字节列语义）、watch（聚合窗口、overflow、delete→create、create→delete、refcount 宽限）——watch 用短窗口注入或直接单测聚合函数。
  - `ade-git`：porcelain 解析（fixtures）、失败路径。
  - `ade-bridge`：命令契约测试（tauri test runtime：命令名、参数反序列化、错误形状）+ specta bindings 新鲜度。
- **TS**
  - `src/bridge/real/*.test.ts`：mock `@tauri-apps/api` 的 `invoke/listen`，断言命令名、参数映射、错误映射、退订。
  - mock/real parity：同一组契约断言跑 real（mock invoke）与 mock（内存实现）。
  - `config/scripts/generate-ade-defaults` 新鲜度测试。
  - 既有 3825 文件套件保持全绿；`create-api` 组装测试（`VITE_ADE_BRIDGE=mock` 全量回退、real 域覆盖）。
- **手工验收**：`pnpm dev` 清单（§1）；Windows 后置跟踪。

## 8. 风险

1. **bootstrap 注入时机**：init script 必须早于渲染层首帧；若 `window.__ADE_BOOTSTRAP__` 缺失，`getSync/platform.get` 需响亮失败并回退异步 `settings.get`（契约返回 `null` 时渲染层已有守卫）。
2. **watch 语义**：macOS FSEvents 的合并行为与 orca 的 parcel 后端不完全一致；以聚合层单测锁定我们自己的语义，差异记录。
3. **搜索字节列语义**：bug-for-bug 会保留非 UTF-16 列偏移；UI 高亮若出现偏移，记录为已知差异（对齐 oracle 优先）。
4. **默认值漂移**：TS 生成 → Rust 内嵌，靠新鲜度测试防漂移；`{{HOME}}` 占位在 Windows 路径分隔符上需单测。
5. **ui-state 事件体积**：全量对象 emit（内联 eval）；当前 schema ~173 字段，体积可控；若后续膨胀改走 command 拉取。
6. **Windows**：本子项目不验证；`notify`/路径大小写/`git worktree` 输出差异记录到跟踪清单。

## 9. 交付物

- Rust：`ade-core`/`ade-store`/`ade-fs`/`ade-git`/`ade-bridge` 新 crates + `orcinus-app` 改造。
- TS：`src/bridge/real/*` + 生成 bindings + 契约/parity 测试 + `create-api` 组装开关。
- 文档：本规格、实现计划、`docs/phase1a-open-project-record.md`（实现记录）。
- 不做：mock 残渣清理（Phase 4）。

## 10. 相对已批准设计的调整（需审阅确认）

1. **最小 git CLI 使用**：原设计说 worktree 投影读 `.git/HEAD` 元数据、不调 git；为与 oracle 的 `git worktree list --porcelain -z` 语义 1:1（含多 worktree、完整 ref、输出序、`isMainWorktree` 次序判定），A 引入 `ade-git` 最小封装（`--version`、`rev-parse`、`worktree list --porcelain -z`）。理由：正确性与 B 复用；代价：A 依赖系统 git（`isGitAvailable` 已有此前提，非新约束）。
2. **specta 的范围**：Rust 原生类型派生并生成 bindings 供新负载参考；桥接适配层仍以既有 TS 契约为准（迁移期 TS 是契约源，避免同形状双源）。生成文件 checked-in + `cargo test` 新鲜度断言。
3. **默认值源**：§4.2 以 TS 为源生成 Rust 内嵌 JSON，而非在 Rust 手写 198 字段默认值；与上游 spec §6.4「默认值固化进 ade-core」为有意偏差，记录理由（单源、防漂移）。
4. **projects 投影留在 TS 侧**：Rust 不实现 projects 命令，桥接层用 `repos_list` + shared 投影函数组装；代价：多一跳 IPC、`projects.update` 不落盘（Windows 后置项）。
