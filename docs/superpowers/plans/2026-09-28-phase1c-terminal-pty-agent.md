# Phase 1C 终端/PTY + agent 实现计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 把终端从 mock 空转推进到真实可用：进程内 PTY 宿主 + WS 环回数据面 + Tauri 控制面，最小纵切跑起 Claude Code/Codex（新会话）。

**Architecture:** 新 crate `ade-pty`（不依赖 tauri，注入 tokio handle）托管 portable-pty 会话；下行/上行终端字节走 `ws://127.0.0.1:<port>` 二进制帧；控制面（spawn/resize/kill 等）走 Tauri 命令，事件走 `pty:exit`/`pty:spawned`；`preflight.refreshAgents` 以登录 shell PATH 水合 + CLI 探测接真，agent 选择器可用。

**Tech Stack:** Rust（portable-pty 0.9、tokio、tokio-tungstenite、uuid、rand）、Tauri 2 命令 + specta、TS bridge（invoke/listen/WebSocket）。

**Spec:** `docs/superpowers/specs/2026-09-28-phase1c-terminal-pty-agent-design.md`（实现依据；本计划所有语义裁定以其为准）

## Global Constraints

- 分支：当前 worktree 分支 `ItsukaKotori/design-spec-remaining`；全部命令从仓库根 `/Users/itsuka/orca/workspaces/ade/abalone` 执行。
- **契约源（normative，实现前必读）**：`src/shared/preload-api/api/pty-api.ts`、`pty-management-api.ts`、`preflight-api.ts`、`src/shared/pty-listed-session.ts`。迁移期 TS 是契约源；Rust 结构体 `#[serde(rename_all = "camelCase")]` 与 TS 形状**逐字对齐**（字段名、可选性）。
- 命令命名 `<域>_<方法 snake_case>`；参数统一 `{ args }` 包裹（对齐 phase1a/1b 既有命令风格——实现前看一眼 `src-tauri/crates/ade-bridge/src/commands/fs.rs` 的既有包裹方式并保持一致）。
- `ade-pty` **不得依赖 tauri**；运行时以 `tokio::runtime::Handle` 注入。
- spike 要求（`docs/spikes/2026-09-14-pty-throughput.md` §Phase 1 宿主实现要求）1–5 必须逐条落实：CPR 跨块扫描且命中后保留末尾 ≤3 字节；读循环排空 + 背压不丢字节；reader 线程可回收（join 超时 + Reaper）。
- 终端数据通道**不用 Tauri Channel/emit**（仅 `pty:exit`/`pty:spawned` 两个小事件走 emit）。
- 未实现/退化的订阅方法返回 no-op 退订（沿 `src/bridge/mock/noop-unsubscribe.ts` 惯例）；stub 同形返回值逐字对齐 `src/renderer/src/web/preload-api/web-terminal-api.ts`。
- 每个 commit 末尾加：`Co-Authored-By: Claude Code <noreply@anthropic.com>`；commit message 用中文 conventional 格式（`feat(pty): …` / `test(bridge): …` 等）。
- 门禁（Task 13）：`cargo test --workspace` 全绿 + `pnpm typecheck && pnpm build:web` exit 0 + `pnpm test` 全绿。

---

### Task 1: orcinus-pty 更名 ade-pty

**Files:**
- Rename: `src-tauri/crates/orcinus-pty/` → `src-tauri/crates/ade-pty/`
- Modify: `src-tauri/crates/ade-pty/Cargo.toml`（package name）

**Interfaces:**
- Consumes: 无（独立 spike crate，无被依赖方——`grep -r "orcinus-pty" src-tauri --include="*.toml"` 确认仅自身 Cargo.toml）。
- Produces: crate 名 `ade-pty`，供后续任务 `-p ade-pty` 引用；spike 文档复现命令（写作 `-p ade-pty`）与实现一致。

- [ ] **Step 1: 确认无外部引用**

Run: `grep -rn "orcinus-pty" src-tauri --include="*.toml" --include="*.rs" | grep -v "crates/orcinus-pty"`
Expected: 空输出（无其他 crate 依赖它）。

- [ ] **Step 2: 更名并改 package name**

```bash
git mv src-tauri/crates/orcinus-pty src-tauri/crates/ade-pty
```

`src-tauri/crates/ade-pty/Cargo.toml` 第一行 name 改为 `ade-pty`（其余不动）。

- [ ] **Step 3: 构建 + spike 测试验证**

Run: `cargo test -p ade-pty --test throughput`
Expected: PASS（cargo 自动构建 `pty_sink` bin；macOS 上 posix 后端跑通，与 Windows 实测数值无关，只验证功能不验证阈值）。

- [ ] **Step 4: Commit**

```bash
git add -A && git commit -m "refactor(pty): orcinus-pty 更名 ade-pty 对齐 crate 职责表

Co-Authored-By: Claude Code <noreply@anthropic.com>"
```

---

### Task 2: ade-pty 依赖与 WS echo server（鉴权骨架）

**Files:**
- Modify: `src-tauri/crates/ade-pty/Cargo.toml`（加依赖）
- Create: `src-tauri/crates/ade-pty/src/server.rs`
- Modify: `src-tauri/crates/ade-pty/src/lib.rs`（mod 声明）
- Test: `src-tauri/crates/ade-pty/tests/ws_server.rs`

**Interfaces:**
- Consumes: Task 1 的 crate 骨架。
- Produces（Task 7 复用/扩展）:
  - `pub struct DataEndpoint { pub port: u16, pub token: String }`
  - `pub fn generate_token() -> String`（rand 32 字节 hex 化，64 字符）
  - `pub async fn serve(listener: tokio::net::TcpListener, token: String, handler: ConnectionHandler)` — accept 循环；升级请求路径不符 `/pty/<id>` 或 token 不符 → close(1008)；handler 为 `Arc<dyn Fn(String, WsConn) + Send + Sync>`（Task 2 里 echo 实现即抛弃，连接校验逻辑保留）。

- [ ] **Step 1: 加依赖**

```bash
cd src-tauri && cargo add -p ade-pty tokio --features net,rt,sync,time,macros
cargo add -p ade-pty tokio-tungstenite
cargo add -p ade-pty uuid --features v4
cargo add -p ade-pty rand
cargo add -p ade-pty serde --features derive
cargo add -p ade-pty thiserror
```

- [ ] **Step 2: 写失败测试**

`tests/ws_server.rs`：

```rust
use std::sync::Arc;
use tokio_tungstenite::connect_async;

fn spawn_server() -> (ade_pty_test::TestServer, String, u16) { /* 见 Step 4 的测试辅助 */ }
```

> 测试经由 `lib.rs` 暴露的 `#[doc(hidden)] pub mod testing`（或直接 pub server API）进行；两个用例：
> 1. `connects_with_valid_token_and_echoes`：起 server（绑 127.0.0.1:0）→ `connect_async(format!("ws://127.0.0.1:{port}/pty/test-id?token={token}"))` → 发 binary `b"hello"` → 收到同字节回显。
> 2. `rejects_bad_token`：错误 token 连接 → 期待 close（`Message::Close` 或连接错误），收不到回显。

Run: `cargo test -p ade-pty --test ws_server`
Expected: FAIL（`server` 模块不存在，编译错误即失败）。

- [ ] **Step 3: 实现 server.rs**

```rust
use futures_util::{SinkExt, StreamExt};
use tokio::net::TcpListener;
use tokio_tungstenite::tungstenite::protocol::{frame::coding::CloseCode, CloseFrame};

pub struct DataEndpoint { pub port: u16, pub token: String }

pub fn generate_token() -> String {
    use rand::RngCore;
    let mut b = [0u8; 32];
    rand::rng().fill_bytes(&mut b);
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// 逐连接：校验 path/token，回调接管连接。Task 2 的 handler 做 echo；Task 7 换成会话路由。
pub async fn serve(
    listener: TcpListener,
    token: String,
    handler: std::sync::Arc<
        dyn Fn(String, tokio_tungstenite::WebSocketStream<tokio::net::TcpStream>) + Send + Sync,
    >,
) {
    loop {
        let Ok((stream, _)) = listener.accept().await else { continue };
        let token = token.clone();
        let handler = handler.clone();
        tokio::spawn(async move {
            let mut uri = None;
            let ws = tokio_tungstenite::accept_hdr_async(stream, |req, resp| {
                uri = Some(req.uri().clone()); // 闭包借用：实现时改为在回调前先取 URI（accept_hdr_async 回调先于返回），以实际编译为准
                (resp, ())
            })
            .await;
            let mut ws = match ws { Ok(w) => w, Err(_) => return };
            let Some(uri) = uri else { return };
            let authorized = uri.path().strip_prefix("/pty/").map(|id| id.split('?').next().unwrap_or(""))
                .zip(uri.query().and_then(|q| q.split('&').find_map(|kv| kv.strip_prefix("token="))))
                .map(|(id, t)| !id.is_empty() && constant_time_eq(t, &token))
                .unwrap_or(false);
            if !authorized {
                let _ = ws.close(Some(CloseFrame { code: CloseCode::Policy, reason: "unauthorized".into() })).await;
                return;
            }
            let session_id = uri.path().trim_start_matches("/pty/").split('?').next().unwrap_or("").to_string();
            handler(session_id, ws);
        });
    }
}

fn constant_time_eq(a: &str, b: &str) -> bool { /* 长度先比后逐字节 OR 累积，防时序 */ }
```

`futures-util` 以 `cargo add -p ade-pty futures-util` 补充。`lib.rs` 加 `pub mod server;`。
> 注意：`accept_hdr_async` 的回调时机以 tokio-tungstenite 实际 API 为准——若回调无法外提 URI，改用 `accept_async` + 首帧前读 `HttpRequest` 的等价路径（`tokio_tungstenite::accept_hdr_async` 支持，见其文档示例）。测试是行为锚点。

- [ ] **Step 4: 测试辅助与实现测试体**

`lib.rs` 追加（仅为集成测试暴露，`#[cfg(feature = "test-util")]` 或 `#[doc(hidden)]` 皆可，选后者简单）：

```rust
#[doc(hidden)]
pub mod test_support {
    pub fn start_echo_server() -> (u16, String, tokio::task::JoinHandle<()>) {
        let rt = tokio::runtime::Handle::current(); // 测试在 #[tokio::test] 内调用
        let listener = rt.block_on(tokio::net::TcpListener::bind("127.0.0.1:0")).unwrap();
        let port = listener.local_addr().unwrap().port();
        let token = crate::server::generate_token();
        let handle = rt.spawn(crate::server::serve(listener, token.clone(), std::sync::Arc::new(|_id, ws| {
            tokio::spawn(async move {
                let (mut tx, mut rx) = ws.split();
                while let Some(Ok(msg)) = rx.next().await {
                    if let tokio_tungstenite::tungstenite::Message::Binary(b) = msg {
                        if tx.send(tokio_tungstenite::tungstenite::Message::Binary(b)).await.is_err() { break; }
                    }
                }
            });
        })));
        (port, token, handle)
    }
}
```

- [ ] **Step 5: 跑测试至绿**

Run: `cargo test -p ade-pty --test ws_server`
Expected: 2 PASS。

- [ ] **Step 6: Commit**

```bash
git add -A && git commit -m "feat(pty): WS 环回 server 骨架与 token 鉴权

Co-Authored-By: Claude Code <noreply@anthropic.com>"
```

---

### Task 3: webview WS 连通性闸门（风险 #1 前置验证）

**Files:**
- Modify: `src-tauri/crates/ade-bridge/src/state.rs`（AppState 挂 `PtyHost` 骨架字段）
- Modify: `src-tauri/crates/ade-bridge/src/lib.rs` + `specta_export.rs`（注册 `pty_data_endpoint` 命令）
- Create: `src-tauri/crates/ade-bridge/src/commands/pty.rs`（仅此一个命令）
- Create: `src/bridge/real/pty-socket.ts`（含诊断探针，非一次性代码）

**Interfaces:**
- Consumes: Task 2 的 server；phase1a 的 AppState/specta 注册模式（读 `state.rs:289` `AppState` 与 `specta_export.rs` 既有登记方式）。
- Produces:
  - `AppState.pty_host: std::sync::Arc<ade_pty::PtyHost>`（本任务 PtyHost 尚未存在，先以 `OnceLock`/`Option` 占位或直接把 server endpoint 存为 `Mutex<Option<DataEndpoint>>`——实现取后者，Task 9 换成 PtyHost 时一并迁移）
  - Tauri 命令 `pty_data_endpoint` → `DataEndpointPayload { port: u16, token: String }`（camelCase）
  - `src/bridge/real/pty-socket.ts` 导出 `fetchPtyDataEndpoint(): Promise<{port:number; token:string}>`（invoke + 模块级缓存）与诊断函数 `__probeAdePtyWs(): Promise<string>`（连 `ws://…/echo-probe?token=…` 发一字节收一字节，返回描述性结果字符串；Task 7 后此探针改测真实会话路径，保留为常驻诊断）。

- [ ] **Step 1: TS 失败测试**

`src/bridge/real/pty-socket.test.ts`：mock `./invoke` 的 `invokeCommand`，断言 `fetchPtyDataEndpoint` 调 `pty_data_endpoint` 且二次调用不再 invoke（缓存）。Run: `pnpm vitest run src/bridge/real/pty-socket.test.ts` → FAIL。

- [ ] **Step 2: Rust 命令 + 注册**

`commands/pty.rs` 实现 `pty_data_endpoint`（endpoint 由 AppState 持有：orcinus-app setup 中起 server（`tauri::async_runtime::handle()` block_on bind）并存入 AppState）；`lib.rs`/`specta_export.rs` 按既有模式登记。契约测试：`commands/pty.rs` 内 `#[cfg(test)]` 断言 payload 序列化形状 `{"port":…,"token":"…"}`（serde_json 到值比对）。

Run: `cargo test -p ade-bridge` → 新测试 PASS。

- [ ] **Step 3: TS 实现至绿**

`pty-socket.ts` 实现 fetch + 缓存；Run: `pnpm vitest run src/bridge/real/pty-socket.test.ts` → PASS。

- [ ] **Step 4: 手工连通性验证（风险闸门）**

```bash
pnpm dev
```

开发者控制台执行：`(await import('/src/bridge/real/pty-socket.ts')).__probeAdePtyWs()`（或经 window 挂载的等价入口）。
Expected: 日志 `ade pty ws: ok`。
**若 BLOCKED**（WKWebView/WebView2 拒绝 ws://127.0.0.1）：停止后续任务，向用户报告并提议 fallback（数据面退 Tauri Channel 分块投递，接口不变，需改规格 §3.2）——不得自行切换方案。

- [ ] **Step 5: Commit**

```bash
git add -A && git commit -m "feat(bridge): pty_data_endpoint 与 webview WS 连通性闸门

Co-Authored-By: Claude Code <noreply@anthropic.com>"
```

---

### Task 4: CPR 扫描器平移与修正

**Files:**
- Create: `src-tauri/crates/ade-pty/src/cpr.rs`
- Modify: `src-tauri/crates/ade-pty/src/lib.rs`（`pub mod cpr;`）

**Interfaces:**
- Produces（Task 5 reader 循环消费）:
  - `pub struct CprResponder { tail: Vec<u8>, reply: Box<dyn Write + Send> }`（或等价：`scan(&mut self, chunk: &[u8]) -> std::io::Result<bool>` 返回是否需要回写，由调用方持 writer——取后者，纯逻辑可测）
  - `pub fn scan_and_reply(tail: &mut Vec<u8>, chunk: &[u8], writer: &mut dyn Write) -> std::io::Result<usize>`：命中 `ESC[6n` 回写 `ESC[1;1R`，返回命中次数；**保留末尾 ≤3 字节**（修正 spike 命中后 clear() 丢同块后续半条查询的已知局限）。

- [ ] **Step 1: 迁移 spike 单测 + 新增修正用例（先红）**

`cpr.rs` `#[cfg(test)]` 三个用例：①跨块命中（spike `replies_when_query_splits_across_chunks` 平移）；②无命中保留 tail ≤3（spike `keeps_only_partial_query_tail_without_replying` 平移）；③**同块双查询**：输入 `\x1b[6nABC\x1b[6n` → 两次回写、tail 保留 `ABC\x1b[6n` 中的尾巴（预期 tail 非空且以 `\x1b[6n` 结尾可被下一块补全——具体断言：第二次命中后 tail == b"\x1b[6n" 前缀情况按算法推导演算并在测试注释中写明中间态）。

Run: `cargo test -p ade-pty --lib cpr` → FAIL（模块不存在）。

- [ ] **Step 2: 实现至绿**

核心循环：`buf = tail + chunk`；`windows(4)` 找全部非重叠命中位置，逐个 `write_all(CPR_REPLY)`；结束后 `tail = buf[last_hit_end ..]` 的**末尾 ≤3 字节**（若 buf 尾部长度 >3 且末 3 字节内无 `ESC` 开头的部分查询前缀，则 tail 清空——精确规则：保留 buf 末尾的 `\x1b`、`\x1b[`、`\x1b[6` 三种部分前缀之一所在的尾段，否则清空；spike 的「末尾 ≤3 字节」规则保留为简化实现，行为以测试为准）。
Run: `cargo test -p ade-pty --lib cpr` → 3 PASS。

- [ ] **Step 3: Commit**

```bash
git add -A && git commit -m "feat(pty): CPR 扫描器跨块扫描与同块双查询修正

Co-Authored-By: Claude Code <noreply@anthropic.com>"
```

---

### Task 5: Session——spawn、reader 背压、pre-attach 缓冲

**Files:**
- Create: `src-tauri/crates/ade-pty/src/session.rs`
- Modify: `src-tauri/crates/ade-pty/src/lib.rs`
- Test: `src-tauri/crates/ade-pty/tests/session.rs`

**Interfaces:**
- Consumes: Task 4 `scan_and_reply`；portable-pty。
- Produces（Task 7/9 消费）:

```rust
pub struct SpawnRequest {
    pub cols: u16, pub rows: u16,
    pub cwd: Option<String>,
    pub env: std::collections::HashMap<String, String>,
    pub env_to_delete: Vec<String>,
    pub command: Option<String>,
    pub shell_override: Option<String>,
}

pub(crate) struct Session {
    pub id: String,
    // writer 独立 Mutex（输入与 kill 竞争小）；child 用于 kill/wait
    writer: std::sync::Mutex<Box<dyn std::io::Write + Send>>,
    child: std::sync::Mutex<Box<dyn portable_pty::Child + Send + Sync>>,
    pub cwd: String,
    pub size: std::sync::Mutex<(u16, u16)>,
    pub outbound: tokio::sync::mpsc::Sender<Chunk>,        // reader → server；容量 128
    pub pre_attach: std::sync::Arc<std::sync::Mutex<std::collections::VecDeque<Chunk>>>, // 256 KiB 环形
    pub connection: std::sync::Arc<tokio::sync::Mutex<Option<ConnectionSlot>>>, // Task 7 填
}
pub struct Chunk(std::sync::Arc<Vec<u8>>);  // 克隆零拷贝

impl Session {
    pub fn spawn(req: SpawnRequest) -> Result<(Arc<Self>, SessionRuntime), PtyError>;
    pub fn write(&self, bytes: &[u8]) -> std::io::Result<()>;
    pub fn resize(&self, cols: u16, rows: u16) -> Result<(), PtyError>;
}
pub(crate) struct SessionRuntime { pub exit: tokio::sync::oneshot::Receiver<i32>, pub reader_handle: std::thread::JoinHandle<()> }
```

- [ ] **Step 1: 写失败集成测试（unix 守卫 `#[cfg(unix)]`）**

`tests/session.rs`（`#[tokio::test]`）：
1. `echo_flows_to_outbound`：spawn `/bin/sh`（cwd=tmpdir，注入 `env={ADE_PTY_TEST:"1"}`）→ `write(b"echo ok-$ADE_PTY_TEST\n")` → 在 outbound `recv_timeout(5s)` 聚合（循环 recv 直到拼出含 `ok-1`，累计上限 5s）→ assert 命中。
2. `exit_code_propagates`：`write(b"exit 7\n")` → `exit.await == Ok(7)`（容忍 shell 折叠码，断言 `code == 7`）。
3. `env_to_delete_removes_key`：注入 env 后 `write(b"echo ${ADE_PTY_DEL:-missing}\n")`，env_to_delete 含该键 → 输出含 `missing`。
4. `backpressure_pauses_without_loss`：把 outbound 容量榨满（不 recv），`write` 一个 4 MiB 的 `printf` 命令产生大输出 → 恢复 recv → 计数收满预期字节数（无丢失）。
5. `command_written_after_spawn_runs`：`command=Some("echo delivered")` → 聚合输出含 `delivered`。

Run: `cargo test -p ade-pty --test session` → FAIL（编译错误）。

- [ ] **Step 2: 实现 session.rs 至绿**

要点（逐条对应 spike 要求）：
- env 组装：`std::env::vars()` 收集 → 删 `env_to_delete` → 覆盖 `env` → unix 追加 `TERM=xterm-256color`、`COLORTERM=truecolor`（存在则不覆盖）→ `CommandBuilder::envs(map)`；shell 按 Task 8 解析（本任务先 `shell_override.unwrap_or(env SHELL 或 /bin/sh)`，Task 8 替换为完整解析并保持测试兼容）；`-l` 参数 unix 加、命令交付 `write(command + "\n")` 于 spawn 后。
- reader 线程：64 KiB 块 `read` → unix 旁路 CPR（Windows 调 `scan_and_reply`，writer 为 master writer——本任务测试在 unix，CPR 分支以 `#[cfg(windows)]` 编译隔离 + 单测覆盖）→ `loop { outbound.try_send(chunk.clone()) else sleep(5ms) }`（**满即停读，不丢块**）→ 同时 push 进 `pre_attach` 环形（256 KiB 上限，超限弹最旧；首连后由 Task 7 置空并停用）。
- exit 检测：独立 `std::thread` `child.wait()` → `oneshot::Sender<i32>`（unix 退出码；信号死亡取 `-signal`，portable-pty `ExitStatus::exit_code()` 为准）。
- `resize`：`master.resize(PtySize{ cols, rows, ..})` + 更新 `size`。master 需存进 Session（`Mutex`）。
Run: `cargo test -p ade-pty --test session` → 5 PASS。

- [ ] **Step 3: Commit**

```bash
git add -A && git commit -m "feat(pty): 会话 spawn、reader 背压与 pre-attach 缓冲

Co-Authored-By: Claude Code <noreply@anthropic.com>"
```

---

### Task 6: Supervisor——kill 路径与 Reaper

**Files:**
- Create: `src-tauri/crates/ade-pty/src/supervisor.rs`
- Modify: `src-tauri/crates/ade-pty/src/session.rs`（kill）、`lib.rs`
- Test: `src-tauri/crates/ade-pty/tests/supervisor.rs`

**Interfaces:**
- Consumes: Task 5 的 `SessionRuntime.reader_handle`。
- Produces（Task 9 消费）:

```rust
pub struct Supervisor { reaper: std::sync::Mutex<std::collections::VecDeque<std::thread::JoinHandle<()>>> } // 上限 64，超限 pop_front
impl Supervisor {
    pub fn new() -> Self;
    /// 先 drop closer（断管道）再 join(2s)；超时入 Reaper。
    pub fn reap(&self, closer: impl FnOnce(), handle: std::thread::JoinHandle<()>);
}
// Session::kill(&self, sup: &Supervisor) -> i32  // child.kill + drop master writer/reader + 摘除 + 返回退出码
```

- [ ] **Step 1: 失败测试**

`tests/supervisor.rs`：①`join_completes_after_close`（真 spawn `/bin/sh`，kill 后 reap 在 2s 内返回）；②`timeout_goes_to_reaper`（构造一个阻塞到底的假 reader 线程 + no-op closer → reap 返回后 Reaper 长度 1）；③`reaper_evicts_oldest_over_cap`（灌 65 个假句柄 → 长度封顶 64）。①为集成、②③纯逻辑。
Run: `cargo test -p ade-pty --test supervisor` → FAIL。

- [ ] **Step 2: 实现至绿**

`reap`：`closer();` → `join_handle.join_timeout`——std 无 join 超时：实现为 flag+condvar 不可行（join 本身阻塞）。**采用 mpsc 模式**：reap 不直接 join；把 handle 移入 `pending` 列表，由一个常驻 `harvester` 线程逐个 `join()`（阻塞）；`reap` 立即返回。超时语义退化为「最终回收」——**修正接口**：`pub fn reap(&self, closer: impl FnOnce(), handle: JoinHandle<()>)` 立即返回，closer 保证 reader 很快 EOF，harvester 兜底泄漏线程。测试 ②③ 相应断言 Reaper/harvester 行为（②closer 真断管道则最终 join 成功，用假 handle 验证队列上限）。**以行为测试为准，接口与上述签名一致，内部实现自由。**
Run: 3 PASS。

- [ ] **Step 3: Commit**

```bash
git add -A && git commit -m "feat(pty): supervisor 回收路径（closer 先断管道 + harvester 兜底）

Co-Authored-By: Claude Code <noreply@anthropic.com>"
```

---

### Task 7: WS 路由会话（双向帧、连接替换、关闭语义）

**Files:**
- Modify: `src-tauri/crates/ade-pty/src/server.rs`（handler 换会话路由）、`session.rs`（ConnectionSlot）
- Test: `src-tauri/crates/ade-pty/tests/ws_session.rs`

**Interfaces:**
- Consumes: Task 2 server 骨架、Task 5 Session。
- Produces: PtyHost 的传输层最终形态。`ConnectionSlot { outbound_rx: mpsc::Receiver<Chunk>, pre_attach: Arc<Mutex<VecDeque<Chunk>>> }`——接管连接的任务：先排空 pre_attach 逐帧发（发完清空并置 `disabled` 标志，reader 从此不再写入 pre_attach），再循环 `outbound_rx.recv()` → `Message::Binary`；上行 `Message::Binary(data)` → `session.write(data)`；`Close/Err` → 连接槽清空。

- [ ] **Step 1: 失败集成测试（unix）**

`tests/ws_session.rs`（`#[tokio::test]`）：完整链路——起 host 级 server（会话路由版）+ spawn `/bin/sh`：
1. `roundtrip_input_output_over_ws`：连 `ws://…/pty/{id}?token=` → 发 `echo ws-ok\n` → 收帧聚合含 `ws-ok`。
2. `pre_attach_buffer_replayed_on_first_connect`：spawn 后**先不连**，等 300ms（写入 `echo early\n` via write API）→ 再连 → 首帧区含 `early`。
3. `connection_replaced`：连接 A 保持 → 连接 B 同 id → A 收 Close；B 继续收发正常。
4. `close_on_exit`：`write(b"exit 0\n")` → WS 收到 Close 帧。
Run: FAIL（路由未接）。

- [ ] **Step 2: 实现至绿**

server 的 handler 改为：查注册表（由测试装配的 `Arc<SessionsMap>` 注入；Task 9 并入 PtyHost）→ 旧连接槽 `close`（向其 outbound 通道 drop + tungstenite close）→ 建新槽接管。exit 收尾顺序（规格 §3.2）：排空 outbound → `ws.close(1000)` → 从注册表摘除。
Run: 4 PASS。

- [ ] **Step 3: Commit**

```bash
git add -A && git commit -m "feat(pty): WS 会话路由、连接替换与退出关闭语义

Co-Authored-By: Claude Code <noreply@anthropic.com>"
```

---

### Task 8: shell 解析与 env/cwd 规则定型

**Files:**
- Create: `src-tauri/crates/ade-pty/src/shell.rs`
- Modify: `src-tauri/crates/ade-pty/src/session.rs`（改用 shell.rs）
- Test: `src-tauri/crates/ade-pty/tests/shell.rs`

**Interfaces:**
- Produces（Task 5 已有临时实现，本任务定型）:

```rust
pub struct ShellSpec { pub program: String, pub args: Vec<String> }
/// unix：shell_override > $SHELL > /bin/zsh > /bin/bash > /bin/sh；存在时 args=["-l"]
/// windows：shell_override > $COMSPEC（默认 "cmd.exe"）；args=[]
pub fn resolve_shell(platform: &str, shell_override: Option<&str>, env_shell: Option<&str>, env_comspec: Option<&str>) -> ShellSpec;
pub fn login_args(platform: &str, program: &str) -> Vec<String>; // unix=["-l"]（fish/cmd 等同规则，1C 不特判），windows=[]
```

- [ ] **Step 1: 失败测试**

`tests/shell.rs` 纯函数表驱动用例：unix override 命中 / $SHELL 命中 / 全缺省回退 / windows COMSPEC / windows override（powershell.exe）。Run: FAIL。

- [ ] **Step 2: 实现至绿；session.rs 换用**

Run: `cargo test -p ade-pty --lib shell && cargo test -p ade-pty --test session` → 全 PASS（session 集成测试证明回归无破坏）。

- [ ] **Step 3: Commit**

```bash
git add -A && git commit -m "feat(pty): shell 解析定型（登录 shell 与 override 规则）

Co-Authored-By: Claude Code <noreply@anthropic.com>"
```

---

### Task 9: PtyHost 组装（注册表、事件回调、shutdown_all）

**Files:**
- Modify: `src-tauri/crates/ade-pty/src/lib.rs`（PtyHost 完整体；session server 并入）
- Test: `src-tauri/crates/ade-pty/tests/host.rs`

**Interfaces:**
- Consumes: Task 5/6/7/8 全部。
- Produces（Task 10 消费——这就是 ade-bridge 看到的全部面）:

```rust
pub struct PtyHost { /* sessions: Arc<Mutex<HashMap<String, Arc<Session>>>>, supervisor, endpoint, exit_cb: Mutex<Option<Box<dyn Fn(ExitInfo) + Send + Sync>>> */ }
#[derive(Clone, serde::Serialize)] #[serde(rename_all="camelCase")]
pub struct ExitInfo { pub id: String, pub code: i32 }

impl PtyHost {
    pub fn start(handle: tokio::runtime::Handle) -> Result<Arc<Self>, PtyError>; // 起 WS server（127.0.0.1:0）
    pub fn endpoint(&self) -> DataEndpoint;
    pub fn spawn(&self, req: SpawnRequest) -> Result<String, PtyError>; // id；内部发 spawned 交给回调（Task 10 由 bridge 转事件）
    pub fn set_event_callback(&self, cb: Box<dyn Fn(PtyEvent) + Send + Sync>);
    pub fn write(&self, id: &str, bytes: Vec<u8>) -> Result<(), PtyError>;
    pub fn write_accepted(&self, id: &str, bytes: Vec<u8>) -> Result<bool, PtyError>;
    pub fn resize(&self, id: &str, cols: u16, rows: u16) -> Result<(), PtyError>;
    pub fn signal(&self, id: &str, sig: &str) -> Result<(), PtyError>; // unix: process_id + libc::kill；windows: 仅 SIGTERM/SIGKILL→kill，其余 Err
    pub fn kill(&self, id: &str) -> Result<i32, PtyError>; // 返回退出码；keepHistory 由 bridge 层忽略
    pub fn clear_buffer(&self, id: &str) -> Result<(), PtyError>;
    pub fn get_cwd(&self, id: &str) -> Option<String>;
    pub fn get_size(&self, id: &str) -> Option<(u16, u16)>;
    pub fn has_pty(&self, id: &str) -> bool;
    pub fn list_sessions(&self) -> Vec<ListedSession>; // {id, cwd, title:"", worktreeId:None(bridge 层不知道，见下), agentOwnership:"unknown"}
    pub fn is_alive(&self, id: &str) -> bool;          // reattach 判定（sessionId 命中活会话）
    pub fn shutdown_all(&self); // 逐会话 kill + server close
}
pub enum PtyEvent { Spawned { id: String }, Exit(ExitInfo) }
```

> `worktreeId` 投影：spawn 时 bridge 知道 worktreeId 而 host 不关心——`PtyHost::spawn` 返回 id 后由 bridge 记 `id→worktreeId` 的映射（Task 10 在 bridge 侧持 `Mutex<HashMap<String,String>>`），list_sessions 时 host 回 id/cwd、bridge 补 worktreeId/title。`ListedSession` 在 host 层就只含 id/cwd/agentOwnership 占位，最终形状由 bridge 组装（契约形状见 `src/shared/pty-listed-session.ts`）。

- [ ] **Step 1: 失败集成测试（unix）**

`tests/host.rs`：①spawn→write→echo 经 WS 到达（复用 Task 7 客户端手法）；②kill 返回退出码且 `has_pty==false`、exit 回调触发；③`is_alive` reattach 判定；④4 会话并发独立；⑤`signal("SIGINT")` 对 `sleep 60` 的 sh 会使 wait 返回非零（unix）；⑥shutdown_all 后全部摘除。
Run: FAIL。

- [ ] **Step 2: 实现至绿**

Run: `cargo test -p ade-pty` 全量 → 全 PASS（含此前全部回归）。

- [ ] **Step 3: Commit**

```bash
git add -A && git commit -m "feat(pty): PtyHost 组装——注册表、事件回调与 shutdown_all

Co-Authored-By: Claude Code <noreply@anthropic.com>"
```

---

### Task 10: ade-bridge pty 命令面（控制 + stub 同形 + 事件）

**Files:**
- Modify: `src-tauri/crates/ade-bridge/src/commands/pty.rs`（Task 3 的文件扩为完整命令面）
- Modify: `src-tauri/crates/ade-bridge/src/state.rs`（AppState：`pty_host: Arc<PtyHost>` 替换 Task 3 占位、`pty_worktree_ids: Mutex<HashMap<String,String>>`、orcinus-app setup 装配 + exit/spawned 回调转 Tauri emit）
- Modify: `src-tauri/crates/ade-bridge/src/lib.rs`、`specta_export.rs`（登记全部命令与类型）
- Test: `src-tauri/crates/ade-bridge/src/commands/pty.rs` `#[cfg(test)]`

**Interfaces:**
- Consumes: Task 9 PtyHost 全部方法。
- Produces（§5 命令面全集；bridge 层职责）:
  - `pty_spawn`：args 按 `pty-api.ts` `spawn` opts 对齐（生效字段见规格 §2.4）；**cwd 解析**：`args.cwd` 优先 → `cwdFallback==='worktree'` 且 `worktreeId` 可解析（`id.split_once("::")` 取 path 段，校验 `Path::new(p).is_dir()`）→ home（`std::env` home crate 或 `dirs`，沿 ade-core 既有 home 获取方式）→ spawn 成功后记 `pty_worktree_ids[id]=worktreeId`（若有）→ emit `pty:spawned {id}` → 回 `{id}`（`isReattach:true` 仅当 `args.sessionId` 命中 `is_alive`）。
  - `pty_signal`：透传字符串；`pty_kill`：忽略 `keepHistory`；`pty_list_sessions`：host list + bridge 补 `worktreeId`/`title:''`/`agentOwnership:'unknown'`（对齐 `PtyListedSession`）。
  - stub 同形命令（§2.2 逐字）：`pty_get_foreground_process`/`pty_confirm_foreground_process` → Ok(null)；`pty_has_child_processes` → Ok(false)；`pty_get_main_buffer_snapshot` → Ok(null)；`pty_get_authoritative_buffer_snapshot_capabilities` → 逐 id `{id, authoritative:null}`（web stub 为 false；规格 §2.2 允许 null/false 任一，**取 false** 对齐 web stub）；`pty_inspect_process` → Err(BridgeError::message("terminal_liveness_unavailable"))；`pty_report_renderer_delivery_state` → Ok(`{inFlightTotalChars:0, inFlightPtyCount:0, msSinceLastAck:null}`)；`pty_get_renderer_delivery_debug_snapshot` → Ok(web stub 零对象逐字)。
  - management：`pty_management_list_sessions/kill_all/kill_one/restart/mac_tcc_attribution`（restart→`{success:true}`；tcc→`{health:"unknown"}`）。
  - 事件：`set_event_callback` 里 `app_handle.emit("pty:spawned", …)` / `app_handle.emit("pty:exit", …)`（`tauri::Emitter` trait，沿 phase1a 事件用法）。
- [ ] **Step 1: 失败契约测试**

`commands/pty.rs` `#[cfg(test)]`（tauri test runtime 或纯 serde 断言，沿 `commands/fs.rs` 既有测试风格）：
1. 命令名锁定：对每个命令函数断言 `#[tauri::command]` 生成的注册名（沿 1A 契约测试手法——读 `commands/fs.rs` 既有测试照抄模式）。
2. `pty_spawn` 参数反序列化：JSON `{"args":{"cols":80,"rows":24,"cwd":"/tmp","cwdFallback":"worktree","worktreeId":"r1::/tmp/x","env":{"K":"V"},"envToDelete":["E"],"command":"echo hi"}}` → 结构体字段全中。
3. spawn 返回形状 `{"id":"…"}` 序列化断言。
4. stub 同形返回值逐字断言（§2.2 五件套）。
5. management 返回形状断言。
Run: `cargo test -p ade-bridge pty` → FAIL。

- [ ] **Step 2: 实现全部命令至绿**

Run: `cargo test -p ade-bridge` → 全 PASS（specta 新鲜度测试一并跑）。

- [ ] **Step 3: specta 登记 + build 验证**

`specta_export.rs` 登记 17 个命令与新增类型（`ExitInfo`、`ListedSession` 投影、endpoint payload 等）；Run: `cargo test -p ade-bridge` 含 bindings 新鲜度 → PASS；`cargo build -p orcinus-app` → exit 0。

- [ ] **Step 4: Commit**

```bash
git add -A && git commit -m "feat(bridge): pty 命令面全集——控制、stub 同形与事件广播

Co-Authored-By: Claude Code <noreply@anthropic.com>"
```

---

### Task 11: preflight_refresh_agents（PATH 水合 + 探测）

**Files:**
- Create: `src-tauri/crates/ade-bridge/src/commands/preflight.rs`（或并入既有 preflight 命令文件——读 `commands/` 确认 `preflight` 域当前归属后二选一，保持域聚合惯例）
- Modify: `lib.rs`、`specta_export.rs`（登记）
- Test: 同文件 `#[cfg(test)]`

**Interfaces:**
- Consumes: 契约 `RefreshAgentsResult`（`preflight-api.ts:31-38`）：`{agents: string[], addedPathSegments: string[], shellHydrationOk: boolean, pathSource: 'shell_hydrate'|'sync_seed_only', pathFailureReason: 'none'|'no_shell'|'timeout'|'spawn_error'|'empty_path'}`。
- Produces:
  - `pub fn hydrate_login_path(platform: &str, shell: &str, timeout: Duration) -> PathHydration`（`PathHydration { path: Option<String>, failure: FailureKind }`）——`shell -l -c 'echo $PATH'`（unix），取**最后一个非空行**且须含路径分隔符，2s 超时；windows 直接 `failure: no_shell, path: None`。
  - `pub fn detect_agents(path_var: &str, probes: &[&str], platform: &str) -> Vec<String>`——unix 逐目录试 `<dir>/<probe>` 可执行；windows 逐目录 × PATHEXT（`.com/.exe/.cmd/.bat`）。
  - 命令 `preflight_refresh_agents`：水合（进程内 `OnceLock` 缓存）→ `addedPathSegments` = 水合 PATH 段 − app 进程 PATH 段（保持水合序）→ `pathSource`/`pathFailureReason` 归类 → `agents = detect_agents(有效PATH, ["claude","codex"])`。

- [ ] **Step 1: 失败测试**

纯函数测试：①水合解析（假 shell：用 `sh -l -c` 于 tmpdir 或直接对解析函数喂多行输出样本断言取尾行）；②失败归类（timeout 用 1ms 超时 + `sleep` shell）；③detect_agents 在 tmpdir 布置假可执行命中；④windows PATHEXT 逻辑（platform 参数注入测试）。命令级：返回形状 serde 断言（`pathSource` 枚举 camelCase 原样小写）。
Run: `cargo test -p ade-bridge preflight` → FAIL。

- [ ] **Step 2: 实现至绿 + 登记**

Run: `cargo test -p ade-bridge` 全量 PASS。

- [ ] **Step 3: Commit**

```bash
git add -A && git commit -m "feat(bridge): preflight_refresh_agents——登录 shell PATH 水合与 agent 探测

Co-Authored-By: Claude Code <noreply@anthropic.com>"
```

---

### Task 12: 渲染层 real/pty.ts + pty-socket.ts

**Files:**
- Create: `src/bridge/real/pty.ts`
- Modify: `src/bridge/real/pty-socket.ts`（Task 3 骨架扩为完整客户端：按会话连接、异常关闭→本地 exit 广播）
- Modify: `src/bridge/create-api.ts`（`pty` 加入 `RealDomains` 与 `createRealDomains`）
- Test: `src/bridge/real/pty.test.ts`

**Interfaces:**
- Consumes: Task 10/11 全部命令；契约 `PreloadApi['pty']`（`pty-api.ts`）。
- Produces: `createPtyRealApi(): PreloadApi['pty']`；内部：
  - emitter（`onData`/`onExit`/`onSpawned` 订阅者表；`getPtyDataListenerCount` 回 onData 监听数）
  - WS hub：spawn 成功后 `fetchPtyDataEndpoint()`（缓存）→ `new WebSocket(ws://…/pty/<id>?token=…)` → `binaryType='arraybuffer'` → `message` → `onData({id, data: utf8Decode(buf), rawLength: buf.byteLength})`；`onclose`（非会话退出触发的关闭）→ 视为会话死亡 → 本地 `onExit({id, code:-1})`
  - `write(id, s)`/`writeAccepted(id, s)` → WS `send(utf8Encode(s))`，accepted 回 true（WS 就绪时），未连接回 false/void
  - Tauri listen：`pty:exit` → 转发 onExit + 关闭对应 WS；`pty:spawned` → 转发
  - noop 集：规格 §2.3 清单（订阅返回 `noopUnsubscribe` from `../mock/noop-unsubscribe`）
  - stub 同形集：规格 §2.2 清单（值逐字照抄 web stub）
  - `ackData`/`ackColdRestore`/`claimViewport`/`reportGeometry`/`setActiveRendererPty` 等无返回方法 → `() => {}`

- [ ] **Step 1: 失败测试**

`src/bridge/real/pty.test.ts`（沿 `src/bridge/real/fs.test.ts` 的 mock invoke/listen 风格 + stub 全局 WebSocket 类）：
1. `spawn_invokes_pty_spawn_with_args_wrapper`：断言命令名与 `{args}` 包裹、返回透传 `{id}`、`sessionId` 命中时回传 `isReattach:true`。
2. `on_data_fans_out_from_ws_frames`：伪造 WebSocket 实例触发 message → onData 收 `{id,data,rawLength}`。
3. `ws_abnormal_close_emits_local_exit`。
4. `tauri_exit_event_closes_ws_and_forwards`。
5. `write_sends_binary_frame_and_write_accepted_true`。
6. stub 同形五件套逐字断言（对照 web-terminal-api.ts 值）。
7. noop 订阅返回函数且重复退订安全。
8. `list_sessions` 形状断言（invoke 透传）。
Run: `pnpm vitest run src/bridge/real/pty.test.ts` → FAIL。

- [ ] **Step 2: 实现至绿**

Run: `pnpm vitest run src/bridge/real/pty.test.ts src/bridge/real/pty-socket.test.ts` → PASS。

- [ ] **Step 3: create-api 接入**

`create-api.ts`：`RealDomains` 联合加 `'pty'`；`createRealDomains()` 加 `pty: createPtyRealApi()`；import 补齐。Run: `pnpm vitest run src/bridge/create-api.test.ts` → PASS（若该测试锁域清单需同步更新，按其现有断言方式改）。

- [ ] **Step 4: Commit**

```bash
git add -A && git commit -m "feat(bridge): pty 域接真——WS 客户端与契约方法实现

Co-Authored-By: Claude Code <noreply@anthropic.com>"
```

---

### Task 13: parity 迁移与全门禁

**Files:**
- Modify: `src/bridge/real/parity.test.ts`（pty/preflight.refreshAgents 方法从「未实现」清单迁出，迁入契约断言组——读该文件现有结构照做）
- Modify: `src/bridge/create-api.test.ts`（若锁 real 域清单）

**Interfaces:**
- Consumes: Task 12 的实现。
- Produces: 门禁全绿状态。

- [ ] **Step 1: parity 迁移**

Run: `pnpm vitest run src/bridge/real/parity.test.ts` → 观察失败项 → 按其清单结构把 pty 域迁出「响亮未实现」组、补最小契约断言（同 Task 12 测试子集引用即可，不重复实现）。

- [ ] **Step 2: 全门禁**

```bash
cargo test --workspace
pnpm typecheck && pnpm build:web
pnpm test
```
Expected: 三者全绿。`pnpm test` 若出现与 pty mock 语义耦合的既有测试失败（mock 域保留，理论上不应有），逐个修复并在 commit message 说明。

- [ ] **Step 3: Commit**

```bash
git add -A && git commit -m "test(bridge): pty 域 parity 迁移与全量门禁

Co-Authored-By: Claude Code <noreply@anthropic.com>"
```

---

### Task 14: 手工验收与收尾记录

**Files:**
- Create: `docs/phase1c-terminal-pty-agent-record.md`

**Interfaces:** 无（验收 + 记录）。

- [ ] **Step 1: 手工验收清单（`pnpm dev`，对照规格 §1）**

逐项验证并记录实际结果：
1. 新建终端 tab：zsh prompt 出现（macOS）；键入 `ls`/`vim`/Ctrl-C 正常。
2. split：两 pane 独立 shell，各自 `echo $$` 不同。
3. Claude Code：worktree 侧栏点选启动 → TUI 渲染正常 → 完成一次真实对话与文件修改 → `/commit` 或手动 `git commit`（依赖 Task 后 B 分支合入情况；若 B 未合入则在 tab 里手动 git 操作验证 agent 可驱动提交）。
4. Codex：同验启动与 TUI。
5. `cat` ≥16 MiB 文件：无丢字节（末尾完整）、UI 不冻结。
6. 关 tab：进程消失（`ps` 验证）、无僵尸。
7. webview reload（Cmd+R）：tab 重挂不崩；终端空白为已知缺口，死亡 tab 可关闭重开。
8. 设置页 agent 选择器（若渲染层入口可见）显示本机探测到的 claude/codex。

- [ ] **Step 2: 收尾记录**

`docs/phase1c-terminal-pty-agent-record.md`：实现清单、与规格偏差的最终核对（§10 三条）、已知缺口（§1 记录项）、遗留跟踪项（Windows 本机验证、CSP 启用时的 connect-src、Phase 2 提取宿主的接缝说明）。格式沿 `docs/phase1a-open-project-record.md`。

- [ ] **Step 3: Commit**

```bash
git add -A && git commit -m "docs: Phase 1 子项目 C 终端/PTY + agent 收尾记录

Co-Authored-By: Claude Code <noreply@anthropic.com>"
```

---

## Self-Review

1. **Spec coverage**：规格 §2.1 处置表 → Task 10（控制面/stub/management）+ Task 12（渲染层）；§3.1 crate → Task 1/5/7/9；§3.2 协议 → Task 2/7；§4.1–4.7 → Task 5/6/8/9/2；§5 命令清单 → Task 10 全枚举；§6 桥接 → Task 3/12/13；§7 refreshAgents → Task 11；§8 测试策略 → 各 Task 测试步 + Task 13 门禁；§1 验收 → Task 14；§9 风险 1 → Task 3 闸门、风险 2 → Task 5 测试 4 + Task 14 第 5 项、风险 3 → Task 11 水合、风险 4 → Task 14 第 7 项。**无缺口**。
2. **Placeholder scan**：Task 2 Step 3 的 `constant_time_eq` 标注了实现要求（长度先比逐字节 OR）；Task 6 的 supervisor 接口在 Step 2 中显式修正了「join 超时」为 harvester 模式并要求以行为测试为准——已消除假接口；无 TBD/TODO。
3. **Type consistency**：`SpawnRequest`/`Chunk`/`DataEndpoint`/`ExitInfo`/`PtyEvent` 在 Task 5/9/10 间签名一致；`scan_and_reply`（Task 4）被 Task 5 引用名一致；`fetchPtyDataEndpoint`（Task 3）被 Task 12 引用一致；`noopUnsubscribe` 路径 `../mock/noop-unsubscribe` 与仓库现有 `src/bridge/mock/noop-unsubscribe.ts` 一致。
