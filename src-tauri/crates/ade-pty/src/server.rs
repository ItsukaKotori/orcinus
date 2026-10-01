//! 进程内数据面订阅：接管/替换/pre-attach 排空/退出收尾（规格 §3.2 修订二，
//! §10 偏差 4）。
//!
//! 原 WS accept/鉴权/帧搬运层已拆除（`serve`/`generate_token`/`DataEndpoint`/
//! `constant_time_eq` 均随偏差 4 移除）；接管次序与 **两维代次判定** 自 WS 版
//! `forward_loop` 原样平移，只换最后一跳：「写 WS 帧」→「调用方 sink 回调」
//! （生产为 Tauri `ipc::Channel`，测试为收集器）。上行不经本模块——键盘输入走
//! `pty_write`/`pty_write_accepted` 命令。
//!
//! 消费形态：[`attach`] 建 [`SessionStream`]（未知名 → Err，对应原 close(1008)
//! 的 404 语义）→ [`forward_stream`] 消费之（backlog 先行 → outbound 逐块 →
//! 退出排空 → 收尾）。

use std::collections::HashMap;
use std::future::Future;
use std::sync::Arc;

use crate::session::{Chunk, ConnectionSlot, Session, OUTBOUND_CAPACITY};
use crate::PtyError;

/// 会话注册表：id → 会话（宿主与 [`attach`] 共用同一张表）。
pub type SessionsMap = HashMap<String, Arc<Session>>;
/// 共享注册表句柄。
pub type Sessions = Arc<std::sync::Mutex<SessionsMap>>;

/// 一次订阅的下行产物。语义：
/// - `backlog`：pre-attach 排空存量（首订阅重放；替换订阅得空集）；
/// - `outbound`：实时块通道（sender 在 hub 中，被替换/退出/断开时关闭）；
/// - `exited`：退出广播（`Some(code)`，见 [`Session::exited`]）。
///
/// 调用契约：`backlog` 必须先于 `outbound` 消费（经 [`forward_stream`] 则天然
/// 满足）；退出时 outbound 排空后关闭（「重放 ∪ 通道」不重不漏且有序）。
/// 私有字段为收尾所需的接管上下文（注册表、会话、代次、让位信号）。
pub struct SessionStream {
    pub backlog: Vec<Chunk>,
    pub outbound: tokio::sync::mpsc::Receiver<Chunk>,
    pub exited: tokio::sync::watch::Receiver<Option<i32>>,
    pub(crate) sessions: Sessions,
    pub(crate) session: Arc<Session>,
    pub(crate) session_id: String,
    pub(crate) gen: u64,
    pub(crate) replace_rx: tokio::sync::oneshot::Receiver<()>,
}

/// 建立（或替换）一次订阅。接管次序即正确性（与 WS 版 `attach_session` 一致，
/// 两处 Err 对应原 close(1008)/close(1000) 的失败收场）：
///
/// ① 建新下行通道并换入 hub（取得代次）——此后 reader 的新块只投递到新通道；
/// ② 确认会话未被并发退出路径摘除（接管已退会话 → Err）；
/// ③ 排空并停用 pre-attach（首订阅重放存量；替换时已停用、得空集）。与 reader
///    的 push 同锁串行：换入后停用前 push 的块全在重放集里，停用后的块走新
///    通道——「重放 ∪ 通道」不重不漏且有序；
/// ④ 安装新连接槽并让旧订阅让位：旧 forward 任务的 replace 分支随即终止
///    （webview reload 重挂路径，规格 §3.2），其通道接收端随流丢弃而关闭。
pub async fn attach(sessions: &Sessions, session_id: &str) -> Result<SessionStream, PtyError> {
    let session = sessions
        .lock()
        .expect("sessions mutex poisoned")
        .get(session_id)
        .cloned()
        .ok_or_else(|| unknown_session(session_id))?;

    let (outbound_tx, outbound_rx) = tokio::sync::mpsc::channel(OUTBOUND_CAPACITY);
    let gen = session.outbound.replace(outbound_tx);
    if !sessions
        .lock()
        .expect("sessions mutex poisoned")
        .contains_key(session_id)
    {
        session.outbound.clear_if(gen);
        return Err(unknown_session(session_id));
    }
    let replay = session
        .pre_attach
        .lock()
        .expect("pre_attach mutex poisoned")
        .take_replay();

    let (replace_tx, replace_rx) = tokio::sync::oneshot::channel::<()>();
    let old = session.connection.lock().await.replace(ConnectionSlot {
        gen,
        replace: replace_tx,
    });
    if let Some(old) = old {
        let _ = old.replace.send(());
    }

    // ④ 装槽后复查注册表（收口 ② 与 ④ 之间并发退出摘除的残余窗口，审查 I-1）：
    // 旧任务的 finish_exit 若在 ② 之后才摘除（其 hub 维判定时尚无新代次在途），
    // 本订阅不得继续以该会话名义转发。让位收场：清本代槽/hub（gen 判定防误删
    // 后续接管方）、报错。已让位的旧订阅自身会按两维判定不摘，安全收敛。
    if !sessions
        .lock()
        .expect("sessions mutex poisoned")
        .contains_key(session_id)
    {
        teardown(&session, gen).await;
        return Err(unknown_session(session_id));
    }

    Ok(SessionStream {
        backlog: replay,
        outbound: outbound_rx,
        exited: session.exited.clone(),
        sessions: Arc::clone(sessions),
        session,
        session_id: session_id.to_string(),
        gen,
        replace_rx,
    })
}

/// 转发终点抽象：把一块字节交给调用方的投递通道（生产为 Tauri
/// `ipc::Channel` 的 Raw 帧；测试为收集器）。`Err` = 终点已消失（webview
/// reload / 关闭，对应 WS 客户端断开）——转发按「客户端断开」收尾：清本订阅
/// 的槽与 hub（代次判据防误清新接管方），但**不摘会话**（同 id 可再订阅）。
pub trait ByteSink: Send {
    type Error;
    fn send(&mut self, bytes: &[u8]) -> impl Future<Output = Result<(), Self::Error>> + Send;
}

/// 消费一条 [`SessionStream`]（一个任务同时服务 backlog/下行/让位/退出四路）：
/// - backlog：存量先行，逐块交 sink；失败即按客户端断开收尾；
/// - 下行：循环通道 `recv()` → sink；
/// - 让位：新订阅发信号 → 终止（通道接收端随流 drop 而关闭）；
/// - 退出（规格 §3.2 顺序）：排空 outbound → **两维判定**摘除 → 流终止
///   （`pty:exit` 事件由 [`crate::PtyEvent::Exit`] 观察者另行发出）。退出检测
///   源有二，殊途同归到 [`finish_exit`]：reader EOF 断开 hub 后通道排空
///   `recv()` 得 `None`；[`Session::exited`] watch（子进程死）在 reader 尚未
///   排完时先到，此时断开 hub 再继续 recv 把尾量送完。
pub async fn forward_stream<S: ByteSink>(stream: SessionStream, mut sink: S) {
    let SessionStream {
        sessions,
        session,
        session_id,
        gen,
        mut replace_rx,
        mut outbound,
        exited: mut exited_rx,
        backlog,
    } = stream;

    // 首订阅重放：存量先于通道实时块（接管次序保证）。
    for chunk in &backlog {
        if sink.send(chunk.as_slice()).await.is_err() {
            // 终点在重放期间消失：按「客户端断开」收尾（不摘会话）。
            teardown(&session, gen).await;
            return;
        }
    }

    loop {
        tokio::select! {
            // 让位：新订阅接管（会话不摘——它仍归新订阅所有）。
            _ = &mut replace_rx => {
                teardown(&session, gen).await;
                return;
            }
            // 退出：watch 先于 reader EOF 到达时由此收尾——先断生产（防
            // 「退出后才接管」的 recv 悬挂），再排净尾量后按两维判定摘除。
            _ = exited_rx.changed() => {
                session.outbound.clear_if(gen);
                finish_exit(&sessions, &session, &session_id, gen, &mut sink, &mut outbound).await;
                return;
            }
            // 下行：PTY 输出 → sink；recv 得 None = 通道已排空且无生产者（reader
            // EOF 或已换代），交 [`finish_exit`] 判定收尾。
            chunk = outbound.recv() => match chunk {
                Some(chunk) => {
                    if sink.send(chunk.as_slice()).await.is_err() {
                        teardown(&session, gen).await;
                        return;
                    }
                }
                None => {
                    finish_exit(&sessions, &session, &session_id, gen, &mut sink, &mut outbound).await;
                    return;
                }
            }
        }
    }
}

/// 退出收尾（规格 §3.2：排空 outbound → 摘除判定）。**摘除判定取两维**
/// （审查 I-1，自 WS 版原样平移）：
/// 1. hub 代次：`rx.recv()` 得 `None` 有两义——reader EOF（真退出）或本代
///    通道已被新订阅方换出（替换 ① 换 hub 与 ④ 装槽之间）；hub 出现别的
///    代次即替换在途，绝不摘。`exited` watch 对被替换的旧订阅同样触发，
///    此维同样拦住旧订阅的迟到退出事件。
/// 2. 槽内代次：仅当本订阅仍是当前接管者才摘。判定与清槽在槽锁内原子完成
///    （新订阅方安装槽与本判定互斥），且 attach ④ 装槽后复查注册表
///    （[`attach`]）与本判定互为收口——任一交错次序下都不会出现
///    「活会话被旧任务摘除且新订阅不自知」。
async fn finish_exit<S: ByteSink>(
    sessions: &Sessions,
    session: &Session,
    session_id: &str,
    gen: u64,
    sink: &mut S,
    outbound_rx: &mut tokio::sync::mpsc::Receiver<Chunk>,
) {
    while let Some(chunk) = outbound_rx.recv().await {
        if sink.send(chunk.as_slice()).await.is_err() {
            break;
        }
    }
    // hub 维：出现其他代次 = 新订阅已换入 hub、装槽在途——不摘。
    let replacement_in_flight = matches!(session.outbound.current(), Some((g, _)) if g != gen);
    let mut slot = session.connection.lock().await;
    let is_current = !replacement_in_flight && slot.as_ref().is_some_and(|s| s.gen == gen);
    if is_current {
        sessions
            .lock()
            .expect("sessions mutex poisoned")
            .remove(session_id);
        *slot = None;
    }
    drop(slot);
    session.outbound.clear_if(gen);
}

/// 终点断开的收尾：清槽（仅当仍为本订阅）与 hub（仅当代次匹配）。会话保留
/// 在注册表——同 id 可再次订阅（webview reload）。
async fn teardown(session: &Session, gen: u64) {
    let mut slot = session.connection.lock().await;
    if slot.as_ref().is_some_and(|s| s.gen == gen) {
        *slot = None;
    }
    drop(slot);
    session.outbound.clear_if(gen);
}

fn unknown_session(id: &str) -> PtyError {
    PtyError::Io(std::io::Error::other(format!("unknown session: {id}")))
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use super::{attach, forward_stream, ByteSink, Sessions};
    use crate::session::{Session, SpawnRequest};

    #[derive(Clone, Default)]
    struct Collect(Arc<Mutex<Vec<u8>>>);

    impl ByteSink for Collect {
        type Error = std::io::Error;

        async fn send(&mut self, bytes: &[u8]) -> Result<(), Self::Error> {
            self.0.lock().expect("collect mutex poisoned").extend_from_slice(bytes);
            Ok(())
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn late_none_during_replacement_keeps_session() {
        // 复现（审查 I-1 / M-4）：订阅替换 ①换 hub → ④装槽 之间，旧订阅 A 的
        // 迟到 `rx.recv()→None` 进入 [`forward_stream::finish_exit`] 时，不得把
        // 仍在册的活会话摘除。多线程 flavor + 手工构造交错次序：hub 先换入新
        // 代次（替换 ①），A 的 gen1 通道随即失生产者、rx-None 就绪——A 醒来时
        // hub 维判定必然见到在途替换。修复前此处确定性摘除（is_current 槽维
        // 判定单维误判真）。
        let sessions: Sessions = Arc::new(Mutex::new(HashMap::new()));

        let (session, _runtime) = Session::spawn(SpawnRequest {
            cols: 80,
            rows: 24,
            cwd: Some(std::env::temp_dir().to_string_lossy().into_owned()),
            env: HashMap::new(),
            env_to_delete: Vec::new(),
            command: None,
            shell_override: Some("/bin/sh".to_string()),
        })
        .unwrap();
        let id = session.id.clone();
        sessions
            .lock()
            .unwrap()
            .insert(id.clone(), Arc::clone(&session));

        // 订阅 A（真实接管：hub gen1 + 槽 gen1 + forward 任务）。
        let a = attach(&sessions, &id).await.unwrap();
        let slot = session.connection.lock().await;
        assert_eq!(slot.as_ref().map(|s| s.gen), Some(1));
        drop(slot);
        let a_task = tokio::spawn(forward_stream(a, Collect::default()));

        // 手工执行 B 的替换①（仅换 hub，④装槽不动）——模拟审查指出的窗口。
        let (tx2, _rx2) = tokio::sync::mpsc::channel(64);
        assert_eq!(session.outbound.replace(tx2), 2);

        // A 的 rx-None 触发 finish_exit：修复后按 hub 维判定「替换在途」不摘，
        // 仅终止流。等 A 的 forward 任务结束（bounded）。
        tokio::time::timeout(Duration::from_secs(5), a_task)
            .await
            .expect("old forward task must end")
            .expect("old forward task ok");
        // 核心断言：会话必须仍在注册表（修复前被 A 的 finish_exit 误摘）。
        assert!(
            sessions.lock().unwrap().contains_key(&id),
            "late rx-None of the replaced subscription must not remove the live session"
        );

        // 自愈：真实订阅 C 照常接管（hub gen3 + 槽 gen3 + roundtrip）。
        let c = attach(&sessions, &id).await.unwrap();
        let collect = Collect::default();
        let c_task = tokio::spawn(forward_stream(c, collect.clone()));
        session.write(b"echo c-ok\n").unwrap();
        let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
        let mut ok = false;
        while tokio::time::Instant::now() < deadline && !ok {
            tokio::time::sleep(Duration::from_millis(20)).await;
            ok = String::from_utf8_lossy(&collect.0.lock().unwrap()).contains("c-ok");
        }
        assert!(
            ok,
            "third subscription must roundtrip after the window; got: {:?}",
            String::from_utf8_lossy(&collect.0.lock().unwrap())
        );
        c_task.abort();
    }
}
