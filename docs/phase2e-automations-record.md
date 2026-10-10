# Phase 2 子项目 E：automations（本地定时自动化）收尾记录

- 日期：2026-10-09（实施）～ 2026-10-10（终态自动门禁）
- 分支：`main`（spec 基线 `1e52723f`；计划 `ce2afc77`；特性提交 `090bca55`…`1c29079e`）
- 规格：`docs/superpowers/specs/2026-10-09-automations-design.md`
- 计划：`docs/superpowers/plans/2026-10-09-automations.md`（8 任务；Task 8 勾选全部复选框）
- 行为参照：`/Users/itsuka/CodeSpace/orca`（Electron oracle；`orca:` 前缀均为其内路径）
- 方法：subagent-driven-development（每任务 TDD 实现 + 评审 + fix 轮；终态三组门禁 + 本记录）
- 手工验收状态：**待用户复核（2026-10-10 自动化门禁已全绿；6 项手工清单见 §4）**

## 1. 范围与验收

| 面 | 内容 | 主提交 |
|---|---|---|
| Rust `ade-automations` crate | `schedule`：cron/RRULE 解析与 occurrence 数学（host-local 墙钟、DST 模糊取最早/空洞前移、trim-before-cap 2 KiB 上限、无解 / 越界 → `None`）；`store`：CRUD、`creationKey` 幂等、`(automationId,scheduledFor)` 去重、终态保留 100、keyset 游标分页、损坏隔离重建 + 加载回填；`precheck`：`sh -c` 执行、超时组 `SIGTERM`→≤2s 无条件组 `SIGKILL`、1 MiB 尾缓冲 + 4000 字符尾截断；`service`：60s tick、armed 门、grace 内追赶最近一次、超 grace 一条 `skipped_missed`（逐字文案）、advance、启动一次性对账 | 090bca55 + 5ed2df1a + e1cc18a3 + 3724bb7f + c5e68cbf + 3bf0cd68 + 5efb72d9 + 254e12be |
| 桥接命令与事件 | 10 命令（`automations_list/create/update/delete/run_now/runs/run_precheck/mark_dispatch_result/snapshot_workspace_name/renderer_ready`）；`automations:changed`（`{reason}`）与 `automations:dispatch-requested`（`{automation,run,dispatchToken}`）两事件；`AppState` 装载 store + 启动 tick；specta 两处清单 + bindings 再生成（`bindings_are_fresh`） | be310ff7 + 9a910b50 |
| TS local RPC | `runtime-rpc-client` local 分支拦截 `automation.*` 6 方法（list/create/update/delete/runNow/runs）→ Tauri 命令；wire 反向映射、fenced 参数忽略、legacy 形态（`null` params / 无 selector）兼容 | fccaa124 + 5b3cf718 |
| real 域接线与 parity | `real/automations.ts` 6 宿主方法（`runPrecheck`/`markDispatchResult`/`snapshotWorkspaceName`/`rendererReady` + 两订阅）；`create-api` RealDomains 接线；parity `automations` surface 6 explicit / 6 missing 与契约键集锁死 | 1c29079e |

验收口径（spec §6.3）：新建 hourly 自动化到点执行、Run Now、precheck 失败 `skipped_precheck`、`new_per_run` 工作区、重启保留、超 grace `skipped_missed` 六项手工验收（§4，待用户复核）；自动门禁三组全绿（§3）。终态变更规模（净树差异，`git diff --shortstat ce2afc77..HEAD`）：**24 个文件，+7133 / −1**（新增 Rust crate 7 文件、bridge 命令/事件/测试 6 文件、TS 6 文件、Cargo.lock/bindings 等；区间内另含计划提交 `ce2afc77` 483 行，故含计划口径 `1e52723f..HEAD` 为 **25 个文件，+7616 / −1**）。若按各提交 stat 行累计（churn，fix 轮对同一批行重复计入）则为 **+7832 / −217**（`git log --numstat` 求和）。

## 2. 提交清单

`git log --oneline --reverse 1e52723f..HEAD`（spec 提交 `1e52723f` 为区间下界，不列入；计划提交 `ce2afc77` 列入；本记录由 Task 8 提交追加，SHA 见提交后 `git log`）：

| 提交 | 内容 |
|---|---|
| ce2afc77 | docs: Phase 2E automations 实施计划（8 任务，TDD） |
| 090bca55 | feat(automations): ade-automations crate 与 schedule 数学（cron/RRULE）（Task 1） |
| 5ed2df1a | fix(automations): schedule 边界改为 None（超范围时间戳不再 panic）（T1 fix 1） |
| e1cc18a3 | feat(automations): AutomationStore（CRUD/去重/保留/分页/损坏隔离）（Task 2） |
| 3724bb7f | fix(automations): 工作区名快照覆盖全部运行 + create_run 更新 lastRunAt（T2 parity fix） |
| c5e68cbf | fix(automations): store 不可变字段守卫/损坏隔离/游标终止（评审修复）（T2 fix 2） |
| 3bf0cd68 | feat(automations): precheck 执行器（超时组杀/尾截断）（Task 3） |
| 5efb72d9 | fix(automations): precheck 超时后无条件组杀（防孙进程握管阻塞）（T3 fix 1） |
| 254e12be | feat(automations): AutomationService（tick/grace/advance/对账）（Task 4） |
| be310ff7 | feat(bridge): automations 命令面与 changed/dispatch-requested 事件（Task 5） |
| 9a910b50 | fix(automations): dispatched 转换由宿主打点 startedAt/dispatchedAt（T5 fix 1） |
| fccaa124 | feat(renderer): local automation RPC 拦截（CRUD 映射）（Task 6） |
| 5b3cf718 | fix(renderer): local automation RPC 补 Tauri args 包装（T6 fix 1） |
| 1c29079e | feat(bridge): automations real 域接线与 parity（Task 7） |
| 7656005e | fix(automations): 创建时补齐 executionTarget 字段（local 默认）（终审修复） |
| （本提交） | docs: Phase 2E automations 实施记录与门禁证据（Task 8） |

## 3. 门禁证据（自动化）

三组门禁在终态 HEAD（`1c29079e`，分支 `main`，`git status` 干净）顺序执行：

| 门禁 | 命令与工作目录 | 结果 |
|---|---|---|
| Rust 全量 | `cargo test --workspace`（`src-tauri/`） | **exit 0**；44 个 suite（含 doc-tests）累计 **786 passed / 0 failed / 0 ignored**；`ade-automations` 单元测试 **69 passed**（schedule/store/precheck/service），`ade-bridge` 单元测试 **156 passed**，`ade-bridge` 集成 `tests/automations.rs` **9 passed**；`specta_export::tests::bindings_are_fresh` 与 `export_lists_every_command` 均 ok |
| 类型与构建 | `rm -f tsconfig.tsbuildinfo && pnpm typecheck && pnpm build:web`（仓库根） | **exit 0**；`tsc --noEmit` 零错误；`✓ 11531 modules transformed`、`✓ built in 3.31s`；`externalized` / `MISSING_EXPORT` / `browser compatibility` **0 命中**；7 条信息性 `(!)`（6 条既有动态/静态双导入提示 + 1 条既有 >500kB chunk-size 警告），无本阶段新增文件 |
| TS 全量 | `pnpm test`（仓库根） | **exit 0，全绿**：`Test Files 3875 passed / 0 failed / 8 skipped (3883)`；`Tests 34631 passed / 0 failed / 122 skipped (34753)`；Duration **447.18s**。**本轮无性能抖动失败**（已知 `browser-history-match.performance.test.ts` 未触发，无隔离复跑需要） |

日志留存于本机临时目录（`cargo-test-phase2e.log` / `build-web-phase2e.log` / `pnpm-test-phase2e.log`）。

终审结论（`1e52723f..a6c9cd98`，15 提交）：**WITH-FIXES** —— 1 项 Important（创建时缺 `executionTargetType/executionTargetId`，违反共享 `Automation` 契约）已修复（`7656005e`：缺省补 `'local'`、显式值保留，新增 2 条测试）；全部 deferred minors 经 triage 判定为 defer（时区窗口边角、超容量瞬时、setsid 逃逸、通知冗余、标题竞态等，均披露上文 §5.2）。终审修复波次（`7656005e`）后的最终树复跑：`cargo test --workspace` **exit 0**（44 suite，**788 passed / 0 failed**，较 §3 的 786 增 2 条新增测试）；TS 无改动（`pnpm test` 34631/0、typecheck/build:web 维持 §3 结果）。

## 4. 手工验收清单（待用户复核）

前置：`pnpm dev` 或打包产物均可；仅 macOS/POSIX（precheck 组杀为 POSIX 语义）。

1. 新建 hourly 自动化（现有工作区模式）→ 到点后后台 agent 启动并执行提示词 → 运行历史出现且完成状态正确 —— **待用户复核**
2. `Run Now` 手动触发（含 disabled 记录）—— **待用户复核**
3. precheck 配 `exit 1` → `skipped_precheck` 且历史可见 stdout/stderr 尾 —— **待用户复核**
4. `new_per_run` 模式 → 生成 `auto-<slug>-<stamp>` 工作区且带 provenance —— **待用户复核**
5. 应用重启 → 定义/历史/`nextRunAt` 保留，tick 继续 —— **待用户复核**
6. 将 grace 设小 + 暂停应用超过 grace → 重启后 `skipped_missed` 一条 —— **待用户复核**

任一项失败：补记到本文件 §5 并修复后，重跑对应自动门禁。

## 5. 规格偏差与边界备案

### 5.1 spec §7 九条（逐条备案）

1. **时区=host-local**：对齐参照（timezone 字段仅存储）；跨时区语义偏差在记录中披露。
2. **无 live-run watchdog**：运行中强杀应用 → 重启后对账 stranded；窗口期内无恢复（参照桌面同款）。
3. **不做项**（§2.2）：external/SSH 多宿主/owner fencing/usage/CLI/headless serve/skip 折叠/`automation.show`/上下文迁移。
4. **skip 折叠**：同一拒绝连续出现各写一条（参照会折叠为 `occurrenceCount`）；历史噪声略高（披露，后续补）。
5. **新增依赖 `chrono`**（`clock` feature）：本地时区/DST 数学的标准工具；无其它新依赖。
6. **dispatch token**：30 分钟 TTL、内存态；应用重启即失效（渲染层未消费 token 校验时无影响；provenance 消费见 `automationProvenanceRequest` 既有链路）。
7. **tick 依赖渲染层武装**：无 headless 调度；窗口关闭即无 tick（桌面单窗口语义）。
8. **fenced 参数忽略**：多宿主数据（selector/expectedOwner）被静默忽略；单机语义正确，未来接多宿主时需回填。
9. **worktree provenance token 不在 Rust 校验**：`new_per_run` 的 `automationProvenanceRequest` 经既有工作区创建链路落 metadata；参照版在 main 端用 dispatch token 校验归属，本切片不校验（渲染层自证 + 单机信任边界），披露。

### 5.2 实现中发现的新偏差（按任务分组）

1. **T1（schedule）**：任务书 `cron_length_cap` 示例（尾部补 2100 空格）相对 TS 参照是**有效**输入——TS 在 `parseCronExpression` 施加 2 KiB 上限前先 trim；按 TS 逐字移植为 trim-before-cap 并用 `oversized_cron_text_is_rejected_after_trimming` 钉住（`090bca55`）。`is_valid_schedule` 用 occurrence 分钟探测（对照 TS 的日期级探测）仅在「唯一发生落在 9×366 天窗口最后一分钟」的构造边缘上可能接受而 TS 拒绝；无实际影响。DST 等价性由一次性差分 harness 钉住（实际 TS 模块 × 本 crate，25 schedules × 19 anchors × 5 时区 = 2850 fixtures × 5 TZ，含 1h/30min 春进空洞与秋退模糊、Apia 24h 跳日；harness 在提交前捕获并修复两个真实 bug：chrono 模糊时刻选择与 JS 相反、双探针在 London 回退日收敛到同一点），未提交固定时区单测（`chrono-tz` 不在依赖预算内）。fix 轮把 schedule 边界改为**全函数**（超范围 i64 → `None`，tick 循环不再可能 panic；`5ed2df1a`）。
2. **T2（store）**：控制器裁定两处 parity 修复——工作区名快照更新**所有**匹配运行（初版仅最新）、`create_run` 触碰 `lastRunAt` 而去重路径不触碰（`3724bb7f`）。评审修复（`c5e68cbf`）：运行不可变字段守卫（`createdAt/runNumber/scheduledFor`）；顶层损坏 JSON 之外的**非数组 sections** 也走隔离（含原始字节备份）；加载时 id/runNumber 回填 + 分页终止（防游标环）；返回的 run 不被 prune；`lastRunAt` 单调；损坏备份文件名唯一后缀。遗留 minors：受保护的冒充运行可让单自动化暂时超出 100 上限直至下一次插入触发 prune；畸形游标按「末页」处理。
3. **T3（precheck）**：超时路径改为 `SIGTERM` 后 ≤2s **无条件组 `SIGKILL`**，不再等待读取线程自然收敛（`5efb72d9`；回归测试造 SIGTERM 免疫孙进程，RED 修复前 join 挂起 >20s，修复后有界）；reader 循环对 `EINTR` 重试；补洪泛输出测试。Windows 进程树杀不在范围（仅 macOS/POSIX）。
4. **T4（service）**：主动披露的语义选择——去重按计划实现（第二个 tick 不重复派发，无事件）；dispatch token 只签发不校验（spec §7.6）；无下一 occurrence 时 advance 停在 `i64::MAX`（永不 due）；对账覆盖应用启动前遗留的全部非终态 run。遗留 minors：受保护 no-op 合并仍发 `changed` 事件（过度通知）；`run_precheck_for` 在持久化失败时仍可能返回 `Some`；run 标题的 runNumber 预读存在竞态（已披露）。
5. **T5（bridge 命令）**：fix 轮让 `dispatched` 转换由宿主打点 `startedAt`/`dispatchedAt`（`9a910b50`）；但直接落 `completed`（无先置 dispatched）仍不打点——比 oracle 的「无条件 `startedAt ?? now`」窄（披露）。`agentId` 校验仅非空（oracle parity，非正则）。同毫秒 `run_now` 存在重复派发边缘；`rrule:null` 的 update 报错而 oracle 会强转（null 删除语义一致性取舍）。为命令面新增 crate 内 `validate_definition`/`run_now`（并含 `Cargo.lock` 变更），属任务书许可的 crate 追加。
6. **T6（local RPC）**：fix 轮发现五个命令缺 Tauri `{args}` 包装（RED 8→GREEN 44/44；`5b3cf718`）。`automation.list` 把所有行标为 `{kind:'self'}`（ssh/orphan scope 请求按空校验通过——计划与任务书认可的首切片口径）；`workspaceId` 的 null 推断已对照「null 删除键」的合并语义验证。
7. **T7（real 域/parity）**：计划文本「explicit 9 / missing 5」相对现行契约**已过时**——`AutomationsApi` 共 12 方法，实际拆分为 **6 explicit（4 命令 + 2 订阅）/ 6 missing（external 五方法 + `retainExternalScopes`）**，parity 以 `satisfies Record<keyof …>` 锁死契约键集。`markDispatchResult` 把宿主 nullable 的 `{run:null}`（自动化被删）强制 cast 透传，渲染层调用方均忽略返回值（披露）。
8. **harness（过程记录）**：多个 reviewer subagent 派发反复死于 SSE 超时；适配为紧凑提示词 + 分块 diff/文件读取（limit/offset），全部评审仍由新鲜 subagent 执行。

## 6. 已知边界与后续

- **external managers**：`listExternalManagerForOwner`/`listExternalRunsForOwner`/`createExternalForOwner`/`updateExternalForOwner`/`runExternalActionForOwner`/`retainExternalScopes` 维持 rejecting fallback（UI 外部区显示不可用，披露）。
- **SSH 多宿主与 owner fencing**：fenced 参数（selector/expectedOwner/destination）接收即忽略；多宿主调度、归属校验与数据回填未接。
- **usage 计费**：run 的 `usage` 字段形态保留，无计费/汇总逻辑。
- **CLI 与 headless serve**：无 CLI 触发面、无 headless 调度端口；tick 依赖渲染层武装（§5.1-7）。
- **skip 折叠（occurrenceCount）**：连续同类拒绝各写一条，未折叠（§5.1-4）。
- **live-run watchdog**：运行中强杀仅靠重启对账落 `dispatch_failed`；窗口期无运行时恢复（§5.1-2）。
- **`automation.show`**：未接，走既有 fallback（渲染层未消费）。
- **legacy `projectId`→`runContext` 迁移**：仅存 wire 重命名映射；旧数据向 `runContext` 的结构化迁移未做。
- **`markDispatchResult` 契约 nullable 化**：宿主在自动化已删时返回 `{run:null}`，preload 契约尚无 null 槽位（当前 cast 透传，调用方忽略）；后续应把契约改为 nullable。
- **回滚策略**：还原 `src/bridge/create-api.ts` 的 `automations` RealDomains 接线与 `runtime-rpc-client` local 拦截为 mock 即可退回 Phase-0 语义；Rust 命令、crate 与 bindings 保留无副作用。
