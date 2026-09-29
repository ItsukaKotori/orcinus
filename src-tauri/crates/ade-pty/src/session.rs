//! 会话核心：PTY spawn、reader 背压、pre-attach 环形缓冲与退出检测。
//!
//! [`Session::spawn`] 一次拉起 pty + shell：reader 线程把输出经有界通道送往
//! server（通道满即停读 + sleep 重试，绝不丢块），同时镜像进 pre-attach 环形
//! 缓冲供首连重放（Task 7 首连后置空并停用）；exit 线程阻塞等待子进程并把退出
//! 码交给 oneshot。kill（[`Session::kill`]）走 supervisor 的 reap 路径：closer
//! 先断 master，reader 句柄交 harvester 兜底 join；ConnectionSlot 由 Task 7 填充。

use std::collections::VecDeque;
use std::io::Write;
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use tokio::sync::mpsc::error::TrySendError;

#[cfg(windows)]
use crate::cpr;
use crate::supervisor::Supervisor;
use crate::PtyError;

/// reader → server 的输出块；内部 `Arc<Vec<u8>>` 使 pre-attach 重放与 WS 广播
/// 之间克隆零拷贝。
#[derive(Clone)]
pub struct Chunk(Arc<Vec<u8>>);

impl Chunk {
    pub fn new(data: Vec<u8>) -> Self {
        Self(Arc::new(data))
    }

    pub fn as_slice(&self) -> &[u8] {
        &self.0
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

/// 连接槽占位。Task 5 暂以 outbound 接收端充当槽内容（spawn 时预置 `Some(rx)`，
/// 集成测试由此取接收端聚合输出）；Task 7 换成
/// `struct ConnectionSlot { outbound_rx: Receiver<Chunk>, pre_attach: …, disabled: … }`。
pub type ConnectionSlot = tokio::sync::mpsc::Receiver<Chunk>;

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
    /// reader → server；容量 [`OUTBOUND_CAPACITY`]。
    pub outbound: tokio::sync::mpsc::Sender<Chunk>,
    /// 256 KiB 环形；首连后由 Task 7 置空并停用。
    pub pre_attach: Arc<Mutex<VecDeque<Chunk>>>,
    /// Task 7 填充；本任务预置 outbound 接收端占位。
    pub connection: Arc<tokio::sync::Mutex<Option<ConnectionSlot>>>,
}

/// 会话运行期句柄：`exit` 在子进程退出后给出退出码（unix 语义；信号死亡时
/// portable-pty 的 `ExitStatus::exit_code()` 已折算，见 [`Session::spawn`] 注释）；
/// `reader_handle` 经 [`Session::kill`] 传入 [`Supervisor::reap`]（closer→join）回收。
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
                std::env::current_dir().ok().map(|p| p.to_string_lossy().into_owned())
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

        let (outbound_tx, outbound_rx) = tokio::sync::mpsc::channel::<Chunk>(OUTBOUND_CAPACITY);
        let pre_attach: Arc<Mutex<VecDeque<Chunk>>> = Arc::new(Mutex::new(VecDeque::new()));
        let connection: Arc<tokio::sync::Mutex<Option<ConnectionSlot>>> =
            Arc::new(tokio::sync::Mutex::new(Some(outbound_rx)));

        let session = Arc::new(Session {
            id: id.clone(),
            writer: Mutex::new(writer),
            child: Mutex::new(child),
            master: Mutex::new(Some(pair.master)),
            cwd,
            size: Mutex::new((req.cols, req.rows)),
            outbound: outbound_tx,
            pre_attach: Arc::clone(&pre_attach),
            connection,
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

        // exit 检测：独立线程阻塞 wait，退出码经 oneshot 交付（接收端已弃置则
        // 发送失败被忽略）。unix 退出码以 portable-pty ExitStatus::exit_code()
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
/// unix 旁路）→ 写 pre-attach 环形（256 KiB 上限）→ `try_send` outbound，
/// 满则 sleep(5ms) 重试，绝不丢块；EOF/错误即退出。
///
/// unix 路径不持有 `Arc<Session>`；Windows 的 CPR 应答必须写 master writer，而
/// 0.9 的 `take_writer` 只能成功调用一次（unix 实现显式拒绝第二次）且句柄已在
/// `Session.writer` 中，无法克隆或二次取用，故 Windows 传 `Arc<Session>` 借
/// [`Session::write`] 的 writer 锁串行回写（见 [`CprReplyWriter`]）。
fn spawn_reader_loop(
    mut reader: Box<dyn std::io::Read + Send>,
    outbound: tokio::sync::mpsc::Sender<Chunk>,
    pre_attach: Arc<Mutex<VecDeque<Chunk>>>,
    #[cfg(windows)] session: Arc<Session>,
) -> std::thread::JoinHandle<()> {
    std::thread::Builder::new()
        .name("pty-reader".to_string())
        .spawn(move || {
            let mut buf = vec![0u8; READ_BUF_BYTES];
            // CPR 跨块扫描尾部状态（仅 Windows 需要；unix 无 ConPTY 静默问题）。
            #[cfg(windows)]
            let mut cpr_tail: Vec<u8> = Vec::new();
            let mut pre_total = 0usize;
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
                            if cpr::scan_and_reply(&mut cpr_tail, &buf[..n], &mut replier)
                                .is_err()
                            {
                                break;
                            }
                        }
                        let chunk = Chunk::new(buf[..n].to_vec());
                        // pre-attach 环形：超限弹最旧。Task 7 首连排空后置停用标志。
                        {
                            let mut pre =
                                pre_attach.lock().expect("pre_attach mutex poisoned");
                            pre.push_back(chunk.clone());
                            pre_total += chunk.len();
                            while pre_total > PRE_ATTACH_CAP_BYTES {
                                match pre.pop_front() {
                                    Some(front) => pre_total -= front.len(),
                                    None => {
                                        pre_total = 0;
                                        break;
                                    }
                                }
                            }
                        }
                        // 背压核心：满即停读，绝不丢块。
                        loop {
                            match outbound.try_send(chunk.clone()) {
                                Ok(()) => break,
                                Err(TrySendError::Full(_)) => {
                                    std::thread::sleep(TRY_SEND_RETRY)
                                }
                                // 接收端消失（会话已被上层摘除）：停止产出。
                                Err(TrySendError::Closed(_)) => return,
                            }
                        }
                    }
                }
            }
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
