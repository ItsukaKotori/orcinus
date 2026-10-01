# Phase 1 子项目 C：终端/PTY + agent（最小纵切） 收尾记录

- 日期：2026-10-01
- 分支：`ItsukaKotori/design-spec-remaining`（基线 `main@c88ae2c`；子项目 B `phase1b-worktree-git` 并行进行中）
- 规格：`docs/superpowers/specs/2026-09-28-phase1c-terminal-pty-agent-design.md`（含 §3.2 修订二与 §10 偏差 1–4）
- 计划：`docs/superpowers/plans/2026-09-28-phase1c-terminal-pty-agent.md`（原任务 1–14 + 修订任务 15–17）
- 执行方式：SDD（subagent-driven-development，每任务独立实现者 + 双阶段审查）；控制器内联完成 Task 3（子代理基建中断期）与文档同步

## 1. 批次与提交序

| 任务 | 内容 | commit |
|---|---|---|
| T1 | orcinus-pty 更名 ade-pty | dc0e947 |
| T2 | WS 环回 server 骨架与 token 鉴权（后随偏差 4 拆除） | 0f52df7 |
| T3 | pty_data_endpoint 与 webview 连通性闸门（控制器内联） | 9ca3f29 |
| T4 | CPR 扫描器跨块扫描与同块双查询修正 | 91cc62d |
| T5 | 会话 spawn、reader 背压与 pre-attach 缓冲 | e0b5dfe |
| T6 | supervisor 回收路径（closer 先断管道 + harvester 兜底） | 34fb593 |
| T7 | WS 会话路由、连接替换与退出关闭语义（1 修复轮） | a587b33 → 47223d7 |
| T8 | shell 解析定型（登录 shell 与 override 规则） | 9b9e3d8 |
| T9 | PtyHost 组装——注册表、事件回调与 shutdown_all | 5225acf |
| T10 | pty 命令面全集——控制、stub 同形与事件广播（1 修复轮） | c42bb3c → 122551b |
| T11 | preflight_refresh_agents——PATH 水合与 agent 探测 | 3e94611 |
| T12 | pty 域接真——WS 客户端与契约方法实现 | 163bd44 |
| T13 | pty 域 parity 迁移 + 端点缓存自愈 + 墓碑门控（1 修复轮） | 004dcb9 → 54393bc |
| 终审修复波 | detectAgents 接真、README 命令、Spawned 事件前移、kill doc | f12fe97 |
| 规格修订 | §3.2 修订一（自定义协议）/ 修订二（Channel）/ 计划修订任务 | 047cb32 → 6c71023 → 3d86194 |
| T15 | 数据面改 Tauri Channel 分块——subscribe + pty_attach，拆除 WS/token 面 | e0c2f6d |
| T16 | 渲染层 Channel 客户端（pty-socket → pty-stream） | 4969bc4 |
| 手工闸门 | `__probeAdePtyStream` = ok + 验收清单（见 §5） | — |

终审（opus 全分支审查）：可合并；1 Important（detectAgents 未接真）+ 2 一行必修，修复波 re-review 4/4 ADDRESSED。

## 2. Crate 与命令面

### 2.1 Rust crate

```
src-tauri/crates/ade-pty/     # 由 spike orcinus-pty 更名；不依赖 tauri
├── lib.rs        # PtyHost：注册表/start/spawn/subscribe/write/resize/signal/kill（2s+2s 升级）/shutdown_all
├── session.rs    # Session：portable-pty、reader 背压、input 通道+writer 线程、exit watch、pid 快照
├── supervisor.rs # closer 先断管道 + harvester 常驻回收（上限 64）
├── server.rs     # attach 接管/替换/pre-attach 排空/退出收尾（two-generation 判定，原 WS 路由层演化为进程内消费面）
├── shell.rs      # resolve_shell/login_args（unix $SHELL -l、windows COMSPEC、override）
├── cpr.rs        # ConPTY CPR 应答（跨块扫描 + tail ≤3 保留，修正 spike clear() 局限）
└── tests/        # throughput（spike 保留）/ session / supervisor / stream / shell / host
```

### 2.2 命令面（ade-bridge，specta 单一登记点 + pty_attach 手工注册）

- 会话：`pty_spawn` `pty_write` `pty_write_accepted` `pty_resize` `pty_signal` `pty_clear_buffer` `pty_kill` `pty_get_cwd` `pty_get_size` `pty_has_pty` `pty_list_sessions`
- 数据面：`pty_attach`（**手工 `__cmd__pty_attach__!` 注册，刻意不在 specta/bindings**——`Channel<InvokeResponseBody>` 不满足 `TSend: Type`；负向契约测试 `pty_data_endpoint_is_gone_and_pty_attach_is_manual_only` 锁定）
- 进程检查/快照/投递 stub 同形：8 条（逐字对齐 web-terminal-api.ts）
- management：`pty_management_list_sessions/kill_all/kill_one/restart/mac_tcc_attribution`（tcc 为 `{health:'unknown'}` 包裹对象）
- preflight：`preflight_refresh_agents`（PATH 水合 + 探测 + 进程内缓存）；渲染层 `refreshAgents` 与 `detectAgents` 双消费

## 3. 数据面终态（§3.2 修订二）与偏差

- **下行**：`pty_attach {args:{id}} + channel` → 宿主 `subscribe(id)`（接管/替换 two-generation 语义）→ backlog 先行 → outbound（128×64KiB 有界）逐块 `InvokeResponseBody::Raw` 转发；≥1KiB 帧走 fetch 队列 ArrayBuffer、<1KiB 走 JSON 数字数组，渲染层 `toDoubleBytes` 双形态解码。
- **上行**：`pty_write`/`pty_write_accepted` 命令（accepted 恒 true）。
- **死亡判定权威**：`pty:exit` 事件唯一；通道静默不判死。
- **§10 偏差 4 证据链**：macOS 26（Darwin 25）对 Tauri app 的 WKWebView 网络子进程环回 WebSocket 静默丢包——服务端 LISTEN 且普通进程 `nc` 秒连、Safari 同页同端口 OPEN→close 1008（服务端与 OS 网络层无碍）、Info.plist `__TEXT,__info_plist` 嵌入无效、系统设置无本地网络条目无法授权。第一次修订（自定义协议流式）被 Tauri API 面否决：responder 为一次性 `FnOnce(Response<Cow<[u8]>>)`（tauri 2.11.5 app.rs:2455-2466，2.12.1 同形），wry macOS 整包投递（wry 0.55.1 url_scheme_handler.rs:280,290）。**终态 = §9.1 原备案 fallback（设计评审已批准）：Tauri Channel 分块**。`macos-info.plist` + build.rs 的 plist 段保留（发布面仍需本地网络声明；bundle Info.plist 优先于段）。
- §10 偏差 1（宿主进程提取推迟 Phase 2）、2（shell-ready 降级 spawn 即投递）、3（crate 更名）如实落地。

## 4. 测试与门禁

- **Rust**：`cargo test -p ade-pty -p ade-bridge` = 174 全绿（ade-pty 31：lib/host/session/shell/supervisor/stream/throughput；ade-bridge 143 含 25+1 命令名锁定、bindings 新鲜度、负向契约）；`cargo test --workspace` 全绿。
- **TS**：`pnpm vitest run src/bridge` = 35 文件 345 用例全绿；`pnpm typecheck && pnpm build:web` exit 0；`pnpm test` 全仓 3849 文件 / 34238 用例全绿（终审与 Task 16 后各复跑一轮）。
- 关键护栏：parity `satisfies Record<keyof PtyApi>` 键集门禁（48+5 键处置强制）+ 契约测试锁命令名/信封 + specta 新鲜度。

## 5. 手工验收结果（用户实测，2026-10-01）

| 项 | 结果 |
|---|---|
| 数据面闸门 `__probeAdePtyStream` | ✅ `ade pty stream: ok`（spawn→attach→Channel→回显全通） |
| 终端 tab（zsh 交互/vim/Ctrl-C） | ✅ |
| split 两 pane 独立会话 | ✅ |
| Claude Code 启动 + 对话 + 提交 | ✅ |
| Codex 启动 + TUI | ✅ |
| 16 MiB `cat /tmp/big-16m.txt`（Channel 吞吐重验） | ✅ 完整到末尾、UI 不冻结 |
| 关 tab 进程回收（无僵尸） | ✅ |
| reload（`location.reload()`；Cmd+R 被应用绑定为重命名 tab） | ✅ 不崩、死亡 tab 可关 |
| 多 tab 退出 | ✅ 正常退出 |
| tab 标题 OSC 抽取 | 未单列验证（渲染层本地路径，无改动面） |

已知缺口（符合预期地缺失）：重启/reload 后终端画面空白（scrollback 恢复 = Phase 2）；agent resume 不可用（会话注册表 = Phase 2）；死会话休眠受限（kill 未知 id 容忍，留档）。

## 6. 延后项

### 6.1 显式延后（Phase 2）

- scrollback 快照与重启恢复、agent 状态识别/hook server、resume、delivery-health 机械、daemon 语义、OSC7 cwd、pending cap（慢消费端宿主侧无上限，Channel 队列吸收）
- incarnationId 穿透数据面（并发 attach 乱序/attach-reject 竞态窗口，Phase 1C 下以 uuid 不复用 + 墓碑缓解）
- kill 未知 id 容忍语义（渲染层 3 处调用点不容错——死 tab 休眠受限）
- resize 集成测试补齐（`tput cols`）

### 6.2 审查 minor 留档（不阻塞合并）

- README.md:18 已修；pid 复用边界注释、Supervisor 单例 doc、Session::kill doc（已顺手修一半）、lib.rs `forward_loop`/`close(1000)` 注释残留、`PtyAttachArgs` 死 derive、`unknown_session` 双份、`PreflightRuntimeContext` 形状漂移（纯装饰）、命令级缓存二次调用测试、探针注释与调用形态不符（devtools 需动态 import）

## 7. 收尾状态

- 自动化门禁全绿 + 手工验收全过；分支待与 `phase1b-worktree-git` 合并后进入 Phase 1 验收闭环「打开 → worktree → agent → 提交」的整体确认。
- Phase 1C 期间规格修订（偏差 4）两轮：WS → 自定义协议（被 API 面否决）→ Channel（终态）；全过程证据与裁定记录于 `.superpowers/sdd/2026-09-28-phase1c-terminal-pty-agent/`（git-ignored 工作区，收口后删除）。
