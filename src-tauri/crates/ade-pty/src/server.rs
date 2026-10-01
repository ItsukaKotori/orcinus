//! WS 数据面：accept 循环 + 逐连接 path/token 鉴权 + 会话路由（Task 7）。
//!
//! [`serve`] 只做鉴权；鉴权通过后交 [`route_connection`]——按 `/pty/<id>` 查
//! 注册表，完成连接替换（旧连接 close(1000) 让位）、双向二进制帧搬运与退出
//! 收尾（排空 outbound → close(1000) → 从注册表摘除，规格 §3.2）。
//! 注册表 `Arc<Mutex<HashMap<String, Arc<Session>>>>` 由宿主装配（测试直接建、
//! Task 9 的 PtyHost 持有同构字段），handler 闭包捕获后传入 [`serve`]。

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use tokio::net::TcpListener;
use tokio_tungstenite::tungstenite::handshake::server::{Request, Response};
use tokio_tungstenite::tungstenite::protocol::{frame::coding::CloseCode, CloseFrame};
use tokio_tungstenite::tungstenite::Message;

use crate::session::{Chunk, ConnectionSlot, Session, OUTBOUND_CAPACITY};

/// 单条已鉴权 WS 连接（Task 7 会话路由的输入）。
pub type WsConn = tokio_tungstenite::WebSocketStream<tokio::net::TcpStream>;

/// 连接接管回调：拿到 session id 与已鉴权连接。
pub type ConnectionHandler = Arc<dyn Fn(String, WsConn) + Send + Sync>;

/// 数据面 endpoint：宿主进程把 port/token 交给前端与 agent 侧连接。
#[derive(Clone, Debug)]
pub struct DataEndpoint {
    pub port: u16,
    pub token: String,
}

/// 生成连接 token：rand 32 字节 hex 化，64 字符。
pub fn generate_token() -> String {
    let mut bytes = [0u8; 32];
    rand::fill(&mut bytes);
    bytes.iter().map(|x| format!("{x:02x}")).collect()
}

/// accept 循环：逐连接升级 WS 并校验 path/token；
/// 路径不符 `/pty/<id>` 或 token 不符 → close(1008)。校验通过后 handler 接管连接。
// WHY: Err 侧的 `ErrorResponse`（HttpResponse<Option<String>>）由 tungstenite 的
// Callback trait 签名强制，回调处无法缩小。
#[allow(clippy::result_large_err)]
pub async fn serve(listener: TcpListener, token: String, handler: ConnectionHandler) {
    loop {
        // accept 的持续性错误（fd 耗尽等）会热忙循环——固定 100ms 退避，成功
        // 自然重置（Task 2 备忘）。
        let (stream, _addr) = match listener.accept().await {
            Ok(pair) => pair,
            Err(_) => {
                tokio::time::sleep(ACCEPT_BACKOFF).await;
                continue;
            }
        };
        let token = token.clone();
        let handler = Arc::clone(&handler);
        tokio::spawn(async move {
            // accept_hdr_async 的回调在握手响应发出前调用，可借回捕获请求 URI；
            // 闭包对 uri 的借用随本语句的未来一起结束。
            let mut uri = None;
            let ws =
                tokio_tungstenite::accept_hdr_async(stream, |req: &Request, resp: Response| {
                    uri = Some(req.uri().clone());
                    Ok(resp)
                })
                .await;
            let mut ws = match ws {
                Ok(ws) => ws,
                // 握手失败即断开，无可通知对象。
                Err(_) => return,
            };
            let Some(uri) = uri else { return };
            let session_id = uri
                .path()
                .strip_prefix("/pty/")
                .map(|rest| rest.split('?').next().unwrap_or(""))
                .unwrap_or("");
            let query_token = uri
                .query()
                .and_then(|q| q.split('&').find_map(|kv| kv.strip_prefix("token=")));
            let authorized =
                !session_id.is_empty() && query_token.is_some_and(|t| constant_time_eq(t, &token));
            if !authorized {
                let _ = ws
                    .close(Some(CloseFrame {
                        code: CloseCode::Policy,
                        reason: "unauthorized".into(),
                    }))
                    .await;
                return;
            }
            handler(session_id.to_string(), ws);
        });
    }
}

/// accept 错误退避（fd 耗尽等持续性错误的热忙循环防护）。
const ACCEPT_BACKOFF: Duration = Duration::from_millis(100);

/// 会话注册表：id → 会话（Task 9 的 PtyHost 持有同构字段）。
pub type SessionsMap = HashMap<String, Arc<Session>>;
/// 共享注册表句柄（server handler 与宿主共用）。
pub type Sessions = Arc<std::sync::Mutex<SessionsMap>>;

/// 会话路由入口（handler 与 Task 9 PtyHost 共用）：id 未注册 → close(1008)
/// （规格 §4.7「未知 id 即 close」）；否则接管连接（连接替换 + 双向帧搬运 +
/// 退出收尾）。
pub fn route_connection(sessions: &Sessions, session_id: String, mut ws: WsConn) {
    let session = sessions
        .lock()
        .expect("sessions mutex poisoned")
        .get(&session_id)
        .cloned();
    let Some(session) = session else {
        tokio::spawn(async move {
            let _ = ws
                .close(Some(CloseFrame {
                    code: CloseCode::Policy,
                    reason: "unknown session".into(),
                }))
                .await;
        });
        return;
    };
    let sessions = Arc::clone(sessions);
    tokio::spawn(attach_session(sessions, session, session_id, ws));
}

/// 接管一条连接。次序即正确性（与 [`Session::connect_channel`] 同一套协议）：
///
/// ① 建新下行通道并换入 hub（取得代次）——此后 reader 的新块只投递到新通道；
/// ② 确认会话未被并发退出路径摘除（退出收尾与本接管并发时，接管的是已退
///    会话 → close(1000) 收场，见 [`forward_loop`] 的退出判定）；
/// ③ 排空并停用 pre-attach（首连重放存量；替换时已停用、得空集）。与 reader
///    的 push 同锁串行：换入后停用前 push 的块全在重放集里，停用后的块走新
///    通道——「重放 ∪ 通道」不重不漏且有序；
/// ④ 安装新连接槽并让旧连接让位：旧任务自行向其 WS 发 close(1000)（webview
///    reload 重挂路径，规格 §3.2）并 drop 其通道接收端（转发自然终止）。
async fn attach_session(sessions: Sessions, session: Arc<Session>, session_id: String, ws: WsConn) {
    let (outbound_tx, outbound_rx) = tokio::sync::mpsc::channel(OUTBOUND_CAPACITY);
    let gen = session.outbound.replace(outbound_tx);
    if !sessions
        .lock()
        .expect("sessions mutex poisoned")
        .contains_key(&session_id)
    {
        session.outbound.clear_if(gen);
        let mut ws = ws;
        let _ = ws
            .close(Some(CloseFrame {
                code: CloseCode::Normal,
                reason: "session ended".into(),
            }))
            .await;
        return;
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
    // 本连接不得继续以该会话名义转发。让位收场：清本代槽/hub（gen 判定防误删
    // 后续接管方）、close(1000)。已让位的旧连接自身会按两维判定不摘，安全收敛。
    if !sessions
        .lock()
        .expect("sessions mutex poisoned")
        .contains_key(&session_id)
    {
        teardown(&session, gen).await;
        let mut ws = ws;
        let _ = ws
            .close(Some(CloseFrame {
                code: CloseCode::Normal,
                reason: "session ended".into(),
            }))
            .await;
        return;
    }

    tokio::spawn(forward_loop(Takeover {
        sessions,
        session,
        session_id,
        ws,
        gen,
        replay,
        outbound_rx,
        replace_rx,
    }));
}

/// 接管产物（[`attach_session`] → [`forward_loop`] 的交接包）。
struct Takeover {
    sessions: Sessions,
    session: Arc<Session>,
    session_id: String,
    ws: WsConn,
    gen: u64,
    replay: Vec<Chunk>,
    outbound_rx: tokio::sync::mpsc::Receiver<Chunk>,
    replace_rx: tokio::sync::oneshot::Receiver<()>,
}

/// 双向帧搬运与收尾（一个任务同时服务上行/下行/让位/退出四路事件）：
/// - 下行：先逐帧发 pre-attach 重放存量，再循环通道 `recv()` → `Binary`；
/// - 上行：`Binary(data)` → 经 [`Session::input`] 有界通道移交写线程（协议
///   §3.2：binary 即原始字节，无文本帧——文本帧忽略）；
/// - 让位：新接管方发信号 → 本连接 close(1000) → 终止（通道接收端随之 drop）；
/// - 退出（规格 §3.2 顺序）：排空 outbound → close(1000) → 从注册表摘除。
///   退出检测源有二，殊途同归到 [`finish_exit`]：reader EOF 断开 hub 后通道排空
///   recv 得 `None`；[`Session::exited`] watch（子进程死）在 reader 尚未排完时
///   先到，此时断开 hub 再继续 recv 把尾量送完。客户端主动断开**不**摘会话
///   （webview reload 会重连同 id）。
async fn forward_loop(t: Takeover) {
    let Takeover {
        sessions,
        session,
        session_id,
        ws,
        gen,
        replay,
        mut outbound_rx,
        mut replace_rx,
    } = t;
    let (mut sink, mut stream) = ws.split();
    let mut exited = session.exited.clone();

    // 退出收尾（规格 §3.2：排空 outbound → close(1000) → 从注册表摘除）。
    // **摘除判定取两维**（审查 I-1）：
    // 1. hub 代次：`rx.recv()` 得 `None` 有两义——reader EOF（真退出）或本代
    //    通道已被新接管方换出（替换 ① 换 hub 与 ④ 装槽之间）；hub 出现别的
    //    代次即替换在途，绝不摘。`exited` watch 对被替换的旧连接同样触发，
    //    此维同样拦住旧连接的迟到退出事件。
    // 2. 槽内代次：仅当本连接仍是当前接管者才摘。判定与清槽在槽锁内原子完成
    //    （新接管方安装槽与本判定互斥），且 attach ④ 装槽后复查注册表
    //    （[`attach_session`]）与本判定互为收口——任一交错次序下都不会出现
    //    「活会话被旧任务摘除且新连接不自知」。
    async fn finish_exit(
        sessions: &Sessions,
        session: &Session,
        session_id: &str,
        gen: u64,
        mut sink: futures_util::stream::SplitSink<WsConn, Message>,
        mut outbound_rx: tokio::sync::mpsc::Receiver<Chunk>,
    ) {
        while let Some(chunk) = outbound_rx.recv().await {
            if sink.send(Message::Binary(chunk.bytes())).await.is_err() {
                break;
            }
        }
        let _ = sink
            .send(Message::Close(Some(CloseFrame {
                code: CloseCode::Normal,
                reason: "session ended".into(),
            })))
            .await;
        // hub 维：出现其他代次 = 新接管已换入 hub、装槽在途——不摘。
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

    // 首连重放：存量先于通道实时块（接管次序保证），逐帧直发 WS。
    for chunk in replay {
        if sink.send(Message::Binary(chunk.bytes())).await.is_err() {
            // 客户端在重放期间消失：按「客户端断开」收尾（不摘会话）。
            teardown(&session, gen).await;
            return;
        }
    }

    loop {
        tokio::select! {
            // 让位：新连接接管（会话不摘——它仍归新连接所有）。
            _ = &mut replace_rx => {
                let _ = sink.send(Message::Close(Some(CloseFrame {
                    code: CloseCode::Normal,
                    reason: "replaced".into(),
                }))).await;
                teardown(&session, gen).await;
                break;
            }
            // 退出：watch 先于 reader EOF 到达时由此收尾——先断生产（防
            // 「退出后才接管」的 recv 悬挂），再排净尾量后按两维判定摘除。
            _ = exited.changed() => {
                session.outbound.clear_if(gen);
                finish_exit(&sessions, &session, &session_id, gen, sink, outbound_rx).await;
                break;
            }
            // 下行：PTY 输出 → WS；recv 得 None = 通道已排空且无生产者（reader
            // EOF 或已换代），交 [`finish_exit`] 判定收尾。
            chunk = outbound_rx.recv() => match chunk {
                Some(chunk) => {
                    if sink.send(Message::Binary(chunk.bytes())).await.is_err() {
                        teardown(&session, gen).await;
                        break;
                    }
                }
                None => {
                    finish_exit(&sessions, &session, &session_id, gen, sink, outbound_rx).await;
                    break;
                }
            },
            // 上行：键盘输入 → 有界通道（写线程串行写 master）。满则在此背压
            // 等待——分支体不受 select 取消影响，帧不丢；tty 阻塞由写线程承担，
            // 不冻结本任务（审查 I-2）。
            msg = stream.next() => match msg {
                Some(Ok(Message::Binary(bytes))) => {
                    let _ = session.input.send(bytes.to_vec()).await;
                }
                // 客户端断开/出错：清本连接的槽与 hub（代次判据防误清新接管方），
                // 但不摘会话——同 id 可重连（webview reload；post-attach 不缓冲，
                // 规格接受重连期间输出弃置）。
                Some(Ok(Message::Close(_))) | Some(Err(_)) | None => {
                    teardown(&session, gen).await;
                    break;
                }
                Some(Ok(_)) => {}
            }
        }
    }
}

/// 客户端断开的收尾：清槽（仅当仍为本连接）与 hub（仅当代次匹配）。会话保留
/// 在注册表——同 id 可再次连接（webview reload）。
async fn teardown(session: &Session, gen: u64) {
    let mut slot = session.connection.lock().await;
    if slot.as_ref().is_some_and(|s| s.gen == gen) {
        *slot = None;
    }
    drop(slot);
    session.outbound.clear_if(gen);
}

/// 长度先比后逐字节 XOR 累积，避免提前短路造成的时序侧信道。
fn constant_time_eq(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use futures_util::{SinkExt, StreamExt};

    use super::{
        constant_time_eq, generate_token, route_connection, serve, ConnectionHandler, Sessions,
    };
    use crate::session::{Session, SpawnRequest};

    #[test]
    fn token_is_64_hex_chars_and_random() {
        let t = generate_token();
        assert_eq!(t.len(), 64);
        assert!(t.chars().all(|c| c.is_ascii_hexdigit()));
        assert_ne!(generate_token(), t, "tokens must not repeat");
    }

    #[test]
    fn equal_strings_match() {
        assert!(constant_time_eq("abc", "abc"));
        assert!(constant_time_eq("", ""));
    }

    #[test]
    fn differing_strings_do_not_match() {
        assert!(!constant_time_eq("abc", "abd"));
        assert!(!constant_time_eq("abc", "ab"));
        assert!(!constant_time_eq("", "a"));
    }

    /// 复现（审查 I-1 / M-4）：连接替换 ①换 hub → ④装槽 之间，旧连接 A 的
    /// 迟到 `rx.recv()→None` 进入 [`forward_loop::finish_exit`] 时，不得把仍在册
    /// 的活会话摘除。多线程 flavor + 手工构造交错次序：hub 先换入新代次（替换
    /// ①），A 的 gen1 通道随即失生产者、rx-None 就绪——A 醒来时 hub 维判定必然
    /// 见到在途替换。修复前此处确定性摘除（is_current 槽维判定单维误判真）。
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn late_none_during_replacement_keeps_session() {
        let std_listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = std_listener.local_addr().unwrap().port();
        std_listener.set_nonblocking(true).unwrap();
        let listener = tokio::net::TcpListener::from_std(std_listener).unwrap();
        let token = generate_token();
        let sessions: Sessions = Arc::new(Mutex::new(HashMap::new()));
        let handler: ConnectionHandler = {
            let sessions = Arc::clone(&sessions);
            Arc::new(move |id, ws| route_connection(&sessions, id, ws))
        };
        let _serve = tokio::spawn(serve(listener, token.clone(), handler));

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

        // 连接 A（真实接管：hub gen1 + 槽 gen1 + 转发任务）。
        let (mut a, _resp) = tokio_tungstenite::connect_async(format!(
            "ws://127.0.0.1:{port}/pty/{id}?token={token}"
        ))
        .await
        .unwrap();
        let slot = session.connection.lock().await;
        assert_eq!(slot.as_ref().map(|s| s.gen), Some(1));
        drop(slot);

        // 手工执行 B 的替换①（仅换 hub，④装槽不动）——模拟审查指出的窗口。
        let (tx2, _rx2) = tokio::sync::mpsc::channel(64);
        assert_eq!(session.outbound.replace(tx2), 2);

        // A 的 rx-None 触发 finish_exit：修复后按 hub 维判定「替换在途」不摘，
        // 仅 close(1000)。等 A 的 Close 帧（bounded）。
        let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
        let mut a_closed = false;
        while tokio::time::Instant::now() < deadline {
            match tokio::time::timeout(Duration::from_millis(100), a.next()).await {
                Ok(Some(Ok(tokio_tungstenite::tungstenite::Message::Close(_)))) => {
                    a_closed = true;
                    break;
                }
                Ok(Some(Ok(_))) => {}
                _ => {}
            }
        }
        assert!(a_closed, "A must be closed by the replacement takeover");
        // 核心断言：会话必须仍在注册表（修复前被 A 的 finish_exit 误摘）。
        assert!(
            sessions.lock().unwrap().contains_key(&id),
            "late rx-None of the replaced connection must not remove the live session"
        );

        // 自愈：真实连接 C 照常路由（hub gen3 + 槽 gen3 + roundtrip）。
        let (mut c, _resp) = tokio_tungstenite::connect_async(format!(
            "ws://127.0.0.1:{port}/pty/{id}?token={token}"
        ))
        .await
        .unwrap();
        c.send(tokio_tungstenite::tungstenite::Message::binary(
            b"echo c-ok\n".as_slice(),
        ))
        .await
        .unwrap();
        let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
        let mut agg: Vec<u8> = Vec::new();
        let mut ok = false;
        while tokio::time::Instant::now() < deadline && !ok {
            match tokio::time::timeout(Duration::from_millis(200), c.next()).await {
                Ok(Some(Ok(tokio_tungstenite::tungstenite::Message::Binary(bytes)))) => {
                    agg.extend_from_slice(bytes.as_ref());
                    ok = agg.windows(4).any(|w| w == b"c-ok");
                }
                Ok(Some(Ok(_))) => {}
                _ => break,
            }
        }
        assert!(
            ok,
            "third connection must roundtrip after the window; got: {:?}",
            String::from_utf8_lossy(&agg)
        );
    }
}
