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
- **分页去重**：`mergePages` 按 commit id 去重追加，metadata 取最新页；offset 分页在历史剧变时可能跳/重，已按 VcsLog 惯例判为可接受（spec §7）。`skip` 取后端 `limit` 回显，防后端 clamp 后错位。
- **错误码契约**：`unknown-oid`/`not-a-git-repo`/`no-active-worktree`/`invalid_params`/`unavailable` 均入 `PluginPanelActionErrorCode` 且同步运行时镜像数组 `PANEL_ACTION_ERROR_CODES`（否则具体码被折叠成 `action_failed`，回归测试锁死）。
- **面板限流友好**：`GitLogView` 对 filter 变更做 300ms debounce；相同内容保持 `debounced` 引用触发 React bail-out，避免挂载误触双请求 + 打字即查风暴（宿主 panel 面 30/10s）。
- **refs 降级**：`loadRefs` reject（含超大仓整体失败）→ 选择器退化为 Current/All，不阻塞日志。
- **路径不外泄**：宿主只回稳定 message，git 原文/rev 仅 `eprintln!` 于宿主侧。

## 审查发现并已修复（要点）

- T1：Rev 解析失败/path 非法曾静默返空 → 改 `CoreError::NotFound`/`InvalidInput`（恢复 unknown-oid 语义、T4 映射不为死码）。
- T4：`path=a.txt` 断言恒真（fixture 每提交都改它）→ 加不匹配路径→`Some(0)` 判别。
- T5：转发不可证（stub 丢 action/params）→ 加请求记录器断言出站 `git.refs`/`git.log` params + 畸形结果 reject。
- T6：`Current` 恒不高亮（`undefined` 塌 `null`）；挂载双请求；`loadMore` 失败静默终止分页 → 三条全部修复 + 判别测试 + 翻页 in-flight ref 闸门。

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
- 全量 `pnpm test` 有 3 个与本体无关的时序 flaky（满并行超时、单跑通过），判 flaky 放行。
