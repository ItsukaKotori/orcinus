# Phase 3B 功能删减记录（Orcinus 第二轮瘦身：聚焦本地编码）

- 分支：`phase3b-trim-local-focus`（分支点 `main` @ `415593ce`；前置文档提交 `639e6de7` 规格、`f4092bc8` 计划）
- 日期：2026-10-13 启动，2026-10-17 收尾（macOS）
- 范围：从渲染层 + 宿主彻底删除 7 个功能域 —— 无后端死面（browser/Linear/Jira/GitLab/SSH/远程主机死树/Electron 遗留）、Web 远程客户端（含 pairing/relay 配角）、GitHub 集成全链（含只读 checks 与 gh 后端）、Automations（含 `ade-automations` crate）、Computer-use（含 `ade-computer` crate 与 Swift 包）、Onboarding、Artifacts；加杂项（AI Vault 死子面、spikes 构建产物、i18n/依赖残留）与全局收尾
- 依据：`docs/superpowers/specs/2026-10-13-local-focus-trim-design.md`、`docs/superpowers/plans/2026-10-13-phase3b-local-focus-trim.md`；主规格 `docs/superpowers/specs/2026-09-14-ade-design.md` §2 已同步更新
- 后续 backlog：`docs/phase0-dead-code-inventory.md`（未随本轮的残留见本记录「残留表」）

## 方法与证据链

1. **生产可达性分析**（Task 0 工具 `orcinus-reach.mjs`，本轮固化于 `/tmp`，未入提交）：入口 = `src/renderer/index.html → main.tsx`（Web 客户端删除后收敛为单入口；`src/main`、`src/preload` 亦不在图内，随域删除），解析相对导入、`@/`、`@renderer/` 别名与 lazy `import()` 字面量；测试文件不纳入可达性图。
2. **基线**（Task 0）：可达性基线 `/tmp/orcinus-reach-baseline.txt`（435 行不可达）；测试基线 `pnpm test` **全绿**：3893 文件 / 34772 测试通过（8 文件 / 122 跳过，~355s，`/tmp/orcinus-test-baseline.log`），失败与 collection 清单为空文件。
3. **删除规则**：沿 Phase 1 —— 域内文件整域 `git rm`，逐批冷门禁；reach 新增孤儿逐个判定（删或记录理由）；`*.d.ts` 不作删除候选；旧会话数据（SQLite automations 表）只删生产者不迁移。
4. **门禁**：每批删除后 `rm -f tsconfig.tsbuildinfo && pnpm typecheck && pnpm build:web` 必须 exit 0；Rust 批次额外 `cargo check/test` + bindings 重生成 + parity 锁定数同步。
5. **i18n**：`orcinus-domain-keys.sh` 收集候选 → `orcinus-i18n-prune.mjs` 仅删「catalog 存在 + 剩余源码无完整字面量 + 父路径无 `${}` 拼接」的键；Task 8 做已删域最后一轮 sweep（含守卫回滚机制）。
6. **提交策略**：每域一个提交（审查修复轮 amend 入原提交），Task 8 一个收尾提交 + 一个文档提交；全程不 merge/push，每提交可独立 revert。最终 `git log --oneline 415593ce..HEAD` = 2 个前置文档提交 + 8 个功能/收尾提交 + 1 个本期记录文档提交。

> 说明：`.superpowers/sdd/2026-10-13-phase3b-local-focus-trim/`（工具、基线、逐步报告、ledger）为本地未跟踪证据（`.superpowers/` 已 gitignore），不是提交产物。

## 提交与门禁表

| 域 | 提交 | 变更文件 | 删除文件（diff-filter=D） | 门禁 |
|---|---|---|---|---|
| 1 无后端死面（browser/Linear/Jira/GitLab/SSH/远程主机死树/Electron 遗留） | `5c845a01` | 2096（+2821/−213894） | 1263 | 冷门禁通过（typecheck/build:web）；守卫/广扫全绿；reach 新增孤儿仅 secret-store（裁定保留） |
| 2 Web 远程客户端（含 pairing/relay 配角与依赖） | `37a3b69b` | 169（+268/−18891） | 128 | 冷门禁通过；deferred 3 项修复 round 后全绿 |
| 3 GitHub 集成全链（含只读 checks 与 gh 后端） | `039e6677` | 1310（+1330/−159457） | 910 | TS 删除 → Rust gh 域删除 + bindings 重生成（−20 行）→ parity 489 绿 → 冷门禁通过 |
| 4 Automations（含 `ade-automations` crate） | `f5f8bc97` | 460（+277/−66977） | 383 | cargo + bindings（−137）+ parity + 渲染层 2499 文件全绿；SQLite 表留存（只删生产者） |
| 5 Computer-use（含 `ade-computer` crate、Swift 包、技能） | `38bd6e8b` | 117（+181/−20474） | 68 | cargo 312/312 + bindings（−73）+ parity 465 + 守卫 613 全绿；`orcinus` CLI 诚实降级 |
| 6 Onboarding 向导（含契约与测试） | `5e6bdd10` | 70（+111/−5395） | 39 | 守卫 604 + 受影响面 1687 文件 16100 测试全绿；首启直入主界面（Landing 兜底） |
| 7 Artifacts（含设置项与测试） | `db0795a3` | 83（+61/−5218） | 45 | 域内门禁通过；由 Task 8 全量复核兜底 |
| 8 杂项 + 全局收尾 | `8fa44059` | 29（+447/−4117） | 13 | 冷 typecheck/build:web exit 0；全量测试零失败（见下）；reach 新增孤儿 11 全部裁定保留/记录；i18n 守卫 139 绿 |

分支累计（`415593ce..8fa44059`）：**3829 个文件变更，+5486/−493344，2849 个文件删除**。

各域提交 stat：

```
5c845a01  2096 files changed,     2821 insertions,  213894 deletions
37a3b69b   169 files changed,      268 insertions,   18891 deletions
039e6677  1310 files changed,     1330 insertions,  159457 deletions
f5f8bc97   460 files changed,      277 insertions,   66977 deletions
38bd6e8b   117 files changed,      181 insertions,   20474 deletions
5e6bdd10    70 files changed,      111 insertions,    5395 deletions
db0795a3    83 files changed,       61 insertions,    5218 deletions
8fa44059    29 files changed,      447 insertions,    4117 deletions
```

## AI Vault 死子面清理（Task 8 Step 1）

零 UX 损失的死 RPC 面（宿主无实现、调用即 reject），删除 13 个文件（`git rm`）：

- 搜索面 9 文件：`shared/ai-vault-search-{client,contract,query-operators,transport,types,limit,test-fixture}.ts` + 2 个测试；
- 子代理列表：`AiVaultSessionSubagents.tsx` + 测试；First Prompt 卡：`ai-vault-first-prompt-card.tsx` + 测试。

关联改动：`real/ai-vault.ts`（剩五命令 + 焦点事件，注释同步）；`AiVaultApi` 契约删 `searchSessions`/`searchStatus`/`listSubagentSessions`/`getFirstUserPrompt`（parity 锁定数 4 → 0 同步，`missing: []`）；`AiVaultSessionDetails.tsx` 删两处引用；`ai-vault-types.ts` 删服务的 4 个死类型；`ai-vault-session-filters.ts` 将活函数 `splitAiVaultSearchQuery` 内联（本地过滤 `repo:`/`path:` 语义原样保留），其测试用例迁移进 `ai-vault-session-filters.test.ts`；Row 测试清除死 API stub。冷门禁 + `pnpm test src/bridge src/renderer/src/i18n` + right-sidebar 全绿。

## spikes 构建产物（Task 8 Step 2）

`spikes/cef-embed/target/` 已被 gitignore（`git check-ignore` 命中）、零追踪文件，仅本地 `rm -rf`（释放 ~1.1GB），不进提交。

## 全局可达性兜底（Task 8 Step 6）

- 最终：`reachable=4412 unreachable=372`（基线 435）。基线不可达中 74 个随本删减被删、361 个为基线既有死代码（非目标）。
- **新增孤儿 11 个**（vs `/tmp/orcinus-reach-baseline.txt`，`LC_ALL=C comm -13`）：全部逐个裁定为**保留**（沿 Phase 1「测试支持的死树先保留」先例 + Task 1 对 secret-store 的预裁定），无新增实删项：

| 新增孤儿 | 保留理由 |
|---|---|
| `components/ui/sheet.tsx` | 预裁定保留；被 `MarkdownImageLightbox.test.tsx` 使用 |
| `shared/secret-store.ts` | 预裁定保留；`config/scripts/vitest-host-ports-setup.ts` 使用 |
| `shared/abort-signal-reason.ts` | 被 `runtime-rpc-call-queue`/`git-fetch-head-lock`（同批不可达）与队列测试传递覆盖 |
| `shared/runtime-rpc-call-queue.ts` | 自身 + 远程 runtime 集成测试覆盖 |
| `shared/remote-runtime-tailscale-hint.ts` | 自身 + 远程 runtime 集成测试覆盖 |
| `shared/tailnet-address.ts` | 自身测试覆盖 |
| `shared/reconnect-jitter.ts` | 自身测试覆盖 |
| `shared/runtime-host-status-owner.ts` | 自身测试覆盖 |
| `shared/workspace-cleanup-ui-state.ts` | 自身测试覆盖 |
| `shared/osc52-clipboard-settings.ts` | 自身测试覆盖；`default-global-settings.ts` 注释引用 |
| `shared/terminal-cursor-style-settings.ts` | `terminal-cursor-appearance-precedence.test.ts` 覆盖 |

> 偏差说明：计划期望新增孤儿 = 0；实际为 11（任务 1–7 各自报「0 新孤儿」是以各自开工点为参照的局部对比，跨任务累积至收尾才在全量对比中显形）。全部为测试支持的指针型工具模块，删除会连带删测试并牵动远程 runtime 集成测试，故按先例保留并记录。

## i18n 清键统计（Task 8 Step 3）

候选 = 8 份域键清单 ∪ 已删域目录叶扫（artifacts/automations/browser/linear/jira/gitlab/github/ssh/onboarding 等 270 键）∪ `menu.showTasksButton`，共 4384 条。`orcinus-i18n-prune.mjs --apply` 结果：

```
notInCatalog: 3829   （历史清单中已被前序任务删除/从未入 catalog）
referenced:   177    （剩余源码仍有完整字面量，保留）
dynamic:        0
deleted:      378    （守卫回滚 4 键后净删，见下）
```

守卫回滚：`settings-status-label-localization.test.ts`（位于 `i18n/` 目录、被 ref-check 按设计排除）锁定 4 个 CLI 集成卡状态键（`...cli.source.control.integration.cards.status{Connected,Unavailable,NotInstalled,NotAuthenticated}`），首次误删导致守卫 4 failed → 从 HEAD 恢复（en + fr 各 4 键；es/ja/ko/zh 原本即无），守卫 139 全绿。

各 catalog 叶子净变化（对比 HEAD，脚本按 key-path 集合）：

| catalog | 删除叶子 | 新增叶子 |
|---|---|---|
| en.json | 374 | 0 |
| es.json | 314 | 0 |
| fr.json | 344 | 0 |
| ja.json | 314 | 0 |
| ko.json | 314 | 0 |
| zh.json | 314 | 0 |
| en-runtime-required.json | 167 | 0 |
| 合计 | **2141** | **0** |

明细：Step 1 死子面 27 键（含 First Prompt 卡 12 键、子代理 3 键）、Task 4 遗留 `auto.components.automations.*` 44 键（其余语言 40）、Task 7 遗留 `auto.components.settings.artifacts.*`、`menu.showTasksButton`、`auto.store.slices.github.*` 等已删域键。`noPrLinked`、`localhostWorktreeLabels` 等前序未披露残留均已在历史轮清除（复核为 0）。

未做（记录在案）：空命名空间壳未清（叶删除后遗留 `{}` 父级，en 236 / es 152 / fr 177 / ja 157 / ko 158 / zh 152，含整棵 `auto.components.automations.*` 空壳树）；全目录粗扫另有约 4000 个无完整字面量的叶，超出「已删域 sweep」范围且存在动态模板误删风险（本次唯一生产动态 `translate(\`${NS}.…\`)` 已单独核验保留），不纳入本轮。

## 依赖清理（Task 8 Step 4）

| 依赖 | 非测试命中 | manifest | 判定 |
|---|---|---|---|
| `qrcode` | 0 | 已不在 package.json | Task 2 已删，复核确认 |
| `tweetnacl` | 0 | 已不在 package.json | 同上 |
| `ws` | 0 | 已不在 package.json | 同上 |
| `@types/qrcode` / `@types/ws` | 0 | 已不在 package.json | 同上（Task 2 fix round） |

`pnpm install --lockfile-only` 后 `pnpm-lock.yaml` 零 diff：依赖与锁文件一致，无残留。

## ratchet 重基线（Task 8 Step 5）

本轮 Task 0 基线**全绿**（无既有红，与 Phase 1 不同）。收尾实测 46 个 ratchet/边界/预算/清单类测试文件 + `windows-console-input.test.ts`：

- `pnpm test <46 files>`：**46 文件 / 281 测试全过**；`windows-console-input`：5/5 过。
- `child-process-import-boundary` ratchet 已随 child-process 域（Task 1）整体删除，无需转绿；`terminal-pane-store-subscription-budget` 通过。
- pin 清单无需更新（无域删除导致 ratchet 失败）。

## 全量测试基线对比（Task 8 Step 7）

```
pnpm test 2>&1 | tee /tmp/orcinus-test-final.log    # exit=0
 Test Files  2988 passed | 4 skipped (2992)
      Tests  26911 passed | 51 skipped (26962)
  Duration  374.87s
```

- 对比 Task 0 基线（3893 文件 / 34772 测试通过，8/122 跳过）：文件 −905、测试 −7861，全部为已删域的测试（7 域 + AI Vault 死子面）。
- 失败集合差（`LC_ALL=C comm -13`）：`/tmp/orcinus-test-final-failures.txt` **0 行**（新增失败 = 0）；collection 清单 **0 行**（新增 = 0）。基线失败清单本身为空，即最终全绿。
- 已知无害噪音：日志含一行 `set: --: invalid option`（Task 0 基线同样存在，vitest 包装脚本产物）。

## 自动启动冒烟（Task 8 Step 8）

人工 GUI 冒烟无法由实施代理执行，以有界自动启动检查替代，证据 `/tmp/orcinus-dev.log`：

```
ROLLDOWN-VITE v7.3.1  ready in 216 ms
Finished `dev` profile [unoptimized + debuginfo] target(s) in 3m 21s   # 本轮冷编译
Running `target/debug/orcinus-app`
```

日志无编译错误、无 panic、无启动报错（error 命中均为 crate 名 `thiserror`）；确认应用进程存活后 kill，端口 1420 已释放。

### 人工冒烟清单（交用户执行）

1. `pnpm dev` 启动，应用无报错；
2. 终端可开 tab 并执行命令；
3. 侧栏 worktree 列表可切换；
4. 编辑器可打开文件，diff 批注可用；
5. agent 在终端启动，右栏 Agents（Vault）列表与 resume 可用；
6. 插件中心可达；
7. 设置页各分组可达；
8. 以下入口不存在：browser tab 类型、Linear/Jira/GitLab/GitHub 设置与面板、Automations 侧栏按钮、ComputerUse 设置项、Onboarding 首启向导、Artifacts 按钮、Web 客户端入口。

## 残留表（未删的孤儿/残留与原因）

| 残留 | 数量/位置 | 原因 |
|---|---|---|
| **Runtime RPC 观测 descope**（Task 5，controller 裁定） | `computer_service.rs` 移除删除了唯一 `runtime.json` 写入者；`orcinus status` 不再 RPC 观测、`open` 仅启动后 exit 0、`help` 不承诺可达性 | 不做新功能，按 trim 精神删除死等待链（CLI 诚实降级优于「永远超时」）；若未来要恢复运行时观测需另起特性 |
| **Rust host-dead onboarding 面**（Task 6，controller 裁定） | `commands/onboarding.rs`、`OnboardingStore`、specta 注册、bindings `onboardingGet/Update` | 宿主契约仍消费持久化 onboarding 状态；只删渲染面 |
| 新增 reach 孤儿 | 11 个（见「全局可达性兜底」表） | 测试支持的死树/预裁定保留，先于删除 |
| 空 i18n 命名空间壳 | en 236 / es 152 / fr 177 / ja 157 / ko 158 / zh 152（含 `auto.components.automations.*` 整棵空壳树） | 本轮只删叶不清壳；待全量清壳批量处理 |
| `config/scripts/locale-ko-key-overrides.json` 残留 | 已删域条目（automations 段 ~572-593 行等；Task 5 记 3173-3198、Task 6 记 1490-1519） | ko 覆盖表与 catalog 同步另行批处理 |
| `.gitignore` 死规则 | 第 5 行 `src-tauri/native/computer-use-macos/.build/` | computer-use 目录已删，规则无害留存（Task 5 deferred） |
| 守卫专属 CLI 状态键 | en/fr 各 4 键（`...cli.source.control.integration.cards.status*`） | 守卫测试锁定；当前无生产写入者（CLI 集成卡随任务收敛），删除需同步改守卫 |
| stale 测试夹具（stub 已删 API/域） | `startup-actions-selector.test.ts`（`initGitHubCache`/`hydrateBrowserSession`）、7 处 `automations: { onChanged }` mock、`RecoverableRenderErrorBoundary.lazy-chunk.test.tsx:23`（`boundaryId="page.automations"`） | 无害残留，随相应测试后续清理 |
| stale 注释/文案 | 已删 crate/域注释（`spawn.rs:4`、`service.rs:398`、`list-row-interaction.ts:1` 等）、`UnexpectedSignoutCard` 文案仍提 Artifact sharing / Orca Relay（含键 `6e3f1a9c5b`）、`source-control-generation-plan.ts`/`HostedReviewCreationDefaults.tsx` PR 文案、`git-remote-sync.ts:25` 缩进、`use-store-actions.ts` 计数句 | 全部为注释/文案级残留，无功能影响 |
| `SETTINGS_NAV_TARGETS` 目标 | `'linear'`（18 行区）、`'browser'`（34 行区） | 导航目标枚举残留，入口已无渲染者 |
| inert 桌面分支/常量 | `isWebClient` 常量喂参、~10 处 `__ORCA_WEB_CLIENT__` 检测路径、`mobilePairingConnectionMode` 默认值 | 终审已验证 inert（恒定桌面分支）；统一收尾再清 |
| 其他 Task 1–7 deferred minor | `terminal-drop` `remotePlatform` 不再传递；`StatusBarItem 'ssh'` 无渲染者；测试夹具 `'Orca Browser'` 字符串；`star-nag-api.ts` 失 exec 位；`'Update the remote runtime…'` 不可达字符串；Task 0 报告统计口径/日志噪音/prune 双次 grep | 逐条见 `.superpowers/sdd/…/progress.md` ledger（均已不影响行为） |
| 历史文档旧语义 | `docs/superpowers/specs/2026-10-11-computer-use-design.md`、`docs/phase2g-computer-use-record.md` 保留删除前语义 | 历史记录不改写，以本记录为准 |

## 验证命令与结果

| 命令 | 结果 |
|---|---|
| `rm -f tsconfig.tsbuildinfo && pnpm typecheck && pnpm build:web`（收尾） | exit 0（`built in 2.96s`） |
| `pnpm test src/renderer/src/i18n` | 16 文件 / 139 测试全过（守卫） |
| `pnpm test`（全量） | 2988 文件 / 26911 测试通过，0 失败、0 collection（exit 0） |
| `node orcinus-reach.mjs` final vs baseline | 新增孤儿 11，全部裁定保留并记录 |
| 46 ratchet 类测试 + `windows-console-input` | 281 + 5 测试全过 |
| `pnpm install --lockfile-only` | 锁文件零 diff |
| `pnpm dev` 有界启动 | Vite ready + 应用 Running，无报错，端口释放 |
