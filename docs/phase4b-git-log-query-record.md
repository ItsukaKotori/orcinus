# Phase 4b — Git Log 插件增量 2b（提交图谱查询：全分支 / 过滤 / 分页 / refs）

日期：2026-10-18 ～ 2026-10-19。分支 `phase4b-git-log-query`（自 main `86540da3`，含增量 1 + 2a）。
规格：`docs/superpowers/specs/2026-10-18-git-log-plugin-increment2b-design.md`
计划：`docs/superpowers/plans/2026-10-18-git-log-plugin-increment2b.md`
台账：`.superpowers/sdd/2026-10-18-git-log-plugin-increment2b/progress.md`

## 目标

在增量 2a（真实 HEAD 历史 + commitFiles，只读、限当前 worktree）之上，交付 IDEA VcsLog 级查询能力：**全分支范围、后端过滤（author/date/path/grep）、offset 分页无限滚动、分支选择器（git.refs）、可分类错误码**，仍为插件 + 自包含内联 React bundle，不改现有 git/Source Control 功能。

## 交付（T1–T7，逐任务 TDD + CodeReview 审查门）

| 任务 | 内容 | commit |
|---|---|---|
| T1 | `ade_git`：`GitHistoryRefs{Head/All/Rev}` + `GitHistoryQuery` + `history_with` + `list_refs`；`history()` 转薄封装向后兼容 | `5408dd47` |
| T2 | TS host 面：`git.log` params 扩 query（strict）、新增 `git.refs`、`PluginPanelActionErrorCode` 扩 3 码（类型 + 运行时数组两侧） | `8f6096e3` |
| T3 | Rust host 面：`git_log_params` 全字段校验器、`git.refs` MethodSpec、`SPECS` 18→19、`GitLogQuery` + trait `git_log(&query)`/`git_refs`、dispatch、gate 7→8、两处 Fake、ade-bridge 签名对齐 | `bcee6797` |
| T4 | ade-bridge：`git_log`→`history_with`（全字段透传）、`git_refs`→`list_refs`、`map_core_err`（NotFound→unknown-oid / InvalidInput→invalid_params / 其余→not-a-git-repo） | `e3fcdd73` |
| T5 | UI 数据层：`domain/query.ts`（`logParams`/`mergePages`/`PAGE_SIZE=100`）、`GitLogRepository.loadSnapshot(filter?,skip?)`/`loadRefs`、`gitRefsSchema`、host/fixture 实现 | `796fadd1` |
| T6 | UI 接线：`GitLogView` filter/debounce/skip/hasMore/loadingMore 状态机 + `loadMore`；`Toolbar` 后端 chips（Author/Path/Grep/Since/Until）；`RefPanel` 后端 range（Current/All/分支）；`GraphList` 无限滚动 | `b4c18fe2` |
| T7 | 全量门禁 + 记录 + 常量统一（`GIT_LOG_DEFAULT_LIMIT`） | 本记录 |

## 关键设计裁定

- **后端过滤而非客户端**：`refs/author/since/until/grep/path/skip` 全部落到 `git log` 参数拼装（options → range rev/`--all` → `--skip`/`-n{limit+1}` → `-- <path>`），TS 侧仅长度/边界，字符集/`--end-of-options`/`resolve_commit` 二次校验在 Rust（T3 `git_log_params` 拒 `-` 前缀；T1 `resolve_commit` 拒空/`-`）。
- **注入防护**：`Rev(r)` 先 `resolve_commit` 校验取 oid 再入 argv，绝不把用户串当 range/option；`path` 拒空/`-` 起 + `--` 分隔。
- **边界语义稳定**：`currentRef/remoteRef/baseRef/mergeBase/ahead/behind` 恒按 HEAD 计算，与 `refs=all` / 分页无关（`history_with` metadata 在拼 argv 前算）。
- **分页去重**：`mergePages` 按 commit id 去重追加，metadata 取最新页；offset 分页在历史剧变时可能跳/重，已按 VcsLog 惯例判为可接受（spec §7）。**offset 按本页实际消费条数累加**（`skip += next.items.length`，首屏 `skip = snap.items.length`），而非页大小回显——终审据此修正了「`skip = next.limit`」的回归。
- **错误码契约**：`unknown-oid`/`not-a-git-repo`/`no-active-worktree`/`invalid_params`/`unavailable` 均入 `PluginPanelActionErrorCode` 且同步运行时镜像数组 `PANEL_ACTION_ERROR_CODES`（否则具体码被折叠成 `action_failed`，回归测试锁死）。
- **面板限流友好**：`GitLogView` 对 filter 变更做 300ms debounce；相同内容保持 `debounced` 引用触发 React bail-out，避免挂载误触双请求 + 打字即查风暴（宿主 panel 面 30/10s）。
- **refs 降级**：`loadRefs` reject（含超大仓整体失败）→ 选择器退化为 Current/All，不阻塞日志。
- **路径不外泄**：宿主只回稳定 message，git 原文/rev 仅 `eprintln!` 于宿主侧。

## 审查发现并已修复（要点）

- T1：Rev 解析失败/path 非法曾静默返空 → 改 `CoreError::NotFound`/`InvalidInput`（恢复 unknown-oid 语义、T4 映射不为死码）。
- T4：`path=a.txt` 断言恒真（fixture 每提交都改它）→ 加不匹配路径→`Some(0)` 判别。
- T5：转发不可证（stub 丢 action/params）→ 加请求记录器断言出站 `git.refs`/`git.log` params + 畸形结果 reject。
- T6：`Current` 恒不高亮（`undefined` 塌 `null`）；挂载双请求；`loadMore` 失败静默终止分页 → 三条全部修复 + 判别测试 + 翻页 in-flight ref 闸门。

## UI 错误分类（终审补入）

- `callHost` 抛 `HostCallError`（携带稳定 code），`GitLogView.describeHostError` 按码出英文文案（unknown-oid/not-a-git-repo/no-active-worktree/rate_limited/invalid_params/capability_denied）。
- 再查（切分支/过滤）失败**保留已加载列表** + 工具栏下内联 banner；仅「从未成功加载」才整屏错误。与 `loadMore` 失败「保留 + 重试」策略一致。

## 门禁

- `cargo test --workspace`：通过（见 CARGO_EXIT）。
- `pnpm typecheck` / `pnpm build:web` / `pnpm build:git-log`（451 KiB 内联单文件）：通过。
- `pnpm test plugins/git-log`：38 通过（含 query/fixture/host/view 接线断言）。

## 已知遗留（非缺陷，后续处置）

- `is_inside_work_tree` 探测缓存未做（每调用 probe 一次；spec §2.1 允许）。如需可抽 `assert_git_repo` + 正向缓存（含 worktree 删除失效策略）。
- `git.refs` 复用 `git_result` 无条数上限：超大 ref 列表会整体 `invalid_params` → UI 已降级为 Current/All。若需可加 `git_refs_result` 条数上限 / 截断策略。
- `git.commit_files` 仍把所有 `commit_compare` 错误映射为 `unknown-oid`（2a 既有），与 `git_log` 新按变体区分不一致；终局清理可统一走 `map_core_err`。
- `selectVisibleRows` 第三参 `activeRef` 已成死参（range 转后端），可在增量 2c 连同测试一并清。
- 缺省 limit 口径：本增量定稿 **100**（`GIT_LOG_DEFAULT_LIMIT`）；spec §3「缺省＝2a 行为」原指 ade-git `GIT_HISTORY_DEFAULT_LIMIT=50`，UI/host 层统一取 100（分页友好、仍 ≤ host 上限 200）。
- 全量 `pnpm test`：仅 `src/shared/pty-reply-echo-shapes.node-pty.test.ts` 4 例失败 = `posix_spawnp failed`（沙箱禁起 PTY，**非 2b 触及文件**、与本体无关，判环境失败放行；非「时序 flaky」）。

## 增量 2c（A 组加固，已合入 main 96cec477）

收尾 2b 遗留的 refs/日期项（分支 `phase4c-git-log-hardening`，worktree 已清理）：
- **C1** `ade_git::list_refs`：`GIT_REFS_MAX_LIMIT=2000` 条数上限（`list_refs_capped(path,max)`，签名不变）→ 超大仓不再整体撑爆 host `git_result`；`for-each-ref` 加 `%(*objectname)`，annotated tag 的 `revision` 用 peeled commit oid（与图内 commit 对齐，闭 M-5）。
- **C2** `since/until` ISO 校验：TS zod 加 `^\d{4}-\d{2}-\d{2}([T ].*)?$`，Rust `git_log_params` 用 `is_date_prefix`（含尾随字符守卫，与 TS 同口径）在入口判 `invalid_params` → 非法日期不再被 git 失败折叠成 `not-a-git-repo`。
- 附带：修「审查并启用无反应」（consent 对话框 key 改按已解析 `consentPlugin`）+ install→consent 时序回归测试；`git-log` manifest `engines.orca` `>=1.4.0`→`>=0.0.1`（匹配宿主实际版本）。
- **仍未做**（后续可选）：`is_inside_work_tree` 探测缓存、`commitFiles` 统一走 `map_core_err`、`selectVisibleRows` `activeRef` 死参清理、`git.refs` 截断无 truncated 信号（>2000 heads 时 tags 组可能不显）、日期语义合法性（月/日范围）。
- 门禁：cargo ade-git/plugins/bridge 0、typecheck/build:web/build:git-log 绿、git-log+plugins+bridge+settings 172 文件/1357 绿。origin/main == main == 96cec477。

## 增量 2d（收尾清理，已合入 main cd5826b1）

分支 `phase4d-git-log-cleanup`（worktree 已清理），闭 2b/2c 遗留三项：
- **D1** `ade-bridge::git_commit_files`：`commit_compare` 的 Err 改走 `map_core_err`（与 git_log/git_refs 一致按变体分码），保留 `invalid-commit→unknown-oid`。（现网 `commit_compare` 的 Err 一支当前从不构造，属一致性/前瞻）
- **D3** `ade-bridge`：`assert_git_repo` + 进程内正向缓存（`OnceLock<Mutex<HashSet>>`，仅缓存 true、锁中毒 fail-safe、无嵌套锁），git.log/git.refs/git.commitFiles 不再各 spawn 一次 `git rev-parse`；路径失去 git 属性时后续真跑 git 经 `map_core_err` 报错（不静默错数据）。
- **D2** UI：`selectVisibleRows` 移除 `activeRef` 死参（range 已后端化）+ 更新测试。
- **仍开放（可选）**：`git.refs` 截断无 truncated 信号（>2000 heads 时 tags 组可能不显）、日期语义合法性（月/日范围，仅形状校验）。
- 门禁：cargo ade-git/plugins/bridge 0、typecheck/build:web/build:git-log 绿、git-log+plugins+bridge 68 文件/654 绿。审查无 Critical/Important/Minor。origin/main == main == cd5826b1。

## 增量 2e（refs 截断信号 + 日期范围，已合入 main 25a46d20）

分支 `phase4e-git-log-refs-dates`（worktree 已清理），闭 2c/2d 最后两项遗留：
- **E1** `git.refs` 截断信号：`ade_git::list_refs`/`list_refs_capped` 返回 `GitRefsResult{refs,truncated}`（装满 max+1 即停、不全量物化）；桥接 `git_refs` 自动序列化 `{refs,truncated}`；TS `gitRefsSchema`→object、`GitLogRepository.loadRefs():Promise<GitRefs>`、host/fixture 同步；`GitLogView` 存 `refsTruncated`、`RefPanel` truncated 时显示「Some refs are not shown」。reject 仍降级 `{refs:[],truncated:false}`。
- **E2** `is_date_prefix` 加月 01-12 / 日 01-31 范围（Rust 为强制点；TS 保持 shape regex）。
- 审查修复：ade-plugins 两处 `git_refs` 测试替身改对象形状（此前锁旧数组、回退会误绿）；`GitRefsResult` 补 specta derive。
- **仍开放（低优先）**：日历非法日（`2026-02-31`）仍放过（需按年月判大小月）；破坏性形状变更未抬 `engines.orca` 下界（本插件随宿主同构建分发，旧宿主场景下会优雅降级为只剩 Current/All）。
- 门禁：cargo ade-git/plugins/bridge 0、typecheck/build:web/build:git-log 绿、git-log+plugins+bridge 68 文件/655 绿。审查无 Critical。
