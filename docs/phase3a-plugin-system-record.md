# Phase 3A 插件系统（双作用域 + 主区插件 Tabs）收尾记录

- 日期：2026-10-11（起始实施）～ 2026-10-13（实施完成 + 终态自动门禁）
- 分支：`main`（spec 基线 `a2c8e021`；计划 `db0ebc4d`；特性/fix 提交 `ec46d5f1`…`7d8914ad`）
- 规格：`docs/superpowers/specs/2026-10-11-plugin-system-design.md`
- 计划：`docs/superpowers/plans/2026-10-11-plugin-system.md`（19 任务；T7→T7a1/T7a2/T7b、T11→T11a/T11b1/T11b2/T11b3、T12→T12a/T12b、T17→T17a/T17b 拆分执行，另含 tab-entry 缺口补缺；Task 19 勾选全部复选框）
- 行为参照：`/Users/itsuka/CodeSpace/orca`（Electron oracle；`orca:` 前缀均为其内路径）
- 方法：subagent-driven-development（每任务 TDD 实现 + 评审 + fix 轮；终态三组门禁 + bindings 新鲜度 + 本记录）
- 手工验收状态：**待用户复核（2026-10-13 自动化门禁已全绿；8 项手工清单见 §4）**

## 1. 范围与验收

| 面 | 内容 | 主提交 |
|---|---|---|
| `ade-plugins` 基础 | manifest（`contributes` 全族群 + tabs ≤64、保留名/越界/上限校验）、content_hash（`orca-plugin-tree-v1\0` + 4B BE 帧 + 排序路径，symlink 拒/跳 `.git`）、capability 目录（7 + `workspace:open-tab`）、discovery（三根扫描、dev/项目遮罩、重 key invalid）、artifact/paths 安全（realpath containment + 单类上限：tab 10MiB 等） | ec46d5f1 + bdc67e56 |
| 安装管线 | staging（过滤/复算/reparse）、atomic_write 重试、pointer（≤128B）、provenance（0600）、lockfile（legacy `capabilityHash` 回读、git URL 白名单）、publication（provenance→指针→lockfile 回滚序）、rollback/卸载、`.ade/.gitignore` deny 写入 | 8a71b2c6 |
| consent/启用/完整性 | 指纹逐字移植 oracle（capabilities 规范化 + `\0trusted-node-worker` + instructional + 内容键）；三态激活 + needsReconsent；approve 指纹竞态拒绝（`consent_stale`）；`verify_installed` 篡改检测（`integrity_mismatch`） | 70468e9e |
| storage/settings/audit/secrets | per-plugin `storage.json`/`settings.json`（0600、保留键、256KiB/5MiB/1024 keys）、audit JSONL（10MiB 轮转）、secrets 经 `SecretStore` trait（Keychain service `dev.itsuka.orcinus.plugin-secrets`、fail-closed `unavailable`） | 77e913ad |
| worker 宿主与池化 | NDJSON 帧（≤1MiB、error ≤8192 截断）、`host-entry.mjs`（file-URL import/SDK/ready/deactivate/fatal）、spawn（node 解析/env 白名单/进程组 SIGTERM→2s→SIGKILL）、supervisor（ready 10s/invoke 30s/event 5min/idle 5min/重启≤3 backoff[500,2000,5000]ms）、manager（≤5 slot、revision 重启、未声明命令→`unknown_command`+deactivate+errored） | 317c4c8a + 6318df27 + 55ee5101 + 10fba29e + 1c3a7982 |
| host gate + methods | 8 capability × panel/worker 判定（`unknown_method/panel_forbidden/consent_required/capability_denied`）+ 14 方法（13 照 oracle 表 + `workspace.openTab`）参数/结果双端校验、mutation audit 顺序、worktree/终端白名单、openTab 声明校验/去重/限额钩子 | a4c21bbc |
| 会话注册表 | Panel/MainTab/GlobalView 绑定（scope/repo/worktree/revision + owner）；CSPRNG 32B base64url token；owner/插件/禁用失效 | 4e192d6b |
| git runner + 市场 | `GitRunner`（argv 无 shell、超时 kill+reap、URL 白名单）、源管理 + snapshot（`plugins-data/marketplaces/`）、官方种子与 trust 三重校验、preview（固定 commit）/install commit 漂移拒绝/rollback | 329c6b9b |
| watcher/events/service | dev watcher（notify、150/500ms 批处理）、`PluginEventName` 3 值投影、service 编排（投影/安装/consent/激活/lazy/事件投递/项目生命周期/lockfile/批量安装/面板 entry/action/全局视图/主区 tab 打开与去重限额、per-key 变更锁） | 886fa3d0 + 0d5bf436 + 3abc6944 + 1fe834bb + 86458344 |
| bridge | `PluginService` 装配（settings consent/disable/devPluginPaths、PTY/notifications/ui-state/ProjectsStore 注入）、`PluginHostServices` 实现、22 命令 + `plugins:changed`/`plugins:open-tab`、bindings、集成测试；项目作用域 host 存储/设置隔离；市场安装 scope 与 `InstallResult.resolvedCommit` 跨任务修复 | 1dc4ba60 + 7e888fbd + f0276d6a + 116c5957 |
| TS 契约 + mock | `PluginHostListEntry` scope/repoId/tabs、`PluginRef` 入参、`PluginHostInstallSource` scope、lock 状态类型、panel action 集合 + `workspace.openTab`、capability 联合 + `workspace:open-tab`、`onOpenTab`；mock fixtures/方法同步 | b9fa03cd |
| TS real 域 | `real/plugins.ts` 23 方法 + `onChanged`/`onOpenTab` 订阅（24 契约成员）；`RealDomains` 接线 + parity 更新 | 6b4f234b |
| 渲染层 | 安装作用域选择/consent 适配/行徽标/右栏过滤（T15）；PluginRef 定位修复；lockfile banner + 全局插件视图（T16）；主区插件 tab 模型/store/onOpenTab 桥（T17a）、渲染/新建菜单（T17b）、reconcile/激活面/复制/键序/调色板修复；市场与开发中 tab（T18）；`plugins_read_tab_entry` 缺口补缺 | d12ce52e + 32aa3da4 + 1b677aae + 69b2ab8e + 557937cd + c527bfe3 + fb3843e6 + d92bdf1a + 7d8914ad |
| 计划与过程 | 19 任务计划 + 四次拆分裂（T7/T11/T12/T17）+ tab-entry 补缺（spec §3.6 缺口）；SSE 评审抖动导致多次重派，实现本身无影响 | db0ebc4d |

验收口径（spec §6.3）：8 项手工验收（§4，待用户复核）；自动门禁三组全绿（§3）+ bindings 新鲜度。终态变更规模（净树差异，`git diff --shortstat a2c8e021..HEAD`）：**174 个文件，+35909 / −466**（含计划 docs；本记录由 Task 19 提交追加，SHA 见提交后 `git log`）。churn（`git log --numstat` 求和，排除二进制）**+36411 / −968**。

## 2. 提交清单

`git log --oneline --reverse a2c8e021..HEAD`（spec 提交 `a2c8e021` 为区间下界，不列入；本记录由 Task 19 提交追加，SHA 见提交后 `git log`）：

| 提交 | 内容 |
|---|---|
| db0ebc4d | docs: Phase 3A 插件系统实施计划（19 任务） |
| ec46d5f1 | feat(plugins): ade-plugins 骨架、manifest 与内容哈希（T1） |
| bdc67e56 | feat(plugins): 发现、产物校验与路径安全（T2） |
| 8a71b2c6 | feat(plugins): 安装管线（staging/发布/lockfile/回滚/卸载）（T3） |
| 70468e9e | feat(plugins): consent 指纹、启用与完整性校验（T4） |
| 77e913ad | feat(plugins): 存储、插件设置、审计与 Keychain secrets（T5） |
| 317c4c8a | feat(plugins): worker 帧协议与 host-entry.mjs 宿主（T6） |
| 6318df27 | feat(plugins): worker spawn（node 解析/环境白名单/进程组终止）（T7a1） |
| 55ee5101 | feat(plugins): worker supervisor（状态机/超时/重启退避）（T7a2） |
| 10fba29e | feat(plugins): worker manager（池化/命令/事件/hostCall 回写）（T7b） |
| 1c3a7982 | fix(plugins): 未声明命令改由 ready 列表判定（T7b fix 1） |
| a4c21bbc | feat(plugins): host gate 与 14 个 host methods（T8） |
| 4e192d6b | feat(plugins): 面板/主区 tab 会话注册表（T9） |
| 329c6b9b | feat(plugins): git runner 与 git 源市场（T10） |
| 886fa3d0 | feat(plugins): dev watcher 与插件事件投影（T11a） |
| 0d5bf436 | feat(plugins): service 核心（投影/安装/consent/lockfile 与变更锁）（T11b1） |
| 3abc6944 | feat(plugins): 激活编排、命令/事件与热重载（T11b2） |
| 1fe834bb | fix(plugins): worker 身份按作用域实例键区分（T11b2 fix 1） |
| 86458344 | feat(plugins): 面板 entry/action、全局视图与主区 tab 打开（去重/限额）（T11b3） |
| b9fa03cd | feat(plugins): 契约扩展（scope/tabs/lock/open-tab）与 mock 同步（T13） |
| 1dc4ba60 | feat(bridge): 插件服务装配与宿主服务实现（T12a） |
| 7e888fbd | fix(bridge): 项目作用域 host 存储/设置隔离（T12a fix 1） |
| f0276d6a | feat(bridge): plugins 22 命令与 bindings（T12b） |
| 116c5957 | fix(plugins): 市场安装选择作用域与 InstallResult resolvedCommit（T12b 跨任务修复） |
| 6b4f234b | feat(plugins): real 域接线与 parity（T14） |
| d12ce52e | feat(plugins-ui): 作用域真数据、安装选择与 consent 适配（T15） |
| 32aa3da4 | fix(plugins-ui): consent 目标按 PluginRef 定位（T15 fix 1） |
| 1b677aae | feat(plugins-ui): lockfile 提示与全局插件视图接真（T16） |
| 69b2ab8e | feat(plugins): 主区 tab entry 面（crate/命令/契约/real）（缺口补缺，T17 前置） |
| 557937cd | feat(plugins-ui): 插件 tab 模型、store 与 onOpenTab 桥（T17a） |
| c527bfe3 | fix(plugins-ui): 插件 tab 的 reconcile/激活面/复制与 web 映射（T17a fix 1） |
| fb3843e6 | feat(plugins-ui): 主区插件 tab 渲染与新建菜单（T17b） |
| d92bdf1a | fix(plugins-ui): 插件 tab 进入 tab 条/键盘顺序/调色板（T17b fix 1） |
| 7d8914ad | feat(plugins-ui): 市场与开发中 tab 接真（T18） |
| （本提交） | docs: Phase 3A 插件系统实施记录与门禁证据（T19） |

## 3. 门禁证据（自动化）

三组门禁在终态 HEAD（`7d8914ad`，分支 `main`，工作树干净）执行：

| 门禁 | 命令与工作目录 | 结果 |
|---|---|---|
| Rust 全量 | `cargo test --workspace`（`src-tauri/`） | **exit 0**；61 个 suite（49 test binary + 12 doc-tests）累计 **1482 passed / 0 failed / 0 ignored**；`ade-plugins` 345（unit 343 + host_entry 集成 2）、`ade-bridge` 193 unit + 集成 137（ai_vault 7 / automations 11 / catalogs 12 / gh_exec 12 / git_commands 29 / plugins 11 / plugins_service 12 / repos 21 / worktree_lifecycle 22）、`ade-computer` 56 unit + 集成 46（client 9 / permissions 12 / rpc_server 16 / transport 9）、`orcinus-cli` 70（unit 12 + cli_basics 18 + cli_computer 40）；`specta_export::tests::bindings_are_fresh` ok（另有 `command_inventory_and_bindings_are_fresh` ok） |
| bindings 新鲜度 | `cargo run -p ade-bridge --bin export-bindings`（`src-tauri/`）→ `git diff --exit-code -- src/bridge/real/generated/tauri-bindings.ts`（仓库根） | **exit 0 / exit 0**；重生成 `len=114245` 后零差异（注：diff 须在仓库根执行，`src-tauri/` 下相对路径会 exit 128） |
| 类型与构建 | `rm -f tsconfig.tsbuildinfo && pnpm typecheck && pnpm build:web`（仓库根） | **exit 0**；`tsc --noEmit -p tsconfig.json` 零错误；`✓ 11546 modules transformed`、`✓ built in 3.49s`；`externalized` / `MISSING_EXPORT` / `browser compatibility` **0 命中**；7 条信息性 `(!)`（既有动态/静态双导入与大 chunk 提示），无本阶段新增阻断 |
| TS 全量 | `pnpm test`（仓库根，vitest 4.1.11） | **exit 0，全绿**：`Test Files 3893 passed / 8 skipped (3901)`；`Tests 34772 passed / 122 skipped (34894)`；Duration **514.99s**。node 存在（v22.21.0）故 node 依赖用例全部实跑，无 SKIP 计通过路径 |

**已知 flake 记录**：首轮 `cargo test --workspace` 在 `ade-bridge` 的 `gh_exec_kills_process_group_on_timeout` 失败（`tests/gh_exec.rs:95` ENOENT，base 已报告 flaky）；隔离子集复跑 **1 passed / 0 failed**；随后整轮重跑 **exit 0**（上表数字取自该绿轮，失败轮不影响其余 suite 判定——cargo 在失败后停止，故未计入）。本文件所有门禁数字均为绿轮实测。

日志留存于本机临时目录（`/tmp/ade-phase3a-cargo-test.log` / `/tmp/ade-phase3a-typecheck.log` / `/tmp/ade-phase3a-build-web.log` / `/tmp/ade-phase3a-pnpm-test.log`）。

## 4. 手工验收清单（待用户复核）

前置：`pnpm dev` 或打包产物；系统 `node>=18`；macOS（Keychain/POSIX）。

1. local 安装一个测试插件（带 panel+tab+command）→ consent → 启用 → 右侧栏面板可交互（readContext/sendText/notifications）—— **待用户复核**
2. 面板按钮 `workspace.openTab` → 主区新 tab（带参数），重复打开聚焦既有实例；「+」菜单可手动打开 —— **待用户复核**
3. git 源安装 + 更新 + 回滚 —— **待用户复核**
4. 项目级安装 → `<repo>/.ade/plugins.lock.json` 提交、`.ade/.gitignore` deny；模拟新克隆（删 `.ade/plugins`）→ 插件中心提示 → 一键安装 —— **待用户复核**
5. 市场：添加自定义 git 源→刷新→浏览→preview→安装→回滚 —— **待用户复核**
6. 开发中：设置 dev 路径→开发中 tab 出现→改插件文件→worker 热重载 —— **待用户复核**
7. secrets 写入/读取（Keychain）→ 重启用仍可读 —— **待用户复核**
8. 禁用/卸载→worker 终止、面板/tab 不可用态、数据目录与 lock 条目清理 —— **待用户复核**

任一项失败：补记到本文件 §5 并修复后，重跑对应自动门禁。

## 5. 规格偏差与边界备案

### 5.1 spec §7 十二项（逐条备案）

1. **Node 运行时依赖**：worker 需系统 `node >= 18`（不内置；缺失 → errored + 提示）；插件生态按 Node ESM 语义。node 解析仅校 major（§5.2-7）。
2. **传输层偏离**：NDJSON stdio 替代 Node `fork` IPC（帧模型不变；helper 同模式先例）；`plugin-host-conformance` relay 半边不移植。
3. **kill list 跳过**：无远程吊销；consent/哈希/惰性完整性保留（记录安全差距 + 后续项）。
4. **bundled 跳过**：不随包分发插件；schema 仍识别（lockfile `Bundled` 变体保留）。
5. **vmRecipes/agents 保留不消费**：schema/artifact/指纹保留兼容（consent 预览仍展示）；执行与消费为后续项。
6. **项目作用域为 fork 新面**：oracle 无参照；遮罩/独立 consent/lockfile 语义按 spec §7.1–7.3 设计实现；「项目打开/关闭」以 ProjectsStore 注册集合近似（注销即终止 worker/会话，见 §5.2-2/11）。
7. **主区 Tabs 为 fork 新面**：manifest `contributes.tabs`、`workspace:open-tab` capability、`workspace.openTab`、`orca-panel-init` 均无 oracle 参照；扩展面全部记录进契约与指纹。补缺 `plugins_read_tab_entry`（§5.2-17）。
8. **Keychain**：ad-hoc 签名开发期重建可能触发授权弹窗（真签名缓解）；fail-closed 语义同 oracle；自动化用 fake keychain（§5.2-19）。
9. **市场为 git-only**：无 HTTP API/私有认证；官方 trust 三重校验照搬；自定义源第一类；测试全用本地临时仓库（§5.2-19）。
10. **host-entry.mjs 上游同步**：手工 cherry-pick（fork 适配很小）。
11. **性能**：发现并发 8、哈希 2k 文件上限、watcher 批处理；大插件树首装耗时披露。
12. **web 端维持无插件面**：composition 测试锁定（T17a fix 补 web 映射）。

### 5.2 实施中新发现偏差（20 项）

1. **T1**：keybinding strict/normalize 未移植；null 字段比 oracle 严（`reject_nulls`）；key 冲突去重未做。
2. **T2**：engine gate 于 T11 过滤（已闭环）；项目遮罩按存在性（broken project 仍遮罩）；dev 重 key 次者 invalid（spec §3.8）。
3. **T3**：provenance-recovery 由 T11 承接（已闭环）；空表卸载仍落盘；provenance 目录 0755（文件 0600）。
4. **T4**：新错误码 `manifest_mismatch`（UI 需容忍）；approve 未含审批时教学内容复验（激活 fail-closed）。
5. **T5**：secrets 64KiB 按字节（比 oracle UTF-16 严）；audit 目录 0755。
6. **T6**：8192 截断；插件直写 stdout 可破坏帧（console 重定向缓解）；无 node 时测试 SKIP 计通过。
7. **T7（拆 T7a1/T7a2/T7b）**：`node_supports` 仅校 major；`unknown_command`（未声明命令）新语义；确定性 spawn 错误也走退避。
8. **T8**：`HostCallContext` 增 scope/repo_id（additive）；secrets/字节限额同上。
9. **T9**：会话 token 为裸 String；revision 校验在调用方。
10. **T10**：git URL 空白 trim 后校验但存储原始；超时仅杀直接子进程（同 ade-git 既有模式）。
11. **T11（拆 T11a/T11b1/T11b2/T11b3）**：worker 实例键（`global:`/`project:` 前缀）为修复引入；`remove()` 裸 key 失效他作用域会话（记录性）；事件常量重复定义；截断按 chars vs UTF-16。
12. **T12（拆 T12a/T12b）**：`disabled_system` 经 T14 映射为 panel outcome `unavailable`；市场安装 scope 经跨任务修复补齐；`InstallResult.resolvedCommit` 补齐；legacy preload 未加新方法（tsconfig 排除）。
13. **T13/T14**：契约成员 24（22 命令 + 2 订阅），plan 文中 23 为笔误。
14. **T15**：consent 目标按 PluginRef（修复引入）；reloadAfterMutation/remove 仍 key-only find（记录性）。
15. **T16**：lockfile banner 不订阅 onChanged（重挂载前可能陈旧）；批次逐条失败仅 console。
16. **T17（拆 T17a/T17b）**：修复引入 reconcile/激活面/条/键序/调色板支持；Cmd+Shift+T 重开为 no-op（表意）；复制保留 pluginTab。
17. **补缺**：`plugins_read_tab_entry` 为 spec §3.6 缺口新增（crate/命令/契约/real 全链）。
18. **T18**：dev 单条无启用/重载按钮（§3.8 后续）；更新继承已装作用域；新 i18n 键为内联缺省（与 T15–T17 一致）。
19. **测试基建**：node 缺失路径 SKIP；marketplace/git 测试全用本地临时仓库；secrets 测试用 fake keychain（真 Keychain 属手工验收）。
20. **SSE 抖动**：多次分派超时重试与任务拆分（T7/T11/T12/T17 拆分，评审重派），无代码影响。

## 6. 已知边界与后续

- **Windows/Linux**：当前 POSIX-only（路径/进程组/Keychain/通知语义）；平台门控与替代实现为后续。
- **远程 kill list**：无远程吊销通道；consent/哈希/惰性完整性保留（spec §7-3 安全差距记录）。
- **bundled 插件**：不随包分发；schema 识别保留（spec §7-4）。
- **dev 单条启用/重载**：开发中 tab 当前为列表 + 全局刷新，单条操作按钮后续（§5.2-18）。
- **Cmd+Shift+T 重开插件 tab**：当前 no-op（表意），最近关闭插件 tab 的重开后续（§5.2-16）。
- **lockfile banner onChanged 订阅**：当前不订阅，重挂载前可能陈旧（§5.2-15）。
- **marketplace 官方源实网验证**：自动化全用本地临时仓库；官方种子与 trust 三重校验留实网手工。
- **node 内置化**：依赖系统 `node>=18`；缺失 → errored + 提示（spec §7-1）。
- **vmRecipes/agents 消费**：schema/artifact/指纹保留兼容，执行与消费后续（spec §7-5）。
- **上游同步**：host-entry.mjs/协议/指纹移植面随 oracle 漂移手工 cherry-pick 对齐（spec §7-10）。
