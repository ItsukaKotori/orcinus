//! Task 7 集成测试：WS 会话路由（双向帧、连接替换、退出关闭语义）。
//!
//! 四用例对应 brief Step 1；全部 `#[cfg(unix)]`（会话测试仅在 unix 运行，与
//! tests/session.rs 口径一致）。装配：测试自建 `Arc<Mutex<HashMap<…>>>` 注册表，
//! handler 闭包捕获后交 `server::route_connection` 查表路由——Task 9 的 PtyHost
//! 用同一结构直接挂接。

#[cfg(unix)]
mod unix_tests {
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};

    use futures_util::{SinkExt, StreamExt};
    use tokio_tungstenite::connect_async;
    use tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode;
    use tokio_tungstenite::tungstenite::Message;

    use ade_pty::server;
    use ade_pty::session::{Session, SpawnRequest};

    /// 单帧读循环的统一时限：超时即 panic 并带上已聚合内容，便于诊断。
    const TIMEOUT: Duration = Duration::from_secs(5);

    type Ws = tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >;

    fn base_request() -> SpawnRequest {
        SpawnRequest {
            cols: 80,
            rows: 24,
            cwd: Some(std::env::temp_dir().to_string_lossy().into_owned()),
            env: HashMap::new(),
            env_to_delete: Vec::new(),
            command: None,
            shell_override: Some("/bin/sh".to_string()),
        }
    }

    /// 测试装配：绑 127.0.0.1:0 起「会话路由版」server + spawn `/bin/sh` 并入注册表。
    struct Host {
        port: u16,
        token: String,
        session: Arc<Session>,
        _serve: tokio::task::JoinHandle<()>,
    }

    fn spawn_host() -> Host {
        // WHY: 与原 test_support 相同的手法——`Handle::block_on` 在运行时上下文
        // 内会 panic，故先以 std 绑定端口再注册为异步 listener。
        let std_listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = std_listener.local_addr().unwrap().port();
        std_listener.set_nonblocking(true).unwrap();
        let listener = tokio::net::TcpListener::from_std(std_listener).unwrap();
        let token = server::generate_token();

        // 注册表由测试装配；Task 9 的 PtyHost 内部为同一结构。
        let sessions: server::Sessions = Arc::new(Mutex::new(HashMap::new()));
        let handler: server::ConnectionHandler = {
            let sessions = Arc::clone(&sessions);
            Arc::new(move |id, ws| server::route_connection(&sessions, id, ws))
        };
        let serve = tokio::spawn(server::serve(listener, token.clone(), handler));

        let (session, _runtime) = Session::spawn(base_request()).expect("spawn session");
        sessions
            .lock()
            .expect("sessions mutex poisoned")
            .insert(session.id.clone(), Arc::clone(&session));

        Host {
            port,
            token,
            session,
            _serve: serve,
        }
    }

    async fn connect(host: &Host) -> Ws {
        let (ws, _resp) = connect_async(format!(
            "ws://127.0.0.1:{}/pty/{}?token={}",
            host.port, host.session.id, host.token
        ))
        .await
        .expect("handshake");
        ws
    }

    fn find(haystack: &[u8], needle: &[u8]) -> bool {
        haystack.windows(needle.len()).any(|w| w == needle)
    }

    /// 循环读帧聚合 Binary，直到 `pred` 命中；总时限 [`TIMEOUT`]。
    async fn aggregate_until(ws: &mut Ws, pred: impl Fn(&[u8]) -> bool) -> Vec<u8> {
        let deadline = Instant::now() + TIMEOUT;
        let mut agg: Vec<u8> = Vec::new();
        loop {
            if pred(&agg) {
                return agg;
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            let msg = tokio::time::timeout(remaining, ws.next()).await;
            match msg {
                Err(_) => panic!(
                    "timed out after {TIMEOUT:?}; got {} bytes: {:?}",
                    agg.len(),
                    String::from_utf8_lossy(&agg)
                ),
                Ok(None) => panic!(
                    "ws stream ended before predicate matched; got: {:?}",
                    String::from_utf8_lossy(&agg)
                ),
                Ok(Some(Err(e))) => panic!("ws stream error: {e}"),
                Ok(Some(Ok(Message::Binary(bytes)))) => agg.extend_from_slice(bytes.as_ref()),
                Ok(Some(Ok(Message::Close(frame)))) => panic!(
                    "server closed early: {frame:?}; got: {:?}",
                    String::from_utf8_lossy(&agg)
                ),
                Ok(Some(Ok(_))) => {}
            }
        }
    }

    /// 循环读帧直到收到 Close，返回其 code；收到 Binary 一律聚合丢弃
    /// （提示/回显先行属正常时序）。
    async fn wait_close(ws: &mut Ws) -> CloseCode {
        let deadline = Instant::now() + TIMEOUT;
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            let msg = tokio::time::timeout(remaining, ws.next()).await;
            match msg {
                Err(_) => panic!("timed out waiting for Close frame"),
                Ok(None) => panic!("ws stream ended without a Close frame"),
                Ok(Some(Err(e))) => panic!("ws stream error: {e}"),
                Ok(Some(Ok(Message::Close(frame)))) => {
                    return frame.expect("close frame present").code;
                }
                Ok(Some(Ok(_))) => {}
            }
        }
    }

    /// ① 上行输入经会话写 master，PTY 回显经下行帧返回（roundtrip）。
    #[tokio::test]
    async fn roundtrip_input_output_over_ws() {
        let host = spawn_host();
        let mut ws = connect(&host).await;
        ws.send(Message::binary(b"echo ws-ok\n".as_slice()))
            .await
            .expect("send input");
        let agg = aggregate_until(&mut ws, |bytes| find(bytes, b"ws-ok")).await;
        assert!(
            find(&agg, b"ws-ok"),
            "got: {:?}",
            String::from_utf8_lossy(&agg)
        );
    }

    /// ② spawn 后先不连：300ms 内经 write API 写入的输出落在 pre-attach 环形，
    /// 首连时重放（首帧区含 `early`）。
    #[tokio::test]
    async fn pre_attach_buffer_replayed_on_first_connect() {
        let host = spawn_host();
        tokio::time::sleep(Duration::from_millis(300)).await;
        host.session.write(b"echo early\n").expect("write");
        let mut ws = connect(&host).await;
        let agg = aggregate_until(&mut ws, |bytes| find(bytes, b"early")).await;
        assert!(
            find(&agg, b"early"),
            "got: {:?}",
            String::from_utf8_lossy(&agg)
        );
    }

    /// ③ 连接替换：同 id 新连接接管后旧连接收到 close(1000)，新连接照常收发。
    #[tokio::test]
    async fn connection_replaced() {
        let host = spawn_host();
        let mut a = connect(&host).await;
        // A 先聚合一点输出（prompt），确认它处于正常收发状态再替换。
        let _ = tokio::time::timeout(Duration::from_millis(300), a.next()).await;

        let mut b = connect(&host).await;
        let code = wait_close(&mut a).await;
        assert_eq!(
            code,
            CloseCode::Normal,
            "old connection must get close(1000)"
        );

        b.send(Message::binary(b"echo b-ok\n".as_slice()))
            .await
            .expect("send via new connection");
        let agg = aggregate_until(&mut b, |bytes| find(bytes, b"b-ok")).await;
        assert!(
            find(&agg, b"b-ok"),
            "got: {:?}",
            String::from_utf8_lossy(&agg)
        );

        // 第三连 C：替换不得摘除注册表（会话仍归 B/C 所有），同 id 仍可路由。
        let mut c = connect(&host).await;
        c.send(Message::binary(b"echo c-ok\n".as_slice()))
            .await
            .expect("send via third connection");
        let agg = aggregate_until(&mut c, |bytes| find(bytes, b"c-ok")).await;
        assert!(
            find(&agg, b"c-ok"),
            "got: {:?}",
            String::from_utf8_lossy(&agg)
        );
    }

    /// ④ 退出关闭：子进程退出后，排空 outbound → close(1000)（规格 §3.2）。
    #[tokio::test]
    async fn close_on_exit() {
        let host = spawn_host();
        let mut ws = connect(&host).await;
        ws.send(Message::binary(b"exit 0\n".as_slice()))
            .await
            .expect("send exit");
        let code = wait_close(&mut ws).await;
        assert_eq!(code, CloseCode::Normal, "session exit must close with 1000");
    }
}
