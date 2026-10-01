pub mod cpr;
pub mod server;
pub mod session;
pub mod shell;
pub mod supervisor;

/// crate 级错误类型：portable-pty 的公开 API（openpty/spawn_command/take_writer/
/// resize 等）返回 `anyhow::Error`，本地 io 失败（线程启动等）归入 `Io`。
#[derive(Debug, thiserror::Error)]
pub enum PtyError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("pty error: {0:#}")]
    Pty(#[from] anyhow::Error),
}

use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use std::io::Read;
use std::path::Path;
use std::sync::mpsc;
use std::time::{Duration, Instant};

pub struct ThroughputReport {
    pub received_bytes: u64,
    pub chunk_bytes: usize,
    pub elapsed: Duration,
    pub mb_per_second: f64,
}

/// Upper bound for one measurement run; on expiry the sink is killed so a stalled
/// PTY fails as `received_bytes < total_bytes` instead of hanging the caller.
pub const READ_DEADLINE: Duration = Duration::from_secs(30);

pub fn measure_pty_throughput(
    sink_exe: &Path,
    total_bytes: usize,
) -> std::io::Result<ThroughputReport> {
    let pty = native_pty_system();
    let pair = pty
        .openpty(PtySize {
            rows: 40,
            cols: 120,
            pixel_width: 0,
            pixel_height: 0,
        })
        .map_err(std::io::Error::other)?;
    let mut cmd = CommandBuilder::new(sink_exe);
    cmd.arg(total_bytes.to_string());
    let mut child = pair
        .slave
        .spawn_command(cmd)
        .map_err(std::io::Error::other)?;
    drop(pair.slave);
    let mut writer = pair.master.take_writer().map_err(std::io::Error::other)?;
    let mut reader = pair
        .master
        .try_clone_reader()
        .map_err(std::io::Error::other)?;
    let chunk_bytes = 64 * 1024;
    let (chunk_tx, chunk_rx) = mpsc::channel::<Vec<u8>>();
    let reader_thread = std::thread::spawn(move || {
        let mut buf = vec![0u8; chunk_bytes];
        loop {
            match reader.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    if chunk_tx.send(buf[..n].to_vec()).is_err() {
                        break;
                    }
                }
            }
        }
    });
    let started = Instant::now();
    let mut received = 0usize;
    let mut scan_tail: Vec<u8> = Vec::new();
    while received < total_bytes {
        // WHY: bounded wait guards the ConPTY startup block (an unanswered CPR query, see
        // cpr::scan_and_reply, leaves the pipe silent); a stall must fail, not hang.
        let remaining = READ_DEADLINE.saturating_sub(started.elapsed());
        if remaining.is_zero() {
            break;
        }
        match chunk_rx.recv_timeout(remaining) {
            Ok(chunk) => {
                cpr::scan_and_reply(&mut scan_tail, &chunk, &mut writer)?;
                received += chunk.len();
            }
            Err(_) => break,
        }
    }
    let elapsed = started.elapsed();
    let _ = child.kill();
    drop(pair.master);
    // NOTE: not joined on purpose; on Windows the ConPTY pipe can stay open after the
    // child is killed, so joining would reintroduce the hang. The reader dies with the process.
    drop(reader_thread);
    Ok(ThroughputReport {
        received_bytes: received as u64,
        chunk_bytes,
        elapsed,
        mb_per_second: (received as f64 / 1024.0 / 1024.0) / elapsed.as_secs_f64(),
    })
}

/// 数据面连通性探针的 echo 装配（crate 级 pub fn，非测试模块）：Task 3 由
/// orcinus-app setup 直接调用（`test_support` 模块随 Task 7 会话路由退役，函数
/// 平移至此保持调用点可用）；`tests/ws_server.rs` 亦复用。Task 9 `PtyHost::start`
/// 接管后，连同 orcinus-app 侧调用整块移除。
#[doc(hidden)]
pub fn start_echo_server() -> (u16, String, tokio::task::JoinHandle<()>) {
    // WHY: `Handle::block_on` 在运行时上下文内调用会 panic（"Cannot start a
    // runtime from within a runtime"），故先以 std 绑定端口，再在当前运行时
    // 上下文里注册为异步 listener——签名与行为不变。
    let std_listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = std_listener.local_addr().unwrap().port();
    std_listener.set_nonblocking(true).unwrap();
    let listener = tokio::net::TcpListener::from_std(std_listener).unwrap();
    let token = server::generate_token();
    let handler: server::ConnectionHandler = std::sync::Arc::new(|_id, ws| {
        use futures_util::{SinkExt, StreamExt};
        tokio::spawn(async move {
            let (mut tx, mut rx) = ws.split();
            while let Some(Ok(msg)) = rx.next().await {
                if let tokio_tungstenite::tungstenite::Message::Binary(b) = msg {
                    if tx
                        .send(tokio_tungstenite::tungstenite::Message::Binary(b))
                        .await
                        .is_err()
                    {
                        break;
                    }
                }
            }
        });
    });
    let handle = tokio::spawn(server::serve(listener, token.clone(), handler));
    (port, token, handle)
}

// ===== Task 9：PtyHost 组装（ade-bridge 将看到的全部门面） =====
//
// 注册表（与 `server::route_connection` 共用同一张表）+ 事件回调 + kill 升级
// 路径 + shutdown_all。生命周期：`PtyHost::start` 起 WS server（127.0.0.1:0，
// 运行在传入的 tokio Handle 上）→ spawn/写入/信号/kill → `shutdown_all` 全量
// 收尾。会话退出事件源是 `Session.exited`（watch），摘除与 Task 7 的
// forward_loop 共用 hub 代次判定语义（见 [`HostState::spawn_exit_watcher`]）。

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use crate::server::{ConnectionHandler, DataEndpoint, Sessions};
use crate::session::Session;
use crate::supervisor::Supervisor;

/// [`PtyHost::spawn`] 的请求形状；门面处再导出（Task 10 的 bridge 命令层
/// 组装用，不必深入 `session` 模块路径）。
pub use crate::session::SpawnRequest;

/// 会话退出事件载荷（camelCase serde，Task 10 经 Tauri emit 给前端）。
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExitInfo {
    pub id: String,
    pub code: i32,
}

/// PtyHost 事件：spawn 成功 / 会话退出。Task 10 由 bridge 回调转 Tauri 事件
/// （`pty:spawned` / `pty:exit`）。
#[derive(Debug, Clone, serde::Serialize)]
pub enum PtyEvent {
    Spawned { id: String },
    Exit(ExitInfo),
}

/// [`PtyHost::list_sessions`] 的 host 层投影：host 只回答它知道的三元组，
/// worktreeId/title 由 bridge 侧补齐（契约形状见 `src/shared/pty-listed-session.ts`）。
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ListedSession {
    pub id: String,
    pub cwd: String,
    /// host 层恒 `"unknown"`（占位——host 无 agent 证据，不得谎报 absent）。
    pub agent_ownership: String,
}

type EventCallback = Box<dyn Fn(PtyEvent) + Send + Sync>;

/// kill 等退出码的两段时限：第一段超时升级 SIGKILL，第二段再超时返回 -1。
const KILL_WAIT: std::time::Duration = std::time::Duration::from_secs(2);

fn unknown_session(id: &str) -> PtyError {
    PtyError::Io(std::io::Error::other(format!("unknown session: {id}")))
}

/// 宿主门面：Task 10 由 bridge 持 `Arc<PtyHost>` 装配命令面。
pub struct PtyHost {
    inner: Arc<HostState>,
}

/// 共享内核：exit watcher 任务与各方法经 `Arc<HostState>` 共享。
struct HostState {
    /// server 与 exit watcher 的运行时（`PtyHost::start` 传入）。
    handle: tokio::runtime::Handle,
    endpoint: DataEndpoint,
    /// 进程级单例（Task 6 交接：勿每会话建）。
    sup: Arc<Supervisor>,
    /// 会话注册表：与 `server::route_connection` 共用同一张表。
    sessions: Sessions,
    /// 每会话 reader 句柄（Task 6 交接：kill 时传 `session.kill(&sup, handle)`）。
    readers: Mutex<HashMap<String, std::thread::JoinHandle<()>>>,
    event_cb: Mutex<Option<EventCallback>>,
    /// accept 循环任务；`shutdown_all` 时 abort（= server close）。
    serve: Mutex<Option<tokio::task::JoinHandle<()>>>,
}

impl HostState {
    /// 同步回调分发。持锁调用：回调内**不得**再进 `set_event_callback`
    /// （会自锁）；Task 10 的回调是 Tauri emit——同步快速、无重入。
    fn emit(&self, event: PtyEvent) {
        let guard = self.event_cb.lock().expect("event callback mutex poisoned");
        if let Some(cb) = guard.as_ref() {
            cb(event);
        }
    }

    /// 退出观察者（Task 7 交接）：克隆 [`Session::exited`] watch，`changed()`
    /// 取码后发 [`PtyEvent::Exit`] 并按 **hub 代次语义**摘除——仍有接管连接
    /// （hub 有代次）时摘除归该连接 forward_loop 的 finish_exit 两维判定
    /// （审查 I-1 的代次闭合原样保留）；无连接才由本观察者摘除。两个观察者
    /// 各管各的代次，互不越界。
    fn spawn_exit_watcher(self: &Arc<Self>, session: &Arc<Session>) {
        let state = Arc::clone(self);
        let session = Arc::clone(session);
        let id = session.id.clone();
        let mut exited = session.exited.clone();
        self.handle.spawn(async move {
            // 发送端只在 exit 线程 send 一次；全灭（kill 路径已同步摘除且
            // exit 线程异常终止）时本观察者无事可做，直接返回。
            if exited.changed().await.is_err() {
                return;
            }
            let code = exited.borrow().unwrap_or(-1);
            if session.outbound.current().is_none() {
                state
                    .sessions
                    .lock()
                    .expect("sessions mutex poisoned")
                    .remove(&id);
            }
            state.emit(PtyEvent::Exit(ExitInfo { id, code }));
        });
    }

    /// 在册会话快照（各查询方法共用）；不在册 → Err。
    fn get(&self, id: &str) -> Result<Arc<Session>, PtyError> {
        self.sessions
            .lock()
            .expect("sessions mutex poisoned")
            .get(id)
            .cloned()
            .ok_or_else(|| unknown_session(id))
    }
}

impl PtyHost {
    /// 起 PtyHost：绑 127.0.0.1:0，把会话路由 server 挂到传入的运行时上。
    pub fn start(handle: tokio::runtime::Handle) -> Result<Arc<Self>, PtyError> {
        let std_listener = std::net::TcpListener::bind("127.0.0.1:0")?;
        let port = std_listener.local_addr()?.port();
        std_listener.set_nonblocking(true)?;
        // WHY enter() 而非 handle.block_on：`TcpListener::from_std` 需要当前
        // 线程处于该运行时的注册上下文；`enter` 在任意线程（含运行时线程自身）
        // 都成立，规避「运行时内不能 block_on」的 panic——Task 10 的 tauri
        // setup 正是在 async_runtime::block_on 内装配。
        let listener = {
            let _guard = handle.enter();
            tokio::net::TcpListener::from_std(std_listener)?
        };
        let token = server::generate_token();
        let endpoint = DataEndpoint {
            port,
            token: token.clone(),
        };
        let sessions: Sessions = Arc::new(Mutex::new(HashMap::new()));
        let handler: ConnectionHandler = {
            let sessions = Arc::clone(&sessions);
            Arc::new(move |id, ws| server::route_connection(&sessions, id, ws))
        };
        let serve = handle.spawn(server::serve(listener, token, handler));
        Ok(Arc::new(PtyHost {
            inner: Arc::new(HostState {
                handle,
                endpoint,
                sup: Arc::new(Supervisor::new()),
                sessions,
                readers: Mutex::new(HashMap::new()),
                event_cb: Mutex::new(None),
                serve: Mutex::new(Some(serve)),
            }),
        }))
    }

    /// 数据面 endpoint（port + token，宿主交前端/agent 连接）。
    pub fn endpoint(&self) -> DataEndpoint {
        self.inner.endpoint.clone()
    }

    /// 设置事件回调（spawn 成功 / 会话退出时分发，见 [`PtyEvent`]）。
    pub fn set_event_callback(&self, cb: EventCallback) {
        *self
            .inner
            .event_cb
            .lock()
            .expect("event callback mutex poisoned") = Some(cb);
    }

    /// spawn 一条会话并入注册表，返回 id；Spawned 事件在成功处发。
    pub fn spawn(&self, mut req: SpawnRequest) -> Result<String, PtyError> {
        // Task 8 遗留归一：`Some("")` 会透传空 program（spawn_command 必败），
        // 归一为未指定、走默认 shell 解析。
        if req.shell_override.as_deref() == Some("") {
            req.shell_override = None;
        }
        // Session::spawn 内部自管线程（reader/writer/exit），此处直调（Task 10
        // 会以 run_blocking 包命令层）。
        let (session, runtime) = Session::spawn(req)?;
        let id = session.id.clone();
        self.inner
            .sessions
            .lock()
            .expect("sessions mutex poisoned")
            .insert(id.clone(), Arc::clone(&session));
        self.inner
            .readers
            .lock()
            .expect("readers mutex poisoned")
            .insert(id.clone(), runtime.reader_handle);
        self.inner.spawn_exit_watcher(&session);
        self.inner.emit(PtyEvent::Spawned { id: id.clone() });
        Ok(id)
    }

    /// 直写 master（阻塞写，等效键入；会话已 kill 时静默丢弃，见
    /// [`Session::write`] 的 sink 语义）。
    pub fn write(&self, id: &str, bytes: Vec<u8>) -> Result<(), PtyError> {
        self.inner.get(id)?.write(&bytes).map_err(PtyError::from)
    }

    /// 非阻塞上行写：经 [`Session::input`] 有界通道入队（写线程串行落
    /// master）。满即 `Ok(false)`——背压显式化，宿主可丢弃或降速；通道已关
    /// （写线程不在）→ Err。
    pub fn write_accepted(&self, id: &str, bytes: Vec<u8>) -> Result<bool, PtyError> {
        use tokio::sync::mpsc::error::TrySendError;
        let session = self.inner.get(id)?;
        match session.input.try_send(bytes) {
            Ok(()) => Ok(true),
            Err(TrySendError::Full(_)) => Ok(false),
            Err(TrySendError::Closed(_)) => Err(PtyError::Io(std::io::Error::other(
                "session input closed (writer thread gone)",
            ))),
        }
    }

    pub fn resize(&self, id: &str, cols: u16, rows: u16) -> Result<(), PtyError> {
        self.inner.get(id)?.resize(cols, rows)
    }

    /// 向子进程发信号。unix：信号名（`SIG` 前缀可选）经 `libc::kill`；windows：
    /// 仅 SIGTERM/SIGKILL 映射 kill，其余 Err（见 [`Session::signal`]）。
    pub fn signal(&self, id: &str, sig: &str) -> Result<(), PtyError> {
        self.inner.get(id)?.signal(sig)
    }

    /// 清空会话的 pre-attach 环形缓冲（首连重放语义保留）。
    pub fn clear_buffer(&self, id: &str) -> Result<(), PtyError> {
        self.inner.get(id)?.clear_buffer();
        Ok(())
    }

    pub fn get_cwd(&self, id: &str) -> Option<String> {
        self.inner.get(id).ok().map(|s| s.cwd.clone())
    }

    pub fn get_size(&self, id: &str) -> Option<(u16, u16)> {
        self.inner
            .get(id)
            .ok()
            .map(|s| *s.size.lock().expect("size mutex poisoned"))
    }

    /// 会话在册即有 pty（kill/退出路径会把它摘出注册表）。
    pub fn has_pty(&self, id: &str) -> bool {
        self.inner
            .sessions
            .lock()
            .expect("sessions mutex poisoned")
            .contains_key(id)
    }

    /// reattach 判定：sessionId 命中在册活会话（与 [`PtyHost::has_pty`] 同源，
    /// 语义名分开保留——bridge 的 isReattach 只看这个）。
    pub fn is_alive(&self, id: &str) -> bool {
        self.has_pty(id)
    }

    /// 在册会话列表（id 排序，输出确定；bridge 补 worktreeId/title）。
    pub fn list_sessions(&self) -> Vec<ListedSession> {
        let sessions = self.inner.sessions.lock().expect("sessions mutex poisoned");
        let mut rows: Vec<ListedSession> = sessions
            .iter()
            .map(|(id, s)| ListedSession {
                id: id.clone(),
                cwd: s.cwd.clone(),
                agent_ownership: "unknown".to_string(),
            })
            .collect();
        rows.sort_by(|a, b| a.id.cmp(&b.id));
        rows
    }

    /// kill 会话并返回退出码。**绝不在调用线程直调 [`Session::kill`]**（Task 6
    /// 审查 Important-1）：closer 断 master 后子进程若无视 HUP，exit 线程的
    /// `wait()` 无界，[`Session::kill`] 会随 child 锁一起挂死调用方（Tauri
    /// 命令线程）。实现：kill 挪专用 std 线程，宿主线程 `recv_timeout(2s)` 等
    /// 退出码；超时即 [`Session::force_terminate`] 升级（unix：pid +
    /// `libc::kill(SIGKILL)`——不碰 child 锁，升级方与被杀线程互不阻塞；
    /// windows：killer 句柄）再等 2s；再超时返回 -1，后台线程照常收尾
    /// （Supervisor harvester 兜底 join）。
    ///
    /// 会话在 kill 起点即从注册表摘除（has_pty/is_alive 立即 false；已挂连接
    /// 由 forward_loop 按退出路径 close(1000) 收尾）。重复 kill/未知 id → Err。
    pub fn kill(&self, id: &str) -> Result<i32, PtyError> {
        let session = self
            .inner
            .sessions
            .lock()
            .expect("sessions mutex poisoned")
            .remove(id)
            .ok_or_else(|| unknown_session(id))?;
        let reader = self
            .inner
            .readers
            .lock()
            .expect("readers mutex poisoned")
            .remove(id)
            // 两表在 spawn 成对插入、kill 在此成对摘除；缺失即不变量被破坏，
            // 按未知会话处理（不得传错句柄给 reap）。
            .ok_or_else(|| unknown_session(id))?;
        let sup = Arc::clone(&self.inner.sup);
        let (tx, rx) = std::sync::mpsc::channel::<i32>();
        let killer = Arc::clone(&session);
        std::thread::Builder::new()
            .name(format!("pty-kill-{id}"))
            .spawn(move || {
                let code = killer.kill(&sup, reader);
                let _ = tx.send(code);
            })
            .map_err(PtyError::from)?;
        use std::sync::mpsc::RecvTimeoutError;
        match rx.recv_timeout(KILL_WAIT) {
            Ok(code) => Ok(code),
            Err(RecvTimeoutError::Timeout) | Err(RecvTimeoutError::Disconnected) => {
                session.force_terminate();
                match rx.recv_timeout(KILL_WAIT) {
                    Ok(code) => Ok(code),
                    // 再超时（或 kill 线程已断开）：返回 -1，回收在后台继续。
                    Err(_) => Ok(-1),
                }
            }
        }
    }

    /// 全量收尾：逐会话 kill（复用升级路径；单个无视信号的子进程至多拖
    /// 2×[`KILL_WAIT`]）+ abort accept 循环（server close）。返回时在册会话
    /// 全部摘除。
    pub fn shutdown_all(&self) {
        let ids: Vec<String> = self
            .inner
            .sessions
            .lock()
            .expect("sessions mutex poisoned")
            .keys()
            .cloned()
            .collect();
        for id in ids {
            let _ = self.kill(&id);
        }
        if let Some(serve) = self
            .inner
            .serve
            .lock()
            .expect("serve mutex poisoned")
            .take()
        {
            serve.abort();
        }
    }
}
