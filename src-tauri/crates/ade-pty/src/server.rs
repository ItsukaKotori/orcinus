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
/// - 上行：`Binary(data)` → `session.write`（协议 §3.2：binary 即原始字节，
///   无文本帧——文本帧忽略）；
/// - 让位：新接管方发信号 → 本连接 close(1000) → 终止（通道接收端随之 drop）；
/// - 退出（规格 §3.2 顺序）：排空 outbound → close(1000) → 从注册表摘除。
///   退出检测源有二，殊途同归到同一收尾：reader EOF 断开 hub 后通道排空 recv
///   得 `None`；[`Session::exited`] watch（子进程死）在 reader 尚未排完时先到，
///   此时断开 hub 再继续 recv 把尾量送完。客户端主动断开**不**摘会话（webview
///   reload 会重连同 id）。
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
    // 退出/让位共用的收尾：close(1000) + 清槽/清 hub；`remove` 仅退出路径按
    // 代次判定（见 [`finish_exit`]）。
    async fn finish(
        sessions: &Sessions,
        session: &Session,
        session_id: &str,
        gen: u64,
        mut sink: futures_util::stream::SplitSink<WsConn, Message>,
        reason: &'static str,
        remove: bool,
    ) {
        let _ = sink
            .send(Message::Close(Some(CloseFrame {
                code: CloseCode::Normal,
                reason: reason.into(),
            })))
            .await;
        if remove {
            sessions
                .lock()
                .expect("sessions mutex poisoned")
                .remove(session_id);
        }
        teardown(session, gen).await;
    }

    // 退出收尾（规格 §3.2：排空 outbound → close(1000) → 从注册表摘除）。
    // **摘除以槽内代次为准**：仅当本连接仍是当前接管者才摘。`rx.recv()` 得
    // `None` 有两义——reader EOF（真退出）或本代通道已被新接管方换出（连接
    // 替换的伴生事件）；[`Session::exited`] watch 对被替换的旧连接同样会触发。
    // 两者的迟到事件都不得摘掉新连接名下的会话，故统一走代次判定，且判定与
    // 清槽在槽锁内原子完成（新接管方安装槽与本判定互斥）。
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
        let mut slot = session.connection.lock().await;
        let is_current = slot.as_ref().is_some_and(|s| s.gen == gen);
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
                finish(&sessions, &session, &session_id, gen, sink, "replaced", false).await;
                break;
            }
            // 退出：watch 先于 reader EOF 到达时由此收尾——先断生产（防
            // 「退出后才接管」的 recv 悬挂），再排净尾量后按代次判定摘除。
            _ = exited.changed() => {
                session.outbound.clear_if(gen);
                finish_exit(&sessions, &session, &session_id, gen, sink, outbound_rx).await;
                break;
            }
            // 下行：PTY 输出 → WS；recv 得 None = 通道已排空且无生产者（reader
            // EOF 或已换代），交 [`finish_exit`] 按代次判定收尾。
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
            // 上行：键盘输入 → master writer。阻塞写保序（spawn_blocking 会乱序）；
            // 击键级数据短暂阻塞本连接任务可接受（1C）。
            msg = stream.next() => match msg {
                Some(Ok(Message::Binary(bytes))) => {
                    let _ = session.write(&bytes);
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
    use super::{constant_time_eq, generate_token};

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
}
