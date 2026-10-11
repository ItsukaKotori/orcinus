# Phase 2 子项目 G：computer-use（桌面控制，macOS 全量自足）收尾记录

- 日期：2026-10-11（实施）～ 2026-10-11（终态自动门禁）
- 分支：`main`（spec 基线 `3cd628ac`；计划 `646178ee` + 预检修正 `d0970081`；特性/fix 提交 `1ce6e7c4`…`63f82efc`）
- 规格：`docs/superpowers/specs/2026-10-11-computer-use-design.md`
- 计划：`docs/superpowers/plans/2026-10-11-computer-use.md`（14 任务；Task 14 勾选全部复选框）
- 行为参照：`/Users/itsuka/CodeSpace/orca`（Electron oracle；`orca:` 前缀均为其内路径）
- 方法：subagent-driven-development（每任务 TDD 实现 + 评审 + fix 轮；终态三组门禁 + T1/T13 专项证据 + 本记录）
- 手工验收状态：**待用户复核（2026-10-11 自动化门禁已全绿；8 项手工清单见 §4）**

## 1. 范围与验收

| 面 | 内容 | 主提交 |
|---|---|---|
| vendored Swift helper | 整树 vendor 自 `orca:native/computer-use-macos`；改名（product/可执行 `orcinus-computer-use-macos`、显示名/文案 Orcinus、`provider:"orcinus-computer-use-macos"`）；认证改 token 严格匹配 + `LOCAL_PEERPID` 同 uid 活性检查（删 Electron 侧车判据）；构建脚本 `config/scripts/build-computer-macos.mjs`（swift release → .app → Info.plist `LSUIElement=true`/bundle `dev.itsuka.orcinus.computer-use` → `APPLE_SIGNING_IDENTITY` 真签或 ad-hoc → `resources/computer-use/` + `.build/release` 双产出）；`resources` 映射记法 | 1ce6e7c4 + b26f34a0 + 0d3e96ff |
| `ade-computer` crate | protocol（16 错误码目录/15 RPC 方法/RuntimeMetadata）、paths/availability（三级解析 + 不可用文案）；transport（spawn/token/NDJSON/重试 100ms×10s/60s 超时/generation 守卫/重启/terminate）；client（handshake 协议门/单飞串行/capabilities 缓存）；动作校验（oracle 文案逐字）与 verification 归一化；permissions（probe/openSetup/reset）；rpc server（runtime.json/socket 0600/信封/1MB/30s/32 连接/keepalive/feature 记录） | 4e307683 + e8f77a04 + 2e274913 + 38aa92e2 + d8c9558b + 6cd70f17 |
| 桥接命令 | `computerUsePermissions` 3 命令 + `ComputerService`/`StoreFeatureRecorder` 装配 + Exit shutdown；`cli` 域 6 命令（PATH 安装/状态/移除 + WSL 恒 unsupported）；`skills_discover`（home 技能根扫描 + 10s TTL）；specta 清单 + bindings 再生成 | d4260739 + e1c0008d + 447c60ee |
| TS real 域 | `real/computer-use-permissions.ts` 3 显式、`real/cli.ts` 6 显式、`real/skills.ts` 1 显式；`create-api.ts` RealDomains 三域接线；parity 更新；PTY env 注入 `ORCINUS_CLI_COMMAND`；feature-interaction catalog 恢复 `computer-use` + ratchet 扫描面扩展 | d4260739 + e1c0008d + 447c60ee + 0ab36d7b |
| `orcinus` CLI | 新 crate `orcinus-cli`（bin `orcinus`，不依赖 tauri）：`status`/`open`/`skills get`（构建期内嵌指南）/`computer` 14 子命令（flag 文案逐字、客户端预校验、`--json` 截图落盘 0600+24h 清扫、失败信封）；`skill-guides/computer-use.md` + `skills/computer-use/SKILL.md`（`ORCINUS_CLI_COMMAND` → PATH，禁回退 `orca`）；URL 仅 computer-use 指本仓库 | 455d5268 + abf04303 + e169f688 + 63f82efc |
| 打包 | `config/scripts/build-orcinus-cli.mjs` → `src-tauri/binaries/orcinus-aarch64-apple-darwin`（gitignore）；`tauri.conf.json` `externalBin: ["binaries/orcinus"]` | c6fb9186 |
| 计划与过程 | 计划 14 任务（TDD）+ T5 结果形状预检修正（对齐 `src/shared/computer-use-permissions-types.ts`）+ T2 resources 布局两次热修 | 646178ee + d0970081 + 66a1b7b6 |

验收口径（spec §6.3）：8 项手工验收（§4，待用户复核）；自动门禁三组全绿（§3）+ T1/T13 专项证据。终态变更规模（净树差异，`git diff --shortstat 3cd628ac..HEAD`）：**94 个文件，+22489 / −18**（含计划/预检/热修 docs；除 docs 外 92 文件 +21877 / −17）。若按各提交 stat 行累计（churn，`git log --numstat` 求和）则为 **+22544 / −73**。

## 2. 提交清单

`git log --oneline --reverse 3cd628ac..HEAD`（spec 提交 `3cd628ac` 为区间下界，不列入；本记录由 Task 14 提交追加，SHA 见提交后 `git log`）：

| 提交 | 内容 |
|---|---|
| 646178ee | docs: Phase 2G computer-use 实施计划（14 任务，TDD） |
| 1ce6e7c4 | feat(computer): vendor Swift helper 改名 Orcinus Computer Use 并改 token 认证（T1） |
| b26f34a0 | fix(computer): helper LSUIElement 对齐 oracle（true）（T1 fix 1） |
| d0970081 | docs(plan): T5 权限结果形状对齐 TS 契约（预检 Ruling） |
| 4e307683 | feat(computer): ade-computer 骨架、协议类型与 helper 路径解析（T2） |
| 66a1b7b6 | fix(computer): resources 目录占位避免空 glob 阻断构建（T2 热修 1） |
| 0d3e96ff | fix(computer): resources 映射记法使 helper 落在 $RESOURCE/computer-use/（T2 热修 2） |
| e8f77a04 | feat(computer): helper transport（spawn/token/NDJSON/重试/超时/重启/terminate）（T3） |
| 2e274913 | fix(computer): 错误信封不再重启 helper（仅 IO/超时失效）（T3 fix 1） |
| 38aa92e2 | feat(computer): provider client、动作校验与 verification 归一化（T4） |
| d8c9558b | feat(computer): TCC 权限探针、授予与重置（T5） |
| 6cd70f17 | feat(computer): runtime RPC server（metadata/socket/信封/分派/feature 记录）（T6） |
| d4260739 | feat(bridge): computerUsePermissions 三命令与 AppState 装配（T7） |
| e1c0008d | feat(bridge): cli 域接真（orcinus PATH 安装/状态/移除）（T8） |
| 447c60ee | feat(bridge): skills.discover 本地最小真实现（home 技能根扫描）（T9） |
| 0ab36d7b | feat: 终端注入 ORCINUS_CLI_COMMAND 与 computer-use 交互 id 恢复（T10） |
| 455d5268 | feat(cli): orcinus CLI 骨架（status/open/skills get）与自有 computer-use skill（T11） |
| abf04303 | fix(cli): keepalive 刷新读超时并保留失败信封 runtimeId（T11 fix 1） |
| e169f688 | feat(cli): orcinus computer 14 子命令与截图落盘（T12） |
| c6fb9186 | build: orcinus CLI externalBin 与 computer-use 打包接线（T13） |
| 63f82efc | fix(skills): computer-use 仅本仓库分发，其余 skill 保持原源（T14 阻断修复） |
| （本提交） | docs: Phase 2G computer-use 实施记录与门禁证据（T14） |

## 3. 门禁证据（自动化）

三组门禁在终态 HEAD（`63f82efc`，分支 `main`，`git status` 干净）顺序执行：

| 门禁 | 命令与工作目录 | 结果 |
|---|---|---|
| Rust 全量 | `cargo test --workspace`（`src-tauri/`） | **exit 0**；56 个 suite（45 test binary + 11 doc-tests）累计 **1113 passed / 0 failed / 0 ignored**；`ade-computer` 56 unit + 集成 46（client 9 / permissions 12 / rpc_server 16 / transport 9）、`orcinus-cli` 70（unit 12 + cli_basics 18 + cli_computer 40）、`ade-bridge` 192 unit + 集成 114（ai_vault 7 / automations 11 / catalogs 12 / gh_exec 12 / git_commands 29 / repos 21 / worktree_lifecycle 22）；`specta_export::tests::bindings_are_fresh` ok |
| bindings 新鲜度 | `cargo run -p ade-bridge --bin export-bindings`（`src-tauri/`）→ `git diff --exit-code -- src/bridge/real/generated/tauri-bindings.ts`（仓库根） | **exit 0 / exit 0**；重生成 `len=93239` 后零差异（注：diff 须在仓库根执行，`src-tauri/` 下相对路径会 exit 128） |
| 类型与构建 | `rm -f tsconfig.tsbuildinfo && pnpm typecheck && pnpm build:web`（仓库根） | **exit 0**；`tsc --noEmit` 零错误；`✓ 11535 modules transformed`、`✓ built in 3.23s`；`externalized` / `MISSING_EXPORT` / `browser compatibility` **0 命中**；7 条信息性 `(!)`（6 条既有动态/静态双导入 + 1 条既有 >500kB chunk-size），无本阶段新增 |
| TS 全量 | `pnpm test`（仓库根，vitest 4.1.11） | **exit 0，全绿**：`Test Files 3879 passed / 8 skipped (3887)`；`Tests 34664 passed / 122 skipped (34786)`；Duration **454.35s**。**本轮无性能抖动失败**（`browser-history-match.performance.test.ts` 未触发，无隔离复跑需要） |

日志留存于本机临时目录（`/tmp/ade-phase2g-cargo-test.log` / `/tmp/ade-phase2g-typecheck.log` / `/tmp/ade-phase2g-build-web.log` / `/tmp/ade-phase2g-pnpm-test.log`）。

### 3.1 T1/T13 专项证据（对应任务时执行；其后无 Swift/打包面变更）

- **T1（`1ce6e7c4` / fix `b26f34a0`）helper 构建 + Swift 测试**
  - `pnpm build:computer-macos` **exit 0**（swift release + Info.plist + 签名；fix 后 `LSUIElement=<true/>` 对齐 oracle，`plutil -lint` OK）；`codesign -dv` → `Identifier=dev.itsuka.orcinus.computer-use`、`Signature=adhoc`；`file` → `Mach-O 64-bit executable arm64`。
  - `swift test`（cwd `src-tauri/native/computer-use-macos`）：**96 绿 / 0 失败**（XCTest 92 + swift-testing 4）；未删除/放宽任何用例。
- **T13（`c6fb9186`）打包验证**
  - `pnpm build:computer-macos && pnpm build:orcinus-cli && APPLE_SIGNING_IDENTITY="Apple Development: 942697184@qq.com (5BKC9569R9)" pnpm tauri build --bundles app` → **exit 0**，`Orcinus.app`（40.04 MiB）。
  - codesign 摘要：sidecar `Identifier=orcinus`、`TeamIdentifier=84D3UFTGFV`、`Authority=Apple Development: 942697184@qq.com (5BKC9569R9)`、`--verify` valid；`.app` `Identifier=dev.itsuka.orcinus`（同 Authority/Team，`--verify --deep --strict` exit 0）；bundle 内 helper `CFBundleIdentifier=dev.itsuka.orcinus.computer-use`（helper 自身仍 ad-hoc，见 §5.2-12）；`Contents/MacOS/orcinus` 存在（Mach-O arm64，0755）。
- 说明：T14 阻断修复 `63f82efc` 仅改 `src/shared` TS，不影响上述 Swift/打包证据面。

## 4. 手工验收清单（待用户复核）

前置：`pnpm build:computer-macos` 产物 + `pnpm dev` 或打包产物；仅 macOS（TCC/AX/ScreenCaptureKit 前提）。

1. `pnpm build:computer-macos` 成功（helper 建立并签名）→ `pnpm dev` 起 app —— **待用户复核**
2. 设置页 Computer Use 显示两权限状态；点授予 → helper 设置窗/系统设置跳转；授权后刷新变 Granted；重置按钮生效（回 not-granted）—— **待用户复核**
3. 面板安装 skill（本仓库源）→ 终端完成 → 检测「已安装」—— **待用户复核**
4. `cli.getInstallStatus` → install → `which orcinus` 命中 symlink；remove 清理 —— **待用户复核**
5. Orcinus 终端内 `orcinus status --json`（reachable:true）；`orcinus skills get computer-use` 输出指南 —— **待用户复核**
6. agent 实操：`orcinus computer list-apps --json`、`get-app-state --app Finder --json`、`click --element-index …`、`type-text`、截图落盘（`path` 非空）—— **待用户复核**
7. app 退出后再跑 computer 命令 → `runtime_unavailable`；`orcinus open` 拉起后恢复 —— **待用户复核**
8. feature-interaction：跑若干 computer 命令后 UI 状态记录含 `computer-use` —— **待用户复核**

任一项失败：补记到本文件 §5 并修复后，重跑对应自动门禁。

## 5. 规格偏差与边界备案

### 5.1 spec §7 十条（逐条备案）

1. **helper 认证放宽**：token 严格匹配（主防线）+ `LOCAL_PEERPID` 同 uid 活性检查替代 Electron 命令路径/bundle ID 判据（Tauri 宿主身份不符的必然改动）；socket 目录 0700 / socket 0600 保持不变。
2. **上游同步**：vendored Swift 与 oracle 上游漂移需手工 cherry-pick（本地 diff 面 = 改名 + 认证 + 文案；内部符号名 `OrcaComputerUseMacOS*` 保留以最小化 diff）。
3. **ad-hoc 签名**：`build:computer-macos` 未导出 `APPLE_SIGNING_IDENTITY` 时 ad-hoc 签 helper，每次构建 CDHash 变化 → 开发期 TCC 可能需重新授权（脚本已打印警告）；真签名路径已备（entitlements 逐字拷贝 oracle）。
4. **门禁不含 helper**：provider 逻辑由进程内 fake helper 覆盖；真实 helper 验证归 §4 手工验收 + T1 `swift test`（96 绿，任务时执行）。
5. **CLI 面裁剪**：无远程/WebSocket/`--environment`/配对、无 `serve`/orchestration envelope、无 `--worktree` 自动解析（仅显式 `--session/--worktree` 透传）；人类输出保证可读但非逐字对标 oracle。
6. **skills.discover 范围**：仅 home 技能根（17 根，无 repo 根/远端）；freshness 类字段最小合法集——面板安装检测够用。
7. **PATH 安装简化**：无 osascript 提权分支（不可写回退 `~/.local/bin`）；不写 shell profile（同 oracle）；非 macOS → `unsupported/platform_not_supported`。
8. **`computer-use` 幂等记录**：RPC 侧按方法名映射记录（`computer.permissions`→`computer-use-setup`；其余 `computer.*` 除 `capabilities`/`permissionsStatus`→`computer-use`）；`feature-interactions.test.ts` ratchet 生产调用方扫描面扩展至 `ade-computer/src` 字面量。
9. **打包依赖手序**：`pnpm build:computer-macos` 是 `tauri build` 前置（文档化）；helper 未构建时 provider 返 `provider_unavailable` + 构建提示而非构建失败。
10. **截图临时目录**：`$ORCINUS_COMPUTER_SCREENSHOT_TMPDIR` 或 `$TMPDIR/orcinus-computer-use/`（不复用 `ORCA_*` 名，避免与原版互踩）。

### 5.2 实施中新发现偏差（brief 清单 1–14 + T14 追加 15–16）

1. **错误码实为 16**（含 `invalid_argument`；TS 目录权威，plan 写 15 有误）。
2. **Tauri resources 用映射记法** `{"resources/computer-use/": "computer-use/"}`（数组记法会带 `resources/` 前缀；T2 两次热修 `66a1b7b6`/`0d3e96ff`）；`externalBin` 使 fresh clone 必须先 `pnpm build:orcinus-cli` 才能 cargo 构建（负面检查错误文案 `resource path 'binaries/orcinus-aarch64-apple-darwin' doesn't exist`，orcinus-app build script exit 101）。
3. **helper Info.plist `LSUIElement=true`**（对齐 oracle；spec 初稿笔误已在 fix commit `b26f34a0` 更正）。
4. **权限 openSetup 的 pkill 采用双模式**（`--permission`/`--permissions`），避免误杀 `--permission-status-file` 探针；reset 无 pre-pkill、tccutil 尽力（无错误通道）。
5. **`getAppState` 动作校验比 oracle 严**（brief 要求）；**不兼容握手不重启 helper**（brief 简化）。
6. **RPC**：未知 `computer.*` → `method_not_found` 且不记录 feature；失败信封含 `_meta`；in-flight 分派不可取消（进程退出兜底）；worker JoinHandle 不回收；`runtime.json` 直接覆写非原子。
7. **`ORCINUS_USER_DATA_PATH` 重定向整个 app 数据目录**（dev/测试定位专用，CLI 对齐；上生产前注意）。
8. **RPC bind 失败中止 app setup**（T6 会先 unlink 陈旧 socket；残余风险为不可写目录/超长 `sun_path`）。
9. **skills.discover**：仅 home 根、depth-1、无 symlink 去重、无 repo 根/远端、静态 TTL 缓存无 home 键、`.system` 未按 root kind 门控、freshness 字段最小合法。
10. **cli 域**：stale 判据按文件名、悬空 symlink 视为 installed、无 legacy 清理（fork 无 legacy）、无管理员提权分支、0o555 测试假设非 root。
11. **CLI**：open 可达信号 = `status.get` 成功（非 desktopWindowStatus）；人类输出无 nextSteps 表；`skills get --json` 的 `_meta.runtimeId` 为 `"none"`；截图落盘 fallback id；`--worktree/--session` 仅显式透传；无 WebSocket/远程/serve。
12. **helper 在 bundle 内为 ad-hoc 签名**（除非 `build:computer-macos` 时导出 `APPLE_SIGNING_IDENTITY`），开发期 TCC 可能需重授权；helper 内部保留少量 `orca-*` 内部标识（`orca-action` assoc key、`com.stablyai.orca.computer-use-*` dispatch label、`ORCA_COMPUTER_USE_SCK_SCREENSHOTS` env、`OrcaComputerUseMacOS*` 模块/测试类名）。
13. **测试基建**：provider 用进程内 fake helper（连接耗尽路径未测）；CLI 用 fake runtime server；SSE 评审通道抖动 → 全任务采用精简+分块评审。
14. **错误码计数**（不重复；上面 1 已含 16 码口径）。
15. **T11 follow-up rulings**：computer-use skill 仅从本仓库分发（`COMPUTER_USE_SKILL_REPOSITORY_URL` = `https://github.com/ItsukaKotori/orcinus`）；其余 skill 保持 `https://github.com/stablyai/orca`（共享常量回退 + 可选 `repositoryUrl` 参数化，commit `63f82efc`）。`CliSkillRuntimeSetup.tsx` 的 win32 update→install fallback 会以默认仓库重取 computer-use——**deferred minor**（Windows 为 fork 不支持平台，死路径；controller ruling 不修）。
16. **T14 阻断经过与修复证据**：首轮 T14 全量 `pnpm test` 红（2 文件 4 测试：OrchestrationPane/linear CTA 断言旧 URL），根因 = T11 `455d5268` 改常量时漏更新这两处断言（隔离复跑确定性复现，非 flake）；修复 commit `63f82efc`（URL 变更 scope 到 computer-use）后本轮全量 **3879 files / 34664 passed / 0 failed**。

## 6. 已知边界与后续

- **Windows/Linux provider**（desktop-script 全族）未做；非 macOS → `unsupported`（本切片 macOS-only）。
- **WebSocket/远程**：`--environment`/配对、`serve`、orchestration envelope 均未接（CLI 裁剪面）。
- **`--worktree` 自动解析**：仅显式 `--session/--worktree` 透传（指南已注明）。
- **人类输出 nextSteps 映射表**：`computer-use-error-recovery` 移植未接（`CliError.data` 结构已预留）。
- **permissions bundleId 解析**：当前为 Info.plist 文本扫描，`PlistBuddy` 真实解析留后续（spec §3.3 原意）。
- **RPC 原子写/取消**：`runtime.json` 非原子覆写、in-flight 不可取消、worker JoinHandle 不回收（§5.2-6）。
- **skills.discover 扩展**：repo 级根、WSL/远端、freshness 真值、install/remove/share（§5.1-6/§5.2-9）。
- **CLI PATH 安装**：管理员提权分支（osascript）与 legacy 清理未做（§5.2-10）；Windows 死路径 minor（§5.2-15）。
- **helper 真签名与上游同步**：`build:computer-macos` 导出 `APPLE_SIGNING_IDENTITY` 做真签名（避免开发期 TCC 重授权）；universal binary；vendored 上游漂移手工 cherry-pick（§5.1-2/3）。
- **ScreenCaptureKit 截图能力验证**：真实 helper 端到端截图/权限行为归 §4 手工验收（自动化门禁不含 helper）。
- **测试增强**：fake helper 连接耗尽路径、CLI stdin 非 TTY 集成测试（TTY 拒绝当前为单测级，T12）。
- **回滚策略**：还原 `src/bridge/create-api.ts` 三域 `RealDomains` 接线为 mock（回 Phase-0 语义）；移除 `tauri.conf.json` `resources`/`externalBin` 即回无 helper/CLI 打包；Rust crates 保留无副作用。
