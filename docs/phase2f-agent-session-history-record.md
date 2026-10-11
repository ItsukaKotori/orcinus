# Phase 2 子项目 F：Agent Session History（本地会话历史）收尾记录

- 日期：2026-10-11（实施）～ 2026-10-11（终态自动门禁）
- 分支：`main`（spec 基线 `03bb8b02`；计划 `a335c6f7` + 计划修正 `1a9a0779`；特性/fix 提交 `cdf9e4d5`…`a5f96ac5`）
- 规格：`docs/superpowers/specs/2026-10-09-agent-session-history-design.md`
- 计划：`docs/superpowers/plans/2026-10-09-agent-session-history.md`（8 任务；Task 8 勾选全部复选框）
- 行为参照：`/Users/itsuka/CodeSpace/orca`（Electron oracle；`orca:` 前缀均为其内路径）
- 方法：subagent-driven-development（每任务 TDD 实现 + 评审 + fix 轮；终态三组门禁 + 本记录）
- 手工验收状态：**待用户复核（2026-10-11 自动化门禁已全绿；6 项手工清单见 §4）**

## 1. 范围与验收

| 面 | 内容 | 主提交 |
|---|---|---|
| Rust `ade-ai-vault` crate | `roots`：Claude projects root + OpenCode data dir（XDG / `OPENCODE_DB` 覆盖、`opencode.db` 优先 + `opencode-*.db` 同胞）；`claude`：JSONL 全量折叠（**首条非空 cwd**、oracle 时间戳门控 branch、标题链、预览 220 尾窗口、`lastUserPrompt`、token/队列/子代理计数、POSIX resume 引用）；`opencode`：旧版 `storage/` JSON + SQLite 只读（`SQLITE_OPEN_READ_ONLY`+`query_only`、逐列探 schema、list SQL/metadata/preview 照参照、跨 DB 同 id 取最新、三档 issue 文案）；`scanner`：两源合并 → `agent+sessionId` 去重（SQLite 压旧版）→ 排序 → limit 截断 + scope 保证 + 取消注册表；`cache`：60s TTL 单槽 + force 绕过 + invalidate + 取消不缓存；`delete`：两段校验/拒绝码矩阵 + companions（子代理目录 → session-env → transcript）trash；`resume`：恒 `{useRealCodexHome:true}`；`titles`：Claude 标题 ≤64/请求 | cdf9e4d5 + 19de9651 + 7bb687dd + 9ee002a8 + f7e252e7 + 8778ad7d |
| 桥接命令 | 5 命令（`ai_vault_list_sessions`/`resolve_session_titles`/`cancel_list_sessions`/`prepare_session_resume`/`delete_session`，camelCase args + `run_blocking`）；`AppState.ai_vault`（`VaultConfig::from_env` + `ListCache` + `CancelRegistry`）；specta 两处清单 + bindings 再生成（`bindings_are_fresh` 锁定） | fd35a433 |
| TS real 域 | `real/ai-vault.ts` 6 显式（5 命令路由 + `onWindowFocused` 订阅 `tauri://focus`）+ 4 方法维持 rejecting fallback（搜索×2/子代理/首条提示）；`create-api` RealDomains 接线；parity explicit 6 / missing 4 | a5f96ac5 |

验收口径（spec §6.3）：6 项手工验收（§4，待用户复核）；自动门禁三组全绿（§3）。终态变更规模（净树差异，`git diff --shortstat 03bb8b02..HEAD`）：**25 个文件，+8022 / −1**（含计划两提交 336 行；除 docs 外代码/配置为 24 文件 +7688 / −1）。若按各提交 stat 行累计（churn，`git log --numstat` 求和）则为 **+8088 / −67**。

## 2. 提交清单

`git log --oneline --reverse 03bb8b02..HEAD`（spec 提交 `03bb8b02` 为区间下界，不列入；本记录由 Task 8 提交追加，SHA 见提交后 `git log`）：

| 提交 | 内容 |
|---|---|
| a335c6f7 | docs: Phase 2F Agent 会话历史实施计划（8 任务，TDD） |
| 1a9a0779 | docs: 2F 计划修正 OpenCode 跨形态去重键（agent+sessionId） |
| cdf9e4d5 | feat(ai-vault): crate 骨架、roots 与 Claude 会话解析（Task 1） |
| 19de9651 | fix(ai-vault): Claude 解析对齐 oracle（首个 cwd/排序键/lastUserPrompt）（T1 fix 1） |
| 7bb687dd | feat(ai-vault): OpenCode 解析（旧版文件 + SQLite 只读）（Task 2） |
| 9ee002a8 | feat(ai-vault): scanner、取消注册表与 60s 列表缓存（Task 3） |
| f7e252e7 | feat(ai-vault): Claude 会话删除（两段校验 + 废纸篓）（Task 4） |
| 8778ad7d | feat(ai-vault): 恢复准备透传与 Claude 标题解析（Task 5） |
| fd35a433 | feat(bridge): aiVault 五命令与 AppState 装配（Task 6） |
| a5f96ac5 | feat(bridge): aiVault real 域接线与 parity（Task 7） |
| （本提交） | docs: Phase 2F 实施记录与门禁证据（Agent 会话历史）（Task 8） |

## 3. 门禁证据（自动化）

三组门禁在终态 HEAD（`a5f96ac5`，分支 `main`，`git status` 干净）顺序执行：

| 门禁 | 命令与工作目录 | 结果 |
|---|---|---|
| Rust 全量 | `cargo test --workspace`（`src-tauri/`） | **exit 0**；47 个 suite（含 doc-tests）累计 **905 passed / 0 failed / 0 ignored**；`ade-ai-vault` 单元测试 **110 passed**，`ade-bridge` 单元测试 **156 passed**，`ade-bridge` 集成 `tests/ai_vault.rs` **7 passed**；`specta_export::tests::bindings_are_fresh` 与 `export_lists_every_command` 均 ok |
| 类型与构建 | `rm -f tsconfig.tsbuildinfo && pnpm typecheck && pnpm build:web`（仓库根） | **exit 0**；`tsc --noEmit` 零错误；`✓ 11532 modules transformed`、`✓ built in 3.15s`；`externalized` / `MISSING_EXPORT` / `browser compatibility` **0 命中**；7 条信息性 `(!)`（6 条既有动态/静态双导入提示 + 1 条既有 >500kB chunk-size 警告），无本阶段新增文件 |
| TS 全量 | `pnpm test`（仓库根） | **exit 0，全绿**：`Test Files 3876 passed / 8 skipped (3884)`；`Tests 34646 passed / 122 skipped (34768)`；Duration **398.95s**。**本轮无性能抖动失败**（`browser-history-match.performance.test.ts` 未触发，无隔离复跑需要） |

日志留存于本机临时目录（`/tmp/ade-phase2f-cargo-test.log` / `/tmp/ade-phase2f-typecheck.log` / `/tmp/ade-phase2f-build-web.log` / `/tmp/ade-phase2f-pnpm-test.log`）。

## 4. 手工验收清单（待用户复核）

前置：`pnpm dev` 或打包产物均可；仅 macOS/POSIX（trash 走 objc2-foundation；需真实 Claude/OpenCode 数据目录存在）。

1. 打开 Agents 面板 → 列出真实 Claude 会话与 OpenCode 会话（SQLite 行可见）—— **待用户复核**
2. 点某个 Claude 会话恢复 → 新终端按 resume 命令启动 —— **待用户复核**
3. 删除一个 Claude 会话 → 原路径消失、废纸篓可见（含子代理目录）—— **待用户复核**
4. 删除菜单对 OpenCode 行不可用/拒绝 —— **待用户复核**
5. Claude 标签页出现会话标题；手动重命名优先 —— **待用户复核**
6. 切换应用焦点回来 → 列表自动刷新 —— **待用户复核**

任一项失败：补记到本文件 §5 并修复后，重跑对应自动门禁。

## 5. 规格偏差与边界备案

### 5.1 spec §7 九条（逐条备案）

1. **无持久解析缓存**：跨启动冷扫（上限 1000、Claude JSONL 全量行读）；大目录首扫较慢（披露）。
2. **无独立扫描进程**：`spawn_blocking` + 8 并发 + 取消注册表；超长扫描占用阻塞线程（可接受）。
3. **OpenCode 删除拒绝**：同参照；附加 `.db` 路径防线；SQLite 行 `filePath` 即 DB 路径，绝不可 trash。
4. **SQLite 只读限制**：WAL 未 flush/被锁 → 该 DB 降级（文案对齐参照「exit OpenCode cleanly」语义）。
5. **能力裁剪**：子代理/首条提示/搜索/结构化守卫未接，面板对应区域降级（既有 catch）；`resolveSessionTitles` 仅 Claude。
6. **`unlimited` 忽略**：契约参数存在但恒 1000 上限（披露）。
7. **trash 平台**：仅 macOS（objc2-foundation）；非 macOS 分支不顺延（POSIX 前提）。
8. **`prepareSessionResume` 恒 `{useRealCodexHome:true}`**：端口无托管 Codex home/账号体系（2A 现状），对 claude/opencode 无副作用。
9. **scope 保证的近似**：Claude scope 内 cwd 候选不受上限截断按参照实现；OpenCode 无 cwd 级 scope 保证（SQLite list 无 cwd 过滤），与参照一致但披露。

### 5.2 实现中发现的新偏差（按任务分组）

1. **T1（Claude 解析）**：plan/brief 原写「cwd 取最后一条有 cwd 的行」，与行为锚 `orca:src/main/ai-vault/session-scanner-claude-cwd-drift.test.ts`（#9361）冲突——oracle 要求**第一条非空 cwd**（`claude --resume` 只在起始 cwd 对应的 project 目录下找 transcript，末次 cwd 会报 "No conversation found"）；fix 轮逐字对齐（首个 cwd 胜、`19de9651`）。`branch` 按 oracle 时间戳门控（乱序旧行不覆盖、无 timestamp 行接受）、`model` 取最后一条 assistant 行的非空 `message.model`。排序键 brief 写 `updatedAt ?? modifiedAt`、源内实现按 oracle 改为 effective recency（`claude.rs:96-105`），但 **scanner 全局合并排序仍只按 `modifiedAt`**（plan Task 3 逐字锚定）——两处口径漂移，披露。`lastUserPrompt` 用预览纪律（隐藏块剥离 + 220 UTF-16 截断 + `...`）而非 oracle `normalizePromptField` 的 200 字符单行归一（仅极长/含隐藏块 prompt 的边界文案差异）。Minors：`totalTokens` 四字段 u64 直接相加（畸形超大值 debug 溢出 panic）；数值时间戳口径不一致（Claude 按毫秒直取、OpenCode 以 `MAX_TIMESTAMP_MILLIS=1e12` 猜秒/毫秒）；resume 引号为 POSIX 单引号转义（非 fish 专属）。
2. **T2（OpenCode 解析）**：`OPENCODE_DB=-` 解释为「忽略该覆盖 → 回退 canonical 发现」（spec 仅写「`-` 忽略」；若原意为禁用 DB 扫描，改动点仅一行 + 测试）。SQLite title 回退链按 oracle：`title` 列 → 预览窗口内 user 行 `summary.title/body` → 生成 `OpenCode <id8>`（brief 提示的「目录名/首条 user 文本」未采用，oracle 胜 plan 措辞先例）。跨 DB 同 id recency（`time_updated>0 ? time_updated : time_created`）相等时保留 canonical（先入）DB。旧版预览的 `summary.body/title` 回退文本未做预览归一（原样 trimmed）。预览时间戳与排序时间戳口径不一致（预览 `timestamp_iso` 数值按毫秒；排序 `timestamp_ms` 应用秒/毫秒阈值猜测）。旧版行在合并前按 per-source limit 截断（`opencode.rs:115-116`），scanner 全局上限不会补回被截掉的旧版行。
3. **T3（scanner/缓存/取消）**：为「扫描中逐文件检查取消」给 claude/opencode 增加 `pub(crate)` 可取消变体（公共签名不变，Task 1/2 全部旧测试未动仍绿）。scope 保证受 `limit*2` 候选上限约束（oracle 有独立 scope-discovery 全量绕过上限，本切片有意简化，stars 更早的 in-scope 行可能仍被丢弃）。单槽缓存无扫描合并（同 key 并发 miss 可双扫、last-write-wins；取消结果不写缓存）。
4. **T4（删除）**：trash 调用瞬间的 ENOENT 竞态映射为 `failed`（oracle 将 ENOENT 映射为成功；TOCTOU 失败安全取舍，重试幂等 = `deleted`，窗口极小）；相对 `home` 未绝对化（潜伏问题；`execute_delete` 内 lexical normalize 不解析相对 root，Task 6 始终传绝对 home，不触发）。
5. **T5（resume/titles）**：无标题 transcript 的请求被省略（oracle title reader 走 finalized session、理论带 `Claude <8>` 生成回退，本切片 `read_claude_title` 仅返回真实标题候选）；未按 `agent+sessionId` 去重（重复请求返回重复条目；消费端 `ai-vault-tab-title-sync.ts` 以 Map 归并，未见影响）；显式 `transcriptPath` 命中后不做 transcript 内 sessionId 身份校验（文件名/路径≠内部 id 的漂移文件返回内部真实标题，对 UI 更可能是期望行为）。
6. **T6（桥接命令）**：list 命令 args 无 `executionHostScope` 字段（TS 层显式剥离；serde 下未知字段本会被静默忽略，二者等价）；被替代扫描与取消注册表的 finish 竞态（与既有 `GitCancelRegistry` 同构——superseded scan 结束后 `finish` 可能释放新扫描的同 token 注册，属既有模式）；桥集成删除测试只断言原路径消失 + 缓存失效，不断言 trash 落点（crate 层已有 `~/.Trash` 近似断言，见 §6 环境限制）；pre-existing 工具链漂移未修（`notifications_native.rs` 4 个 `needless_return`、`tests/gh_exec.rs` 1 个 `cloned_ref_to_slice_refs`、整 crate 58 处 rustfmt 差异均 HEAD 既有，保持提交最小）。
7. **T7（real 域/parity）**：非 local/`'all'` 的 `executionHostScope` 退化为本地扫描（Tauri 侧 local-only 宿主；renderer merged 视图在该阶段等价本地结果）；`titles` 因 Tauri 契约显式声明该字段而原样透传（与 list 的剥离不对称是契约决定的，代码注释说明）。`onWindowFocused` 走 `tauri://focus` 事件名，真实聚焦触发依赖 §4 手工第 6 项。
8. **harness（过程记录）**：多个 reviewer subagent 派发仍反复死于 SSE 超时；全部评审适配为紧凑提示词 + 分块 diff/文件读取（limit/offset），评审仍由新鲜 subagent 执行。

## 6. 已知边界与后续

- **搜索索引**（FTS5 `ai-vault-search/*`）：未接，`searchSessions`/`searchStatus` 维持 rejecting fallback（面板无 UI 消费）。
- **子代理列表** `listSubagentSessions` 与**首条提示** `getFirstUserPrompt`：未接（面板既有 catch 降级；`firstUserPrompt` 也不在 list scan 产出）。
- **其余解析器**：oracle 共 18 个 agent 解析器，本切片仅移植 Claude/OpenCode（spec §2.2：Codex/Cursor/Gemini 及其余 14 个未做；差额计数以「18 总 − 2 已移植」为准）。
- **Codex 标题**：未实现（`resolveSessionTitles` 的 codex 请求返回空；标题仅 Claude）。
- **SSH/远端/运行时宿主**：`aiVault.*` 本地单一主机；非本地 scope 退化为本地扫描（§5.2-T7）；WSL 未接。
- **持久解析缓存**：跨启动冷扫（§5.1-1）。
- **独立扫描服务进程**：`spawn_blocking` 替代（§5.1-2）。
- **结构化会话所有权守卫**：未接（oracle guard 机制未移植）。
- **`unlimited` 真实支持**：参数接受但恒 1000 上限（§5.1-6）。
- **fish 安全引用**：resumeCommand 为 POSIX 单引号转义（§5.2-T1）。
- **`Claude <8>` 标题回退**：无标题 transcript 的 titles 请求被省略（§5.2-T5）。
- **trash 回读断言的环境限制**：crate 测试的 `~/.Trash` 近似断言依赖本机废纸篓可写且枚举稳定（并发/权限环境可能不稳）；桥集成测试不做 trash 回读。测试会向真实 `~/.Trash` 留下少量 PID 唯一命名的测试产物。
- **回滚策略**：还原 `src/bridge/create-api.ts` 的 `aiVault` RealDomains 接线为 mock 即可退回 Phase-0 语义；Rust crate/命令/bindings 保留无副作用。
