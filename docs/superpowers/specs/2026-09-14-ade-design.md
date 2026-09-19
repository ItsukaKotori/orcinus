# ade 设计规格（基于 Orca 二次开发）

- 日期：2026-09-14
- 状态：brainstorming 输出，待用户审阅
- 基线：stablyai/orca v1.4.197（`D:\CodeSpace\itsuka-orca\orca-main\orca-main`，只读参照）

## 1. 背景与目标

Orca 是 Electron 43 + React 19 + Tailwind 4 + shadcn/Radix + Zustand 的 AI 编排 IDE。本项目在 Orca 基础上二次开发 ade：

- 桌面壳迁移到 **Tauri v2**，主进程**完整 Rust 重写**
- 渲染层**整包 fork**，保留 `window.api` 作为迁移缝
- 首发平台：**Windows + macOS**
- 第一里程碑：**UI 先行**（直接建 Tauri 骨架，mock 数据跑通界面）

基线事实（用于估算）：

| 项 | 数值 |
|---|---|
| 渲染层文件数 | ~9,978 |
| 依赖 `src/shared` 的渲染层文件 | 5,227 |
| 深层相对导入（`../../..`） | 5,288 |
| 使用 `window.api` 的渲染层文件 | ~952 |
| preload API 方法（约） | ~987 |
| preload `invoke/send` 调用点 | 787 |
| 主进程 TS 体量 | ~77 MB（含测试） |
| 现有插件系统 | `orca-plugin.json` v1、用户级作用域、私有市场、consent 指纹、kill list、worker、panel bridge |

关键约束：`orca-main` 是无 git 历史快照，fork 采用文件拷贝，**放弃上游合并能力**。

## 2. 范围

### 2.1 保留

- **A 终端与进程**：多标签/分屏 PTY、WebGL 渲染、scrollback 持久化与重启恢复、agent TUI 状态识别、hook server
- **B 工作区**：仓库目录、项目分组、git worktree 创建/切换/删除、文件夹工作区、磁盘占用清理
- **C Agent 编排**：Claude Code/Codex/OpenCode 等 CLI 启动与 resume、agent 状态/通知/未读、automations、AI Vault
- **D Git 与评审**：source control 面板、GitHub/GitLab/Bitbucket/Gitea/Azure DevOps/Linear/Jira、diff 注释、PR 创建
- **E 编辑器与预览**：Monaco、文件树、Markdown/图片/PDF 预览、文件拖入 prompt
- **F 内嵌浏览器**：Chromium 浏览器标签、Design Mode、cookie 导入、网络隧道
- **H 桌面壳**：多窗口、托盘、自动更新、全局快捷键、computer-use、通知

### 2.2 删除

- **G 远程与多端**：SSH 执行、远程 runtime/relay、移动端配对、Web 客户端模式（暂不做）
- **I**：Orca CLI、skills 共享、ephemeral VM、多账号切换与用量统计
- **J Phase 1 功能删减（10 域）**：Pet、Contextual Tours、Feature Tips、Feature Wall、Setup Guide、Dictation、Emulator Pane、Activity/Dashboard（含 Kanban/Agent Map/Popout）、Native Chat、Telemetry（含崩溃上报与 Feedback）——含生产代码、测试、mock bridge 域、preload 契约、store 注册、设置项与 i18n 键；明细与证据见 `docs/phase1-feature-trim-record.md`

### 2.3 改造 / 新增

- **配置简化**：砍选项 + 默认值固化
- **插件中心**：全局 + 项目级双作用域

### 2.4 明确不做

- Linux 平台
- 从 Electron 版迁移数据（settings 可手改）
- 上游 Orca 合并

## 3. 仓库与工程结构

方案 A：ade 独立仓库（`itsuka-orca/ade/`），`orca-main/` 只读参照。

```
ade/
├── src-tauri/                 # Rust workspace
│   ├── Cargo.toml
│   ├── crates/
│   │   ├── ade-core/          # 领域模型：项目/工作区/worktree/会话/agent 状态
│   │   ├── ade-store/         # SQLite + settings.json 持久化
│   │   ├── ade-pty/           # PTY 宿主（独立进程 + 本地 IPC）
│   │   ├── ade-git/           # 系统 git CLI 封装、porcelain 解析（2.25 基线）
│   │   ├── ade-agents/        # agent 启动/恢复、hook server、TUI 状态识别
│   │   ├── ade-fs/            # 文件树、watch、ripgrep 搜索
│   │   ├── ade-browser/       # 内嵌浏览器 + CDP
│   │   ├── ade-plugins/       # 插件宿主（双作用域）
│   │   ├── ade-shell/         # 窗口/托盘/更新/通知/computer-use
│   │   └── ade-bridge/        # Tauri commands、事件、specta TS 导出
│   └── ade-app/               # Tauri 入口（tauri.conf.json）
└── src/
    ├── renderer/              # fork 自 orca src/renderer（路径原样，保证相对导入生效）
    ├── shared/                # fork 自 orca src/shared（路径原样）
    └── bridge/                # window.api 适配层（先 mock，后 Tauri IPC）
```

仓储约束：

- `renderer` 与 `shared` 必须保持 `src/renderer/...` ↔ `src/shared/...` 的相对关系（5,288 处深层相对导入依赖此布局）
- `window.api` 类型声明 fork 自 `src/preload/api-types.ts`，只保留类型与契约，不带 Electron 实现
- orca 测试 fixture 可用于 bridge mock 数据

## 4. 关键架构决策

1. **契约方向**：Rust 为源；struct/enum 用 serde + **specta** 导出 TS 类型。`src/bridge` 用 TS 实现现有 `OrcaApi` 接口，类型检查强制不漏方法。UI 阶段由 mock 实现，后续逐方法替换为 Tauri IPC。
2. **PTY 宿主**：独立长驻进程（Windows 命名管道 / macOS Unix socket），保住“终端重启恢复 + scrollback 持久化”；渲染层投递走 **Tauri Channel**，带背压分片。
3. **内嵌浏览器 (F)**：保留 Design Mode/CDP 对等需内嵌 **CEF**（`cef` crate，sidecar 子进程）。系统 WebView 无法对等（WebView2 无 CDP 控制面、WKWebView 更弱）。Phase 0 spike 决定 go/no-go；失败则降级为系统 WebView + 注入式元素拾取（Design Mode 仅 HTML/CSS，无录像/完整 cookie 能力），并在 UI 标注能力差异。
4. **持久化**：设置 = 单一版本化 `settings.json`（schema 校验、可手改）；会话/插件审计/索引用 SQLite（rusqlite）；scrollback 快照为独立文件（参照 orca snapshot 机制）。
5. **错误处理**：Rust 用 `thiserror` 分域错误，bridge 映射为渲染层认识的现有错误形状；订阅类事件保持“快照 + 增量”语义。

## 5. Rust crate 职责

| crate | 职责 | 主要依赖 |
|---|---|---|
| `ade-core` | 领域模型与状态机：项目/工作区/worktree/会话/agent 状态/默认配置 | serde |
| `ade-store` | settings.json 读写与迁移、SQLite（会话/审计/索引）、scrollback 快照 | rusqlite |
| `ade-pty` | PTY 宿主进程、会话生命周期、scrollback 序列化、重启恢复 | portable-pty（ConPTY/posix） |
| `ade-git` | git CLI 调用与 porcelain 解析、worktree 操作、capability 探测（2.25 基线） | std::process |
| `ade-agents` | agent 启动/resume 命令行构建、hook server、TUI 状态识别（transcript 驱动） | — |
| `ade-fs` | 文件树、watch（notify）、搜索（ripgrep）、路径安全 | notify |
| `ade-browser` | CEF 宿主、CDP 会话、Design Mode、cookie 导入、网络隧道 | cef |
| `ade-plugins` | manifest 校验、安装/staging、哈希与完整性、lockfile、consent、kill list、worker supervisor、panel bridge、市场客户端 | — |
| `ade-shell` | 窗口/托盘/自动更新/通知/全局快捷键/computer-use | tauri plugins |
| `ade-bridge` | tauri command 注册、事件、specta 导出、错误映射 | tauri, specta |

## 6. UI 结构

### 6.1 最左侧全局 Activity Rail（新组件）

- 竖向图标栏，常驻；条目：`项目`（默认）/ `插件中心` / 动态列出的全局插件 / 底部设置·帮助
- 语义为**顶层视图切换器**：
  - **项目视图**：现有 Orca 主界面（左侧项目/worktree sidebar + 主工作区），行为与基线一致
  - **插件中心视图**：主区域整体切换为插件中心页面，现有左侧 sidebar 隐藏
  - **全局插件视图**：主区域整体切换为该插件沙箱面板（全宽容器复用 `plugin-panel-shell`），左侧 sidebar 隐藏
- 基于现有 `top-level-view` 机制扩展（现枚举：terminal/settings/tasks/activity/automations/space/skills/artifacts/mobile；删除 skills/mobile，新增 `plugin-center`；全局插件视图以 `plugin:<pluginKey>` 形式寻址），同一持久化边界校验
- 当前视图与选中项纳入 UI 持久化，重启恢复

### 6.2 插件中心视图（全局作用域）

- 三视图：**已安装**（启用/禁用、权限同意、更新、卸载、运行日志）、**市场**（沿用现有私有 marketplace）、**开发中**（dev watcher）
- 每行标注作用域 `全局` / `当前项目`；安装时可选目标作用域
- consent UI 沿用现有指纹模型并补充作用域说明

### 6.3 右侧栏：项目级插件

- 沿用现有「插件面板 → 右侧 activity bar 条目」机制（`plugin-panel-activity-items.ts`），新增**作用域过滤**：右侧只显示对当前项目启用的插件面板；全局作用域插件面板只出现在最左 rail
- 顺序：files / source control / checks 之后追加项目插件，支持现有排序能力
- 面板渲染沿用沙箱 panel frame + panel bridge 权限门，不新造运行时

### 6.4 设置页简化

- 保留分组：外观、终端、Agent 默认项、通知、插件、更新、快捷键、隐私
- 删除：SSH/远程、移动端、CLI/skills/VM、多账号/用量、host 覆盖、大多数实验开关
- 默认值固化进 `ade-core` 配置默认；**取消项目级设置覆盖**（项目仅保留业务配置，如 setup script；插件自身设置除外 `settings:own`）
- 存储：单一版本化 `settings.json` + schema 迁移

### 6.5 连带删除的 UI 面

SSH 连接/端口转发、移动端配对、远端主机添加、账号切换/用量页、CLI/skills/VM 页面，及对应 `window.api` 方法与 `top-level-view` 枚举项。

Phase 1 追加删除：Pet、Contextual Tours、Feature Tips、Feature Wall、Setup Guide、Dictation、Emulator Pane、Activity/Dashboard（含 Kanban/Agent Map/Popout）、Native Chat、Telemetry（含崩溃上报与 Feedback）的入口，及对应 `window.api` 契约、设置项与 `top-level-view` 枚举项。

## 7. 插件系统设计

### 7.1 作用域与存储

| | 全局 | 项目级 |
|---|---|---|
| 安装实体 | `{appData}/ade/plugins/<key>` | `<repo>/.ade/plugins/<key>` |
| 数据目录 | `{appData}/ade/plugin-data/<key>` | `<repo>/.ade/plugin-data/<key>`（本地，gitignore） |
| 入口位置 | 最左 rail | 右侧栏（files/git 之后） |
| 启用范围 | 所有项目 | 仅所属项目 |

- 仓库提交 `<repo>/.ade/plugins.lock.json`（id/版本/来源/内容哈希），实体与数据默认忽略；新机器打开项目时按 lockfile 提示一键安装
- 同 id 两边都装时**项目级遮罩全局**，UI 明确标注；同一作用域内不允许重复 key

### 7.2 信任与同意

- 沿用现有 consent 指纹模型（capabilities + worker 信任），指纹变化强制重新同意；两作用域**独立同意**
- 项目级插件代码来自仓库，视为不可信输入：**绝不自动安装/启用**，首次启用必须显式确认（防恶意仓库诱导）；提供“受信任项目”标记
- 内容完整性校验、kill list（吊销）、审计日志沿用

### 7.3 加载与生命周期

- 启动加载全局已启用插件；项目打开时加载该项目已装且已启用的插件；项目关闭即终止其 worker、卸载面板
- worker 按需启动（沿用 lazy 模式）；全局/项目插件各自独立进程与独立 capability gate；capability 检查在 Rust 侧每个 host API 调用点执行
- 命令 `context: global|worktree` 语义沿用；同 id 命令冲突时项目作用域覆盖全局

### 7.4 安装 / 更新 / 卸载

- 安装流：中心 UI 选来源（git/本地路径/市场）→ 选作用域 → staging 下载 → 清单+哈希+路径安全校验 → lockfile 落盘 → consent → 启用
- 更新：全局可自动检查；项目级仅显式触发（避免团队内行为漂移）
- 卸载：删实体 + lockfile 条目；数据目录默认保留，UI 提供清理入口

### 7.5 manifest 与贡献点

- manifest 采用新名 `ade-plugin.json`，**兼容读取 `orca-plugin.json`**（迁移期）；plugin API 自 v1 起冻结兼容承诺
- 贡献点裁剪：保留 panels / commands / agent profiles / keybindings / language packs；删除 vm recipes（随 I 删除）
- 面板渲染与 panel bridge 协议与现有同形（host API 调用 → Tauri IPC → ade-plugins → capability gate）

### 7.6 Rust 映射

`ade-plugins` 承载：manifest 解析/校验、staging 安装、哈希与完整性、lockfile、consent 指纹、kill list、worker supervisor、panel bridge host、storage/secrets、市场客户端。

## 8. 迁移阶段

### Phase 0 — 骨架与 UI（当前里程碑）

交付：

- 新建仓库、Tauri v2 骨架（Windows/macOS）、Vite + React + TS 构建；fork `renderer` + `shared`，删除 G/I 代码与 UI 面
- `src/bridge` 以 mock 实现 `OrcaApi`（数据复用 orca fixtures；未实现方法显式标记）
- 最左 rail 三视图切换、插件中心页（mock 数据）、右侧项目插件面板（mock）、简化设置页
- spike：**CEF 打包验证**（体积/启动/双平台构建）、**Tauri Channel 终端吞吐验证**

验收：`pnpm dev` 启动 Tauri 窗口；三视图可切换且状态可持久化；插件中心可浏览；设置页可用；构建无 Electron 依赖；两个 spike 有书面结论。

### Phase 1 — 核心工作流闭环

`ade-core/fs/git/pty/agents` 最小集：打开项目/仓库 → worktree 创建/切换/删除 → 终端 tabs/splits 运行 Claude Code/Codex → git status/diff/commit → 文件树 + Monaco 编辑。bridge 按视图将 mock 替换为真实 IPC。

验收：在真实项目上跑通“打开 → worktree → agent → 提交”。

### Phase 2 — 体验补全

SQLite 持久化、scrollback 快照与重启恢复、agent 状态/hook server、通知/未读、quick open/搜索、diff 注释与评审、GitHub 等 Provider、automations、AI Vault、computer-use（按依赖排序）。

验收：日常可替代原版使用（除浏览器/插件外）。

### Phase 3 — 浏览器与插件后端

CEF 内嵌浏览器（Design Mode/cookie 导入/网络隧道）；`ade-plugins` 全链路；插件中心接真实数据；右侧项目插件真实加载。

验收：插件从安装到面板运行全链路；项目 lockfile 一键安装。

### Phase 4 — 桌面壳收尾

托盘/多窗口/签名自动更新、macOS 公证、CI 打包；清理 mock 残渣与契约测试全绿。

## 9. 测试策略

- Rust：各 crate 单元 + 集成测试（PTY/git 走真实进程）
- `ade-bridge`：契约测试（命令名/参数/错误形状）防漂移
- TS：沿用 vitest；bridge mock/real 双实现一致性测试；渲染层 fork 自带测试删减后跑通
- E2E：tauri-driver（仅 Windows）；macOS 以组件测试 + 截图冒烟为主；CEF 单独 e2e 目标
- 迁移期以 Electron 版 orca 作为行为 oracle 人工对照关键交互

## 10. 风险清单

1. **CEF 集成与打包**（体积、签名、公证、崩溃隔离）→ Phase 0 spike 定 go/no-go
2. **终端性能与背压**（Tauri Channel vs 本地 socket）→ Phase 0 spike
3. **渲染层 fork 构建体量**（~10k 文件）→ 保留 orca 懒加载设计，Phase 0 优化
4. **987 方法迁移期的半 mock 体验** → 显式“未实现”标记 + 按视图接入
5. **隐式行为翻译**（agent 状态识别、终端语义）→ 复用 transcript fixtures 固化为行为测试
6. **computer-use 平台 API 的 Rust 可用性** → 放最后阶段，必要时降级

## 11. 后续

本规格审阅通过后，使用 writing-plans 技能编写 Phase 0 实施计划。
