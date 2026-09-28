//! WS 数据面服务端骨架：accept 循环 + 逐连接 path/token 鉴权。
//!
//! Task 2 只要求 echo 行为（见 `lib.rs` 的 `test_support`）；Task 7 会把
//! handler 换成会话路由，accept 循环与鉴权逻辑保持不变。

use std::sync::Arc;

use tokio::net::TcpListener;
use tokio_tungstenite::tungstenite::handshake::server::{Request, Response};
use tokio_tungstenite::tungstenite::protocol::{frame::coding::CloseCode, CloseFrame};

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
        let Ok((stream, _addr)) = listener.accept().await else {
            continue;
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
