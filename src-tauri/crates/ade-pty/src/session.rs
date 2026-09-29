//! 会话核心：PTY spawn、reader 背压、pre-attach 环形缓冲、退出检测与连接槽。
//!
//! [`Session::spawn`] 一次拉起 pty + shell。输出路径：reader 线程每块先镜像进
//! pre-attach 环形（首连前积累，重放即停用，见 [`PreAttach`]），再经
//! [`OutboundHub`] 投递给当前连接的下行通道（满即停读 + sleep 重试，绝不丢块）；
//! 无连接且 pre-attach 已停用时块弃置（post-attach 不缓冲，规格 §3.2）。
//! 退出检测双通道：oneshot 交 [`SessionRuntime`]（spawn 返回给调用方），
//! [`Session::exited`] watch 广播给路由层——Task 9 的 PtyEvent::Exit 挂在
//! `changed()` 上。kill（[`Session::kill`]）走 supervisor 的 reap 路径。
//! WS 连接的接管/替换/收尾在 [`crate::server::route_connection`]；非 WS 装配
//! （测试）用 [`Session::connect_channel`]。

use std::collections::VecDeque;
use std::io::Write;
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use tokio::sync::mpsc::error::TrySendError;
use tokio_tungstenite::tungstenite::Bytes;

#[cfg(windows)]
use crate::cpr;
use crate::supervisor::Supervisor;
use crate::PtyError;

/// reader → server 的输出块；内部 `Bytes`（引用计数）使 pre-attach 重放与 WS
/// 帧构造（`Message::Binary` 同为 `Bytes`）之间零拷贝。
#[derive(Clone)]
pub struct Chunk(Bytes);

impl Chunk {
    pub fn new(data: Vec<u8>) -> Self {
        // Bytes::from(Vec) 直接接管堆内存，无拷贝。
        Self(Bytes::from(data))
    }

    pub fn as_slice(&self) -> &[u8] {
        &self.0
    }

    /// 内部 `Bytes` 的克隆（引用计数 +1，零拷贝）；WS 帧构造用。
    pub(crate) fn bytes(&self) -> Bytes {
        self.0.clone()
    }
}

impl std::ops::Deref for Chunk {
    type Target = [u8];

    fn deref(&self) -> &[u8] {
        &self.0
    }
}

/// outbound 通道容量（块数；每块至多 [`READ_BUF_BYTES`]）。
pub(crate) const OUTBOUND_CAPACITY: usize = 128;
/// pre-attach 环形缓冲字节上限，超限弹最旧。
pub(crate) const PRE_ATTACH_CAP_BYTES: usize = 256 * 1024;
/// reader 单次 read 的块大小。
const READ_BUF_BYTES: usize = 64 * 1024;
/// outbound 满时的重试间隔：满即停读 + sleep 重试，绝不丢块。
const TRY_SEND_RETRY: Duration = Duration::from_millis(5);

/// pre-attach 环形缓冲的共享状态（reader 与接管方经同一把互斥锁访问）。
///
/// Task 5 → 7 交接裁定落实：字节水位从 reader 线程本地记账移入本结构
/// （`total` 字段），`disabled` 停用标志同锁维护——接管方排空即置位，reader
/// 同锁可见，不会出现「本地计数虚高导致过度弹出」或「排空后仍写入」的竞态。
pub struct PreAttach {
    buf: VecDeque<Chunk>,
    total: usize,
    disabled: bool,
}

impl PreAttach {
    pub(crate) fn new() -> Self {
        Self {
            buf: VecDeque::new(),
            total: 0,
            disabled: false,
        }
    }

    /// 未停用时镜像一块（超 [`PRE_ATTACH_CAP_BYTES`] 弹最旧）；返回「是否需要
    /// 下游投递」——停用后为 true（块未被镜像，只能走通道）。reader 在同一次
    /// 加锁内取得该判定，保证「重放 ∪ 通道」不重不漏：push 见到未停用的块只
    /// 会出现在重放集里，见到停用后的块只会出现在通道里。
    pub(crate) fn push(&mut self, chunk: &Chunk) -> bool {
        if self.disabled {
            return true;
        }
        self.buf.push_back(chunk.clone());
        self.total += chunk.len();
        while self.total > PRE_ATTACH_CAP_BYTES {
            match self.buf.pop_front() {
                Some(front) => self.total -= front.len(),
                None => {
                    self.total = 0;
                    break;
                }
            }
        }
        false
    }

    /// 排空并停用（首连重放取存量；返回后 reader 不再写入）。
    ///
    /// 调用次序契约：接管方必须**先**换入新下行发送端（[`OutboundHub::replace`]）
    /// **再**调用本方法——本方法与 [`PreAttach::push`] 同锁串行，换入后停用前
    /// push 的块落在本方法返回的存量里，停用后 push 的块走新通道，不重不漏。
    pub(crate) fn take_replay(&mut self) -> Vec<Chunk> {
        self.disabled = true;
        self.buf.drain(..).collect()
    }

    /// 当前块数（[`Session::connect_channel`] 预估通道容量用，未停用）。
    pub(crate) fn len(&self) -> usize {
        self.buf.len()
    }
}

/// reader → 当前连接下行通道的出口。接管方经 [`OutboundHub::replace`] 换入新
/// 通道的发送端并取得**代次**；转发任务收尾时 [`OutboundHub::clear_if`] 只清
/// 自己的代次——旧任务不会误清新连接的发送端，reader 也不会向已死通道空转。
///
/// 内部 std 锁：reader 是 std 线程（`try_send` 同步重试），持锁只做句柄克隆，
/// 不跨 await。
#[derive(Clone)]
pub struct OutboundHub(Arc<Mutex<HubState>>);

/// hub 内部状态：`(代次, 当前下行发送端)`；`None` = 无连接挂接。
type HubState = Option<(u64, tokio::sync::mpsc::Sender<Chunk>)>;

impl OutboundHub {
    pub(crate) fn new() -> Self {
        Self(Arc::new(Mutex::new(None)))
    }

    /// 换入新下行发送端，返回新代次（自 1 递增）。
    pub(crate) fn replace(&self, tx: tokio::sync::mpsc::Sender<Chunk>) -> u64 {
        let mut guard = self.0.lock().expect("outbound hub poisoned");
        let gen = guard.as_ref().map_or(1, |(gen, _)| gen + 1);
        *guard = Some((gen, tx));
        gen
    }

    /// 当前发送端快照（代次 + 句柄克隆）；`None` = 无连接挂接。
    pub(crate) fn current(&self) -> Option<(u64, tokio::sync::mpsc::Sender<Chunk>)> {
        self.0.lock().expect("outbound hub poisoned").clone()
    }

    /// 仅清除指定代次（转发任务收尾用；代次不符说明已有新连接接管，勿动）。
    pub(crate) fn clear_if(&self, gen: u64) {
        let mut guard = self.0.lock().expect("outbound hub poisoned");
        if guard.as_ref().is_some_and(|(g, _)| *g == gen) {
            *guard = None;
        }
    }

    /// 无条件清除（仅 reader EOF 使用：生产者已死，任何代次都应断开，避免
    /// 「退出后才接管」的连接 recv 悬挂）。
    pub(crate) fn clear(&self) {
        *self.0.lock().expect("outbound hub poisoned") = None;
    }
}

/// 活动连接槽（[`Session::connection`] 的载荷）：一个会话至多一条活动 WS 接管
/// 任务。spawn 预置 `None`——下行通道与槽都由接管方
/// （[`crate::server::route_connection`]）建立；「Close/Err → 连接槽清空」即
/// 接管任务收尾时的 `take()`（仅当代次仍匹配）。
///
/// 与 brief 接口草图的偏差（记录在案）：草图的 `{ outbound_rx, pre_attach }`
/// 由接管任务**本地持有**——rx 被 `recv()` 循环独占移动使用、pre-attach 排空后
/// 不再需要，两者放进共享槽会迫使锁跨 `.await` 持有，替换方与收尾方会互相死锁。
/// 槽内只留替换协议所需的最小状态：代次（收尾方的「还是我吗」判据）与让位闸
/// （新接管方 `send(())`，旧任务收到后自行 `ws.close(1000)` 并退出）。
pub struct ConnectionSlot {
    pub(crate) gen: u64,
    pub(crate) replace: tokio::sync::oneshot::Sender<()>,
}

/// spawn 请求（Task 10 由 bridge 从 JSON 反序列化）。
pub struct SpawnRequest {
    pub cols: u16,
    pub rows: u16,
    pub cwd: Option<String>,
    pub env: std::collections::HashMap<String, String>,
    pub env_to_delete: Vec<String>,
    pub command: Option<String>,
    pub shell_override: Option<String>,
}

/// 一条 PTY 会话。生命周期：spawn → write/resize/消费 outbound → 子进程退出。
///
/// 可见性说明：brief 写的是 `pub(crate)`，但 Task 5 的集成测试 `tests/session.rs`
/// （`cargo test --test session`）作为外部 crate 只能看到 `pub` 项——`spawn`、
/// `write`、`connection`、`SessionRuntime` 均在测试签名路径上，故整体 `pub`；
/// `writer`/`child`/`master` 仍保持私有，形状与 brief 一致。
pub struct Session {
    pub id: String,
    // writer 独立 Mutex（输入与 kill 竞争小）；child 用于 kill/wait
    writer: Mutex<Box<dyn Write + Send>>,
    child: Mutex<Box<dyn portable_pty::Child + Send + Sync>>,
    /// master 句柄：resize 需要（brief：master 需存进 Session）。kill 时
    /// take 出来 drop（断管道）；None = 会话已 kill（终态）。
    master: Mutex<Option<Box<dyn portable_pty::MasterPty + Send>>>,
    pub cwd: String,
    pub size: Mutex<(u16, u16)>,
    /// reader → 当前连接下行通道的出口（接管方换入通道，见 [`OutboundHub`]）。
    pub outbound: OutboundHub,
    /// 256 KiB 环形（含水位与停用位，见 [`PreAttach`]）；首连排空即停用。
    pub pre_attach: Arc<Mutex<PreAttach>>,
    /// 活动连接槽：spawn 预置 `None`，WS 接管方建立并安装（见 [`ConnectionSlot`]）。
    pub connection: Arc<tokio::sync::Mutex<Option<ConnectionSlot>>>,
    /// 退出广播：exit 线程置 `Some(code)`。路由层/Task 9 用 `changed()` 挂接
    /// 退出收尾与 PtyEvent::Exit（oneshot 已被 [`SessionRuntime`] 占用）。
    pub exited: tokio::sync::watch::Receiver<Option<i32>>,
}

/// 会话运行期句柄：`exit` 在子进程退出后给出退出码（unix 语义；信号死亡时
/// portable-pty 的 `ExitStatus::exit_code()` 已折算，见 [`Session::spawn`] 注释）；
/// `reader_handle` 经 [`Session::kill`] 传入 [`Supervisor::reap`]（closer→join）回收。
/// 同一退出也广播在 [`Session::exited`]（watch），供未持有 runtime 的一方监视。
pub struct SessionRuntime {
    pub exit: tokio::sync::oneshot::Receiver<i32>,
    pub reader_handle: std::thread::JoinHandle<()>,
}

impl Session {
    /// spawn 一条会话：openpty → 起 shell（unix 加 `-l`）→ reader / exit 线程。
    pub fn spawn(req: SpawnRequest) -> Result<(Arc<Self>, SessionRuntime), PtyError> {
        let id = uuid::Uuid::new_v4().to_string();

        // shell 解析（临时实现）：override → $SHELL → /bin/sh。Task 8 定型
        // shell.rs（含带参解析、cwdFallback 等）后整体替换，保持测试兼容。
        let shell = req
            .shell_override
            .clone()
            .or_else(|| std::env::var("SHELL").ok())
            .unwrap_or_else(|| "/bin/sh".to_string());

        // env 组装（规格 §4.6 顺序）：进程 env → envToDelete 删除 → env 覆盖 →
        // unix 追加 TERM/COLORTERM（已存在不覆盖）。
        // 注：portable-pty 0.9 没有 `envs(map)` 批量入口；`CommandBuilder::new`
        // 经 get_base_env()（= std::env::vars_os）自行快照进程 env，即「收集」
        // 步骤，删除/覆盖/追加相应落在 env_remove / env / get_env 判空上，
        // `as_command` 最终 env_clear 后只应用该快照，删除语义真实生效。
        let mut cmd = CommandBuilder::new(&shell);
        #[cfg(unix)]
        cmd.arg("-l"); // login shell
        for key in &req.env_to_delete {
            cmd.env_remove(key);
        }
        for (key, value) in &req.env {
            cmd.env(key, value);
        }
        #[cfg(unix)]
        {
            if cmd.get_env("TERM").is_none() {
                cmd.env("TERM", "xterm-256color");
            }
            if cmd.get_env("COLORTERM").is_none() {
                cmd.env("COLORTERM", "truecolor");
            }
        }

        let cwd = req
            .cwd
            .clone()
            .or_else(|| {
                std::env::current_dir()
                    .ok()
                    .map(|p| p.to_string_lossy().into_owned())
            })
            .unwrap_or_default();
        cmd.cwd(&cwd);

        let pty = native_pty_system();
        let pair = pty
            .openpty(PtySize {
                rows: req.rows,
                cols: req.cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(PtyError::from)?;
        let child = pair.slave.spawn_command(cmd).map_err(PtyError::from)?;
        // 子进程已持有 slave 端；宿主侧立即丢弃（portable-pty API 惯例）。
        drop(pair.slave);
        let writer = pair.master.take_writer().map_err(PtyError::from)?;
        let reader = pair.master.try_clone_reader().map_err(PtyError::from)?;

        // 下行出口（空 hub：通道由 WS 接管方建立）与 pre-attach 环形。
        let outbound = OutboundHub::new();
        let pre_attach = Arc::new(Mutex::new(PreAttach::new()));
        let connection: Arc<tokio::sync::Mutex<Option<ConnectionSlot>>> =
            Arc::new(tokio::sync::Mutex::new(None));

        // 退出广播（exit 线程置 Some(code)；oneshot 走 SessionRuntime）。
        let (exit_bcast_tx, exit_bcast_rx) = tokio::sync::watch::channel::<Option<i32>>(None);

        let session = Arc::new(Session {
            id: id.clone(),
            writer: Mutex::new(writer),
            child: Mutex::new(child),
            master: Mutex::new(Some(pair.master)),
            cwd,
            size: Mutex::new((req.cols, req.rows)),
            outbound,
            pre_attach: Arc::clone(&pre_attach),
            connection,
            exited: exit_bcast_rx,
        });

        // 命令交付：spawn 后把 command 作为一行输入写入（等效用户键入）。
        if let Some(command) = &req.command {
            session.write(format!("{command}\n").as_bytes())?;
        }

        // unix：reader 刻意不持有 Arc<Session>——Session 的存活不应被 reader 钉住，
        // drop Session（关 master）必须能让 reader 自然 EOF 退出。
        #[cfg(unix)]
        let reader_handle =
            spawn_reader_loop(reader, session.outbound.clone(), Arc::clone(&pre_attach));
        #[cfg(windows)]
        let reader_handle = spawn_reader_loop(
            reader,
            session.outbound.clone(),
            Arc::clone(&pre_attach),
            Arc::clone(&session),
        );

        // exit 检测：独立线程阻塞 wait，退出码经 oneshot 交付 SessionRuntime，
        // 同时广播到 watch（路由层/Task 9 监视用；接收端已弃置则发送失败被忽略）。
        // unix 退出码以 portable-pty ExitStatus::exit_code()
        // 为准——0.9 的 From<std::process::ExitStatus> 已解码：普通退出为 0..=255，
        // 信号死亡记入 signal 字段（名字字符串）且 code 取 0/1，无法还原 -signal
        // 数值，Task 6/9 如需 -signal 语义可改走 process_id + libc::waitpid。
        let (exit_tx, exit_rx) = tokio::sync::oneshot::channel::<i32>();
        let exit_session = Arc::clone(&session);
        std::thread::Builder::new()
            .name(format!("pty-exit-{id}"))
            .spawn(move || {
                // 阻塞期间持有 child 锁：kill（Session::kill）走「先断 master
                // （closer）→ 子进程退出 → wait 返回释放锁」，与之天然串行。
                let code = exit_session
                    .child
                    .lock()
                    .expect("child mutex poisoned")
                    .wait()
                    .map(|st| st.exit_code() as i32)
                    .unwrap_or(-1);
                let _ = exit_bcast_tx.send(Some(code));
                let _ = exit_tx.send(code);
            })
            .map_err(std::io::Error::other)?;

        Ok((
            session,
            SessionRuntime {
                exit: exit_rx,
                reader_handle,
            },
        ))
    }

    /// 写入输入（等效键入）：阻塞 write_all + flush。
    pub fn write(&self, bytes: &[u8]) -> std::io::Result<()> {
        let mut writer = self.writer.lock().expect("writer mutex poisoned");
        writer.write_all(bytes)?;
        writer.flush()
    }

    /// 非 WS 的下行接管口（宿主/测试装配；生产路径走
    /// [`crate::server::route_connection`]，两者接管次序一致）：建通道 → 换入
    /// hub → 排空并停用 pre-attach、把存量排进通道头部 → 返回接收端。重放块
    /// 与实时块因此不重不漏且有序。
    ///
    /// 注：不动连接槽（无 WS 可让位）；重放一次性入队，通道容量按存量预估放大，
    /// 偶发超限时 sleep 重试（消费端聚合循环会持续 recv 腾位）。
    #[doc(hidden)]
    pub fn connect_channel(&self) -> tokio::sync::mpsc::Receiver<Chunk> {
        // ① 探存量定容量（此刻尚未停用）。
        let cap_hint = self
            .pre_attach
            .lock()
            .expect("pre_attach mutex poisoned")
            .len();
        let (tx, rx) = tokio::sync::mpsc::channel(OUTBOUND_CAPACITY.max(cap_hint));
        // ② 先换 hub：此后 reader 的新块只投递到本通道。
        self.outbound.replace(tx.clone());
        // ③ 再排空停用（与 reader 的 push 同锁串行）：停用前 push 的块全在
        //    返回的存量里，停用后的块走通道——不重不漏。
        let replay = self
            .pre_attach
            .lock()
            .expect("pre_attach mutex poisoned")
            .take_replay();
        for chunk in replay {
            while let Err(TrySendError::Full(_)) = tx.try_send(chunk.clone()) {
                std::thread::sleep(TRY_SEND_RETRY);
            }
        }
        rx
    }

    /// 调整 pty 大小并更新 size。会话已 kill（master 已摘除）时报错。
    pub fn resize(&self, cols: u16, rows: u16) -> Result<(), PtyError> {
        let mut master = self.master.lock().expect("master mutex poisoned");
        let master = master
            .as_mut()
            .ok_or_else(|| PtyError::Io(std::io::Error::other("session killed")))?;
        master
            .resize(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(PtyError::from)?;
        *self.size.lock().expect("size mutex poisoned") = (cols, rows);
        Ok(())
    }

    /// kill 会话：closer 先断 master（writer 换入 sink——旧 writer 的 Drop 发
    /// EOT——再把 master take 出来 drop，关闭 master fd），随后 child.kill 兜底、
    /// 重读 waitpid 状态取退出码返回。
    ///
    /// 顺序契约（Task 5 审查交接）：closer 必须先于 child.kill——master 关闭
    /// 使子进程 HUP/EIO 退出，exit 线程的 `wait()` 返回并**释放 child 锁**后，
    /// 下面的 `child.lock()` 才能取得（exit 线程整个 wait 期间持锁）。因此本
    /// 方法在子进程收尾期间可能短暂阻塞，语义即「kill 等到退出码」。
    ///
    /// reader 句柄交 [`Supervisor::reap`]：closer 已断管道，harvester 兜底
    /// join（本方法不等待其完成）。若 reader 因 outbound 接收端存活且通道
    /// 打满而滞留在背压重试里，join 会晚些完成（最终回收语义，不丢失）。
    ///
    /// 终态语义：之后 `write` 静默丢弃（sink）、`resize` 报错；「从注册表摘除」
    /// 由上层（Task 9 PtyHost）负责。可重复调用（幂等）。
    ///
    /// 与 brief 接口草图的偏差：签名多 `reader_handle` 参数——句柄在
    /// [`SessionRuntime`]（其 pub 形状冻结），Session 无法自取，由调用方传入。
    pub fn kill(&self, sup: &Supervisor, reader_handle: JoinHandle<()>) -> i32 {
        let closer = || {
            *self.writer.lock().expect("writer mutex poisoned") = Box::new(std::io::sink());
            self.master.lock().expect("master mutex poisoned").take();
        };
        sup.reap(closer, reader_handle);
        let mut child = self.child.lock().expect("child mutex poisoned");
        // 子进程此刻已死或正在死：kill 兜底（已回收时 portable-pty 内部
        // try_wait 命中缓存直接返回，或 ESRCH 报错——一并忽略）。
        let _ = child.kill();
        // std 缓存首次 wait 的状态：此处重读与 exit 通道同源、同值。
        child.wait().map(|st| st.exit_code() as i32).unwrap_or(-1)
    }
}

/// reader 线程主体：64 KiB 块 `read` →（Windows 经 Session.writer 应答 CPR，
/// unix 旁路）→ 写 pre-attach 环形（256 KiB 上限，同锁取得投递判定）→ 按判定
/// `try_send` 当前下行通道（经 [`OutboundHub`]），满则 sleep(5ms) 重试，绝不丢块；
/// EOF/错误即退出并在退出前断开下行出口（通道排空后转发任务 recv 到 None）。
///
/// unix 路径不持有 `Arc<Session>`；Windows 的 CPR 应答必须写 master writer，而
/// 0.9 的 `take_writer` 只能成功调用一次（unix 实现显式拒绝第二次）且句柄已在
/// `Session.writer` 中，无法克隆或二次取用，故 Windows 传 `Arc<Session>` 借
/// [`Session::write`] 的 writer 锁串行回写（见 [`CprReplyWriter`]）。
fn spawn_reader_loop(
    mut reader: Box<dyn std::io::Read + Send>,
    outbound: OutboundHub,
    pre_attach: Arc<Mutex<PreAttach>>,
    #[cfg(windows)] session: Arc<Session>,
) -> std::thread::JoinHandle<()> {
    std::thread::Builder::new()
        .name("pty-reader".to_string())
        .spawn(move || {
            let mut buf = vec![0u8; READ_BUF_BYTES];
            // CPR 跨块扫描尾部状态（仅 Windows 需要；unix 无 ConPTY 静默问题）。
            #[cfg(windows)]
            let mut cpr_tail: Vec<u8> = Vec::new();
            loop {
                // unix 上 EIO（slave 已关）已被 portable-pty 的 Read 实现映射为
                // Ok(0)，EOF/错误统一退出。
                match reader.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        #[cfg(windows)]
                        {
                            // ConPTY 对未应答的 CPR 查询会让管道静默（见 cpr 模块
                            // 文档）；写失败视为会话致命错误，退出 reader。
                            let mut replier = CprReplyWriter { session: &session };
                            if cpr::scan_and_reply(&mut cpr_tail, &buf[..n], &mut replier).is_err()
                            {
                                break;
                            }
                        }
                        let chunk = Chunk::new(buf[..n].to_vec());
                        // pre-attach 镜像 + 投递判定（同一把锁内原子完成）：
                        // 返回 true = pre 已停用、块未被镜像，必须走通道；
                        // 返回 false = 块已入环形，由首连重放送达。
                        let deliver = {
                            let mut pre = pre_attach.lock().expect("pre_attach mutex poisoned");
                            pre.push(&chunk)
                        };
                        if deliver {
                            // 背压核心：满即停读，绝不丢块。旧接收端随旧转发任务
                            // 消失（Closed）时休眠后重读 hub——接管方即将换入新代
                            // （或收尾方清空 hub）。
                            while let Some((_gen, tx)) = outbound.current() {
                                match tx.try_send(chunk.clone()) {
                                    Ok(()) => break,
                                    Err(TrySendError::Full(_)) => {
                                        std::thread::sleep(TRY_SEND_RETRY)
                                    }
                                    Err(TrySendError::Closed(_)) => {
                                        std::thread::sleep(TRY_SEND_RETRY)
                                    }
                                }
                            }
                            // 无连接：首连前块已在 pre-attach（重放送达）；
                            // 首连后 post-attach 不缓冲（规格 §3.2），弃置。
                        }
                    }
                }
            }
            // EOF（子进程退出 / master 关闭）：断开下行出口——当前通道排空后，
            // 转发任务 recv 到 None，按「排空 → close(1000) → 摘除」收尾（§3.2）。
            outbound.clear();
        })
        .expect("spawn pty reader thread")
}

/// Windows 专用 CPR 应答通道：把应答字节经 [`Session::write`] 写回 master。
/// unix 旁路此路径（ConPTY 特有的未应答 CPR 静默问题不存在），本 crate 测试
/// 亦不覆盖 Windows 分支。
#[cfg(windows)]
struct CprReplyWriter<'a> {
    session: &'a Session,
}

#[cfg(windows)]
impl std::io::Write for CprReplyWriter<'_> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.session.write(buf)?;
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        // Session::write 内部已 flush。
        Ok(())
    }
}
