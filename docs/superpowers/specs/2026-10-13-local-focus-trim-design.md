# Orcinus 功能删减设计(第二轮瘦身:聚焦本地编码)

- 日期:2026-10-13
- 状态:brainstorming 输出,待用户审阅
- 基线:`main` @ `415593ce`(Phase 3A 插件系统收口后)
- 相关文档:`docs/superpowers/specs/2026-09-14-ade-design.md`、`docs/phase1-feature-trim-record.md`、`docs/phase0-dead-code-inventory.md`
- 下游:本规格审阅通过后使用 writing-plans 技能编写实施计划

## 1. 背景与目标

Phase 1 已删 10 个功能域(1383 文件)。本轮为第二轮删减,产品方向收敛为**本地编码**:终端/PTY、worktree、编辑器、agent-in-terminal、git 基础。经盘点(生产可达性 + Tauri bindings 命名空间对照),删除以下 7 个域 + 杂项:

无后端死面、Web 远程客户端、GitHub 全链(含只读 checks)、Computer-use、Automations、Onboarding、Artifacts。

关键判定事实(盘点证据,详见删减记录):

- **无后端死面**:Tauri bindings(`src/bridge/real/generated/tauri-bindings.ts`,约 150 命令)无 browser/ssh/jira/linear/ephemeral-vm/runtime-environments/updater 等命名空间,对应 UI 在生产环境调用即 reject;
- **Web 远程客户端双重死**:Rust 侧无 WebSocket/relay 服务端、配对 UI 已删(`30a6956e`)、`web-index.html` 不进生产包(vite 单入口,`build:web` 名不副实、实际只构建桌面入口),浏览器端永远连不上;
- **GitHub**:后端仅 2 条 gh 命令(readonly),PR 页/task 集成/project 面无后端;
- **AI Vault 无发布/共享面**(全树无 upload/share/publish 路径),不在本轮删除范围。

「彻底删除」定义沿 Phase 1:生产代码 + 测试 + mock bridge 域 + preload 契约 + store 注册/slice + 设置项与导航/搜索条目 + i18n 键 + 仅被删域独占的依赖 + 对应 Rust crate/命令/bindings。

目标:

- 删除后 `pnpm typecheck`、`pnpm build:web` 全绿,`pnpm dev` 可启动,终端/worktree/编辑器/agent/通知/插件中心等保留功能正常
- 每步可独立回滚;`pnpm test` 不新增失败(开工前重采集基线)
- 更新主规格,产出删减记录 `docs/phase3b-feature-trim-record.md`

非目标:不动终端/PTY 本地部分、worktree/项目模型、agent hooks/状态、编辑器 + diff 批注、git 基础(source control/commit/sparse)、Cmd+J、通知、会话持久化(含 `agent_sessions.rs`)、**插件系统**、**AI Vault 核心**、ephemeral-vm composer 运行目标 UI(维持 Phase 0「保留 run-target」决策)、Landing、浮动终端、workspace 清理。

## 2. 删除范围与边界

### 2.1 无后端死面

判定依据:桥 namespace 无 real/bindings 支撑,生产调用即 reject。

| 项 | 内容 |
|---|---|
| 内嵌浏览器 | `components/browser-pane/`(291 文件)+ `shared/browser-*`(45)+ store/slices/browser + tab 类型 `'browser'` + 设置 BrowserPane/BrowserUsePane + preload browser-bridge |
| Linear/Jira/GitLab | linear*(~128)、jira*(~49)、gitlab*(~32,含 task-page 内相应部分) |
| SSH | `renderer/src/ssh/`(8)+ ssh 关键字文件中非共享部分(sidebar host 管理、设置、终端远程 PTY 传输);**本地 PTY 链路保留** |
| 远程主机死树 | runtime-environments / ephemeral-vm 纯不可达文件 / `remote-runtime-*`/`relay-*` 死树(~46;牵连 live 测试支持的实施时先解耦) |
| 杂项孤儿 | updater(`maintenance/` 7)、star-nag(4)、ports(3)、network/AddressPicker(3) |
| Electron 遗留 | `src/main` 全目录(20 文件,无生产引用方);`src/preload` 除 `api-types.ts` 外全部(154/155) |
| 设置/导航 | BrowserPane、host 管理等设置项、设置导航/搜索条目、Cmd+J 条目、feature-interactions 条目 |
| i18n | 各域键(6 locale + `en-runtime-required`) |

边界:SSH/browser 在 sidebar、terminal-pane、settings 的挂载点逐一解耦;`bridge/create-api.ts` 对应 mock 域一并删。

### 2.2 Web 远程客户端

| 项 | 内容 |
|---|---|
| 主目录 | `src/renderer/src/web/` 全目录(100 文件 ≈ 16k 行,含 33 测试;与桌面代码零反向依赖,已验证) |
| shared 配角 | `pairing-local-ui-fields`、`runtime-environment-store`、`mobile-relay-pairing-fixtures`、`runtime-pairing-reach`、`mobile-relay-status`/`-mint-failure`/`mobile-pairing-connection-mode`、`mobile-pairing-custom-address`(连带 settings store normalize 与 `default-global-settings` 字段)、`remote-pairing-address`/`-verification` + `lib/remote-pairing-copy.ts` + 孤儿组件 `RuntimeAccessGrantList.tsx` + `runtime-access-grants.ts`、`network/pairing-url.ts`、`serve-option-validation.ts` |
| **保留** | `shared/pairing.ts` 本体(`parsePairingCode` 被 `ephemeral-vm-recipes.ts` 引用);门禁命令 `build:web` 零改动(实际只构建桌面入口) |
| 桌面分支 | `lib/web-client-location.ts` 的 `isWebClientLocation` 约 12+ 处桌面消费分支(settings/terminal-clipboard 等),随删或收尾统一判定(编译器兜底) |
| 依赖 | `qrcode`(全仓零 import)、`tweetnacl`、`ws` |
| i18n | `auto.web.*` 等键 |

### 2.3 GitHub 全链(含只读 checks)

用户决策:只读 checks 一并全删。

| 项 | 内容 |
|---|---|
| 组件 | task-page/github 部分、`pull-request-page/`(54)、`github-item-dialog/`(52)、`github-project/`(53)、`github/`(44)+ 散文件、right-sidebar checks/pr-checks、editor CheckRun 面板 |
| 设置 | IntegrationsPane 的 Review providers 卡、`GitProviderApiBudgetPane`、相关搜索条目 |
| 后端 | 桥 `gh` 域(`ghExec`/`ghEnvProbe` 2 命令)、`bridge/real/gh.ts`、mock gh 域、bindings 重生成 |
| 边界 | GitPane/CommitMessageAiPane 中 git 基础部分(commit/diff/status)保留;task-page 宿主若纯为 provider 服务则整删(实施时按引用判定,linear/jira 部分已随 2.1 走) |
| i18n | 相关键 |

### 2.4 Automations

| 项 | 内容 |
|---|---|
| 渲染层 | `components/automations/`(283 文件)+ store 切片 + 侧栏 Automations 按钮(默认显示)+ 设置 pane 与导航/搜索条目 |
| Rust | crate `ade-automations`(5 文件 5048 行)、`ade-bridge` automations.rs(497 行)、10 条 `automations_` 命令、AppState/specta 清单、bindings 重生成 |
| 存储 | SQLite automations 表:只删生产者,不迁移旧表(沿 Phase 1「只删生产者不删字段」惯例) |
| i18n | 相关键 |

### 2.5 Computer-use

| 项 | 内容 |
|---|---|
| Rust | crate `ade-computer`(12 文件 3630 行)、`computer_use.rs`(294 行)+ `computer_service.rs`、3 条 `computer_` 命令、AppState/specta、bindings 重生成 |
| Swift | `src-tauri/native/computer-use-macos/`(28 源文件)+ 相关构建配置(tauri.conf 外部二进制/构建脚本,实施时定位) |
| 渲染层 | `ComputerUsePane`、`ComputerUseSkillSetupPanel`、`computer-use-permissions` 桥、设置导航条目(仅桌面显示) |
| 技能 | `skills/computer-use/SKILL.md`、`skill-guides/computer-use.md`、orcinus-cli `computer` 子命令 |
| i18n | 相关键 |

### 2.6 Onboarding

| 项 | 内容 |
|---|---|
| 组件 | `components/onboarding/`(32 文件)+ `use-onboarding` 首启链挂载点 |
| 行为 | 删除后首启直接进主界面(Landing 兜底);注意与 Phase 1 已拆的 feature-tips 部分无残留纠缠 |
| 契约 | 仅服务向导的 shared/onboarding 契约(实施时判定) |
| i18n | 相关键 |

### 2.7 Artifacts

| 项 | 内容 |
|---|---|
| 组件 | `components/artifacts/`(29 文件)+ 设置 pane |
| 入口 | 侧栏 `showArtifactsButton` 显示逻辑与设置字段 |
| i18n | 相关键 |

### 2.8 杂项(随收尾或就近顺带)

- `spikes/cef-embed/target/` 构建产物(~2730 文件):若已被 gitignore 则仅本地磁盘清理、不算提交;若被追踪则 `git rm --cached` + gitignore。
- **AI Vault 死子面清理**(零 UX 损失):搜索面(`shared/ai-vault-search-*` 9 文件 + real 域 rejecting 方法)、子代理列表(`AiVaultSessionSubagents.tsx` + stub)、First Prompt 卡(`ai-vault-first-prompt-card.tsx` + `getFirstUserPrompt` stub);parity 锁定数同步。**不碰** Vault 核心(扫描/列表/resume/续聊/tab 标题)与 `agent_sessions.rs`。

### 2.9 全局收尾

- 可达性兜底:最终入口集(web 入口删除后收敛为 `index.html → main.tsx` + bridge/preload/types)全量扫描,新增孤儿 = 0
- i18n 全量清键 + 空壳清理 + 守卫/semantic 测试更新
- 依赖清理:逐项 grep,仅删独占项(候选:`qrcode`、`tweetnacl`、`ws`、browser/SSH/GitHub 独占依赖,实施时确认)
- ratchet 重基线:`terminal-pane-store-subscription-budget`、`pane-agent-identity-*`、`child-process-import-boundary`、`windows-console-visibility`(preload 树删除后后者可能转绿)等按实际更新 pin;既有红逐字对齐基线
- `pnpm test` 全量与开工前新采基线集合对比:新增失败必须清零
- 文档:更新 `2026-09-14-ade-design.md` §2 保留/删除清单;新增 `docs/phase3b-feature-trim-record.md`(格式沿 phase1-feature-trim-record)

## 3. 执行顺序与理由

| 步 | 域 | 理由 |
|---|---|---|
| 1 | 无后端死面(2.1) | 规模最大、风险最低(生产必 reject),为后续步骤清场;SSH/task-page 与 GitHub 的边界在本步定型 |
| 2 | Web 远程客户端(2.2) | 零反向依赖,独立性强;死面之后删减少 reach 噪音 |
| 3 | GitHub 全链(2.3) | 后端仅 2 命令,牵连集中在 right-sidebar/editor/task-page |
| 4 | Automations(2.4) | 后端完备,首个动 Rust crate/命令/bindings 的步骤 |
| 5 | Computer-use(2.5) | 含 Swift 包与构建配置,独立性最强 |
| 6 | Onboarding + Artifacts(2.6/2.7) | 小域,各一个小提交 |
| 7 | 杂项 + 全局收尾(2.8/2.9) | 兜底扫描、i18n、依赖、ratchet、文档 |

提交策略:分支 `phase3b-trim-local-focus`;第 1–6 步每域一个提交、第 7 步一个收尾提交,每个提交可单独 revert;全程不 merge/push。

## 4. 每域标准作业流程(SOP)

沿 Phase 1,另补 Rust 侧:

1. **可达性分析**(`orcinus-reach.mjs`):入口 = `src/renderer/index.html → main.tsx`(+ 第 2 步前含 `web-index.html → web/main.tsx`)+ `src/main`、`src/preload`、`src/bridge`、`src/types` 全量;解析相对导入、`@/`、`@renderer/` 别名与 lazy `import()` 字面量;测试文件不入图。产出域内独占文件清单与共享文件引用点清单
2. **分批删除**:域内文件整批 `git rm`;每批后冷门禁 `rm -f tsconfig.tsbuildinfo && pnpm typecheck && pnpm build:web` 必须 exit 0
3. **清理关联面**:mock 域 → preload 契约 → Tauri 命令/`ade-bridge` AppState/specta → bindings 重生成 → store 注册/slice → 设置项/导航/搜索条目 → i18n 键(6 locale + `en-runtime-required`)→ ratchet/守卫 pin
4. **Rust 补充**:删 crate → workspace 成员清理 → `cargo check`/`cargo test -p <受影响 crate>` → bindings 重生成 → parity 锁定数同步
5. **测试**:受影响 guard/boundary 测试;沿惯例加「入口不得回潮」反回归断言
6. **提交**:`chore: 移除 <域>(含契约与测试)`

## 5. 验收门禁

| 层级 | 门禁 |
|---|---|
| 每批删除后 | 冷 `pnpm typecheck` + `pnpm build:web` exit 0;Rust 批次后 `cargo check`(+ 受影响 crate 测试) |
| 每域完成后 | 受影响 guard 测试通过;`pnpm dev` 冒烟:启动、终端可开 tab 执行命令、worktree 列表可切换、设置页可达、被删入口不存在 |
| 收尾 | reach 新增孤儿 = 0;`pnpm test` 对新基线无新增失败;i18n 守卫绿;依赖 grep 无独占残留 |

已知约束:`pnpm test` 基线存在既有失败(phase1-feature-trim-record 记录 172 failed + 4 collection,以开工前重采为准),本任务只保证不新增。

## 6. 风险与缓解

| 风险 | 缓解 |
|---|---|
| 死面判定基于「mock-only→reject」分析,可能存在活引用(如 `parsePairingCode`→ephemeral-vm-recipes) | 逐文件以导入图为准;歧义文件保守保留并记录;编译门禁逐批拦截 |
| SSH 牵动 terminal-pane 远程传输与 sidebar host 管理,易误伤本地 PTY | 死面步内单独小批删除;每批后本地终端冒烟 |
| web 客户端复用整个 App,删除后类型链断裂 | 已验证零反向依赖;`build:web` 门禁兜底;`isWebClientLocation` 分支收尾统一清 |
| GitHub 与 task-page/source-control 交界模糊 | GitHub 步内先做 task-page 宿主引用判定;git 基础面只减 provider 不动 core |
| Rust crate 删除牵动 bindings/specta/parity | 每 crate 删除后立即重生成 bindings;parity 锁定数同步;`cargo test` |
| Onboarding 删除影响首启路由 | Landing 兜底;首启直接落主界面冒烟 |
| ratchet/测试基线漂移与既有红混淆 | 开工前重采集基线;收尾集合对比,既有红逐字对齐 |
| SQLite automations 表残留 | 按惯例只删生产者;记录中注明表留存 |

## 7. 后续

本规格审阅通过后,使用 writing-plans 技能编写实施计划(按 §3 顺序拆分任务,每任务含可达性分析、批次删除、契约/键清理、Rust 侧处理、验证命令与提交)。
