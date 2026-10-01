//! Task 15 修订二集成测试：进程内流式订阅（subscribe/forward_stream）。
//!
//! 六用例对应原 ws_server(2) + ws_session(4) 的语义等价重写（偏差 4 修订二：
//! 数据面 WS → Tauri Channel 分块，鉴权/token/端口整体取消——原 rejects_bad_token
//! 的「不可达连接被拒」语义由 unknown_id_errors 承接）。全部 `#[cfg(unix)]`
//! （会话测试仅在 unix 运行，与 tests/session.rs 口径一致）。

#[cfg(unix)]
mod unix_tests {
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};

    use ade_pty::server::{forward_stream, ByteSink, SessionStream};
    use ade_pty::session::SpawnRequest;
    use ade_pty::{PtyEvent, PtyHost};

    /// 单步等待的统一时限：超时即 panic 并带上已聚合内容，便于诊断。
    const TIMEOUT: Duration = Duration::from_secs(5);

    fn base_request() -> SpawnRequest {
        SpawnRequest {
            cols: 80,
            rows: 24,
            cwd: Some(std::env::temp_dir().to_string_lossy().into_owned()),
            env: std::collections::HashMap::new(),
            env_to_delete: Vec::new(),
            command: None,
            shell_override: Some("/bin/sh".to_string()),
        }
    }

    /// 收集型 sink：字节进共享缓冲（克隆共享），`fail` 置位后模拟终点消失。
    #[derive(Clone, Default)]
    struct Collect {
        bytes: Arc<Mutex<Vec<u8>>>,
        fail: Arc<Mutex<bool>>,
    }

    impl ByteSink for Collect {
        type Error = std::io::Error;

        async fn send(&mut self, bytes: &[u8]) -> Result<(), Self::Error> {
            if *self.fail.lock().expect("fail mutex poisoned") {
                return Err(std::io::Error::other("sink closed"));
            }
            self.bytes
                .lock()
                .expect("collect mutex poisoned")
                .extend_from_slice(bytes);
            Ok(())
        }
    }

    impl Collect {
        fn snapshot(&self) -> Vec<u8> {
            self.bytes.lock().expect("collect mutex poisoned").clone()
        }
    }

    /// 装配：PtyHost（无端点形态）+ 事件回调入 mpsc。
    struct Fixture {
        host: Arc<PtyHost>,
        events: std::sync::mpsc::Receiver<PtyEvent>,
    }

    fn fixture() -> Fixture {
        let host = PtyHost::start(tokio::runtime::Handle::current()).expect("start pty host");
        let (tx, rx) = std::sync::mpsc::channel::<PtyEvent>();
        host.set_event_callback(Box::new(move |event| {
            let _ = tx.send(event);
        }));
        Fixture { host, events: rx }
    }

    fn find(haystack: &[u8], needle: &[u8]) -> bool {
        haystack.windows(needle.len()).any(|w| w == needle)
    }

    async fn wait_marker(collect: &Collect, needle: &[u8]) -> Vec<u8> {
        let deadline = Instant::now() + TIMEOUT;
        loop {
            let snapshot = collect.snapshot();
            if find(&snapshot, needle) {
                return snapshot;
            }
            assert!(
                Instant::now() < deadline,
                "marker {needle:?} not seen within {TIMEOUT:?}; got: {:?}",
                String::from_utf8_lossy(&snapshot)
            );
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }

    /// 订阅并起 forward_stream 任务，返回收集句柄。
    async fn drive(host: &PtyHost, id: &str) -> (tokio::task::JoinHandle<()>, Collect) {
        let collect = Collect::default();
        let stream = tokio::time::timeout(TIMEOUT, host.subscribe(id))
            .await
            .expect("subscribe within timeout")
            .expect("subscribe ok");
        let task = tokio::spawn(forward_stream(stream, collect.clone()));
        (task, collect)
    }

    fn count(haystack: &[u8], needle: &[u8]) -> usize {
        haystack.windows(needle.len()).filter(|w| *w == needle).count()
    }

    /// ① echo 回显经 subscribe 的 outbound 通道到达（原 ws_session roundtrip /
    /// ws_server echo 的进程内等价）。
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn echo_roundtrip_via_subscribe() {
        let fx = fixture();
        let id = fx.host.spawn(base_request()).expect("spawn");
        let (_task, collect) = drive(&fx.host, &id).await;
        fx.host.write(&id, b"echo st-ok\n".to_vec()).expect("write");
        let agg = wait_marker(&collect, b"st-ok").await;
        assert!(
            find(&agg, b"st-ok"),
            "got: {:?}",
            String::from_utf8_lossy(&agg)
        );
    }

    /// ② pre-attach backlog 排空：首订阅时 `SessionStream.backlog` 携带存量
    /// （原 ws_session pre_attach_buffer_replayed_on_first_connect）。
    /// 1s 静置：shell 启动 + 回显落环形缓冲远小于该余量（同原 300ms 手法的
    /// 加固版——backlog 专属断言要求写入先于订阅落盘）。
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn pre_attach_backlog_drained_first() {
        let fx = fixture();
        let id = fx.host.spawn(base_request()).expect("spawn");
        tokio::time::sleep(Duration::from_secs(1)).await;
        fx.host.write(&id, b"echo early\n".to_vec()).expect("write");
        tokio::time::sleep(Duration::from_secs(1)).await;
        let stream: SessionStream = tokio::time::timeout(TIMEOUT, fx.host.subscribe(&id))
            .await
            .expect("subscribe within timeout")
            .expect("subscribe ok");
        let backlog = stream
            .backlog
            .iter()
            .flat_map(|chunk| chunk.as_slice().to_vec())
            .collect::<Vec<u8>>();
        assert!(
            find(&backlog, b"early"),
            "backlog must carry the pre-attach buffer; got: {:?}",
            String::from_utf8_lossy(&backlog)
        );
        // 订阅后的实时输出走 outbound（原 WS「重放 ∪ 通道」不重不漏）。
        // forward_stream 先经 sink 重放 backlog 再转发通道，故聚合流含「early」
        // 恰 2 次（输入回显 + 命令输出）——通道若重复重放则为 4 次。
        let collect = Collect::default();
        let task = tokio::spawn(forward_stream(stream, collect.clone()));
        fx.host.write(&id, b"echo live\n".to_vec()).expect("write");
        let agg = wait_marker(&collect, b"live").await;
        assert_eq!(
            count(&agg, b"early"),
            2,
            "pre-attach content must be delivered exactly once (echo+output); got: {:?}",
            String::from_utf8_lossy(&agg)
        );
        task.abort();
    }

    /// ③ 同 id 重复 subscribe 接管：旧流被取消（forward 任务终止），新流照常
    /// 收发，第三次订阅仍可路由（注册表不因替换摘除）
    /// （原 ws_session connection_replaced）。
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn resubscribe_takes_over_old_stream() {
        let fx = fixture();
        let id = fx.host.spawn(base_request()).expect("spawn");
        let (a_task, _a_collect) = drive(&fx.host, &id).await;

        let (b_task, b_collect) = drive(&fx.host, &id).await;
        tokio::time::timeout(TIMEOUT, a_task)
            .await
            .expect("old forward task must end after takeover")
            .expect("old forward task ok");

        fx.host.write(&id, b"echo b-ok\n".to_vec()).expect("write");
        wait_marker(&b_collect, b"b-ok").await;

        // 第三连 C：替换不得摘除注册表（会话仍归 B/C 所有），同 id 仍可订阅。
        assert!(fx.host.is_alive(&id), "takeover must keep the session");
        let (_c_task, c_collect) = drive(&fx.host, &id).await;
        fx.host.write(&id, b"echo c-ok\n".to_vec()).expect("write");
        wait_marker(&c_collect, b"c-ok").await;
        // B 的转发任务随测试结束（tokio 运行时关闭时一并丢弃）。
        b_task.abort();
    }

    /// ④ 退出收尾：exited 触发 + outbound 关闭 + 注册表摘除 + Exit 事件
    /// （原 ws_session close_on_exit；观察者为原始 SessionStream，
    /// 不经 forward_stream——两者语义一致，raw 形态便于逐项断言）。
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn exit_closes_outbound_and_fires_exited() {
        let fx = fixture();
        let id = fx.host.spawn(base_request()).expect("spawn");
        let mut stream = tokio::time::timeout(TIMEOUT, fx.host.subscribe(&id))
            .await
            .expect("subscribe within timeout")
            .expect("subscribe ok");
        fx.host.write(&id, b"exit 0\n".to_vec()).expect("write");

        let code = tokio::time::timeout(TIMEOUT, async {
            stream
                .exited
                .clone()
                .changed()
                .await
                .expect("exit watch alive");
            *stream.exited.borrow()
        })
        .await
        .expect("exited must fire");
        assert_eq!(code, Some(0), "exit 0 must surface on the watch");

        // outbound 关闭：reader EOF 断 hub → 发送端全灭 → 排空后 recv 得 None。
        tokio::time::timeout(TIMEOUT, async {
            while stream.outbound.recv().await.is_some() {}
        })
        .await
        .expect("outbound must close after exit");

        // 注册表摘除（exit watcher：hub 已空时由它摘除）+ Exit 事件到达。
        let deadline = Instant::now() + TIMEOUT;
        while fx.host.has_pty(&id) {
            assert!(Instant::now() < deadline, "session must be removed");
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        let deadline = Instant::now() + TIMEOUT;
        loop {
            match fx.events.recv_timeout(Duration::from_millis(100)) {
                Ok(PtyEvent::Exit(info)) => {
                    assert_eq!(info.id, id);
                    assert_eq!(info.code, 0);
                    break;
                }
                Ok(_) => {}
                Err(_) => assert!(
                    Instant::now() < deadline,
                    "Exit event not seen within {TIMEOUT:?}"
                ),
            }
        }
    }

    /// ⑤ 未知 id → Err（原 ws_server rejects_bad_token 的「不可达被拒」等价；
    /// 鉴权已随偏差 4 取消，路由面只剩 id 命中判定）。
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn unknown_id_errors() {
        let fx = fixture();
        let err = tokio::time::timeout(TIMEOUT, fx.host.subscribe("no-such-id"))
            .await
            .expect("subscribe within timeout");
        assert!(err.is_err(), "unknown id must error");
        // 已退出会话同样不可订阅。
        let id = fx.host.spawn(base_request()).expect("spawn");
        fx.host.kill(&id).expect("kill");
        let err = tokio::time::timeout(TIMEOUT, fx.host.subscribe(&id))
            .await
            .expect("subscribe within timeout");
        assert!(err.is_err(), "killed session must error on subscribe");
    }

    /// ⑥ backlog 先行、实时续后、不重不漏（原「重放 ∪ 通道」不重不漏语义的
    /// 逐字节断言：存量全链路恰出现一次，实时块只在 outbound 出现）。
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn backlog_then_live_no_dup_no_loss() {
        let fx = fixture();
        let id = fx.host.spawn(base_request()).expect("spawn");
        tokio::time::sleep(Duration::from_secs(1)).await;
        fx.host.write(&id, b"echo one\n".to_vec()).expect("write");
        tokio::time::sleep(Duration::from_secs(1)).await;
        let stream = tokio::time::timeout(TIMEOUT, fx.host.subscribe(&id))
            .await
            .expect("subscribe within timeout")
            .expect("subscribe ok");
        let backlog = stream
            .backlog
            .iter()
            .flat_map(|chunk| chunk.as_slice().to_vec())
            .collect::<Vec<u8>>();
        assert!(
            !backlog.is_empty(),
            "pre-attach buffer must have captured the prompt"
        );

        let collect = Collect::default();
        let task = tokio::spawn(forward_stream(stream, collect.clone()));
        fx.host.write(&id, b"echo two\n".to_vec()).expect("write");
        let agg = wait_marker(&collect, b"two").await;
        // 不重不漏（终态回显语义：每个标记 = 输入回显 + 命令输出各 1 次，计 2）：
        // 「one」只经 backlog 重放进入聚合流（恰 2 次——通道重复重放则为 4）；
        // 「two」只在通道（恰 2 次—— backlog 混入则 >2）。
        assert_eq!(
            count(&agg, b"one"),
            2,
            "pre-attach marker must appear exactly once (echo+output) via replay; got: {:?}",
            String::from_utf8_lossy(&agg)
        );
        assert_eq!(
            count(&agg, b"two"),
            2,
            "live marker must appear exactly once (echo+output) via channel; got: {:?}",
            String::from_utf8_lossy(&agg)
        );
        task.abort();
    }
}
