//! Task 9 集成测试：PtyHost 组装（注册表、事件回调、shutdown_all）。
//!
//! 六用例对应 brief Step 1；全部 `#[cfg(unix)]`（会话测试仅在 unix 运行，与
//! tests/session.rs、tests/stream.rs 口径一致）。数据面手法为进程内订阅
//! （Task 15 修订二：subscribe + forward_stream，替代原 WS 客户端）；
//! 事件回调经 std mpsc 捕获供断言轮询。

#[cfg(unix)]
mod unix_tests {
    use std::collections::HashMap;
    use std::sync::mpsc;
    use std::time::{Duration, Instant};

    use ade_pty::server::{forward_stream, ByteSink};
    use ade_pty::session::SpawnRequest;
    use ade_pty::{PtyEvent, PtyHost};

    /// 单步等待的统一时限：超时即 panic 并带上已聚合内容，便于诊断。
    const TIMEOUT: Duration = Duration::from_secs(5);

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

    /// 收集型 sink：字节进共享缓冲（克隆共享）。
    #[derive(Clone, Default)]
    struct Collect(std::sync::Arc<std::sync::Mutex<Vec<u8>>>);

    impl ByteSink for Collect {
        type Error = std::io::Error;

        async fn send(&mut self, bytes: &[u8]) -> Result<(), Self::Error> {
            self.0.lock().expect("collect mutex poisoned").extend_from_slice(bytes);
            Ok(())
        }
    }

    impl Collect {
        fn snapshot(&self) -> Vec<u8> {
            self.0.lock().expect("collect mutex poisoned").clone()
        }
    }

    /// 装配：PtyHost（无端点形态）+ 事件回调入 mpsc。
    struct HostFixture {
        host: std::sync::Arc<PtyHost>,
        events: mpsc::Receiver<PtyEvent>,
    }

    fn spawn_host_fixture() -> HostFixture {
        let host = PtyHost::start(tokio::runtime::Handle::current()).expect("start pty host");
        let (tx, rx) = mpsc::channel::<PtyEvent>();
        host.set_event_callback(Box::new(move |event| {
            let _ = tx.send(event);
        }));
        HostFixture { host, events: rx }
    }

    fn find(haystack: &[u8], needle: &[u8]) -> bool {
        haystack.windows(needle.len()).any(|w| w == needle)
    }

    /// 订阅并起 forward_stream 任务，轮询收集缓冲直到 `needle` 命中。
    async fn collect_until(
        host: &PtyHost,
        id: &str,
        needle: &[u8],
    ) -> (tokio::task::JoinHandle<()>, Vec<u8>) {
        let stream = tokio::time::timeout(TIMEOUT, host.subscribe(id))
            .await
            .expect("subscribe within timeout")
            .expect("subscribe ok");
        let collect = Collect::default();
        let task = tokio::spawn(forward_stream(stream, collect.clone()));
        let deadline = Instant::now() + TIMEOUT;
        loop {
            let snapshot = collect.snapshot();
            if find(&snapshot, needle) {
                return (task, snapshot);
            }
            assert!(
                Instant::now() < deadline,
                "marker {needle:?} not seen within {TIMEOUT:?}; got: {:?}",
                String::from_utf8_lossy(&snapshot)
            );
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }

    /// 轮询事件通道直到 `pred` 命中（事件由异步 watcher 发出，容许调度延迟）。
    fn wait_event(
        what: &str,
        events: &mpsc::Receiver<PtyEvent>,
        pred: impl Fn(&PtyEvent) -> bool,
    ) -> PtyEvent {
        let deadline = Instant::now() + TIMEOUT;
        loop {
            assert!(
                Instant::now() < deadline,
                "event not seen within {TIMEOUT:?}: {what}"
            );
            match events.recv_timeout(Duration::from_millis(100)) {
                Ok(event) if pred(&event) => return event,
                Ok(_) => {}
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    panic!("event channel dropped: {what}")
                }
            }
        }
    }

    /// ① spawn → write → echo 经订阅流到达；Spawned 事件先于一切到达。
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn spawn_write_roundtrip_via_subscribe() {
        let fx = spawn_host_fixture();
        let id = fx.host.spawn(base_request()).expect("spawn");
        wait_event(
            "Spawned",
            &fx.events,
            |e| matches!(e, PtyEvent::Spawned { id: got } if got == &id),
        );
        fx.host.write(&id, b"echo h1-ok\n".to_vec()).expect("write");
        let (_task, agg) = collect_until(&fx.host, &id, b"h1-ok").await;
        assert!(
            find(&agg, b"h1-ok"),
            "got: {:?}",
            String::from_utf8_lossy(&agg)
        );
    }

    /// ② kill 返回退出码、会话即刻摘除（has_pty/is_alive false）、Exit 回调触发
    /// 且码值与 kill 返回一致。
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn kill_returns_code_removes_and_fires_exit() {
        let fx = spawn_host_fixture();
        let id = fx.host.spawn(base_request()).expect("spawn");
        assert!(fx.host.is_alive(&id));
        let code = fx.host.kill(&id).expect("kill");
        assert!(!fx.host.has_pty(&id), "killed session must lose its pty");
        assert!(!fx.host.is_alive(&id));
        let event = wait_event(
            "Exit after kill",
            &fx.events,
            |e| matches!(e, PtyEvent::Exit(info) if info.id == id),
        );
        match event {
            PtyEvent::Exit(info) => assert_eq!(info.code, code, "exit code must match kill"),
            other => panic!("expected Exit, got {other:?}"),
        }
    }

    /// ③ is_alive 即 reattach 判定：活会话 true（含已订阅），kill/未知 id false。
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn is_alive_reattach_predicate() {
        let fx = spawn_host_fixture();
        let id = fx.host.spawn(base_request()).expect("spawn");
        assert!(fx.host.is_alive(&id), "spawned session must be alive");
        let stream = tokio::time::timeout(TIMEOUT, fx.host.subscribe(&id))
            .await
            .expect("subscribe within timeout")
            .expect("subscribe ok");
        // 持有流（订阅在途）期间会话保持 alive；短等一拍让接管生效。
        tokio::time::sleep(Duration::from_millis(200)).await;
        assert!(fx.host.is_alive(&id), "subscribed session must stay alive");
        drop(stream);
        fx.host.kill(&id).expect("kill");
        assert!(!fx.host.is_alive(&id), "killed session must not be alive");
        assert!(!fx.host.is_alive("no-such-id"));
    }

    /// ④ 4 会话并发独立：各自的 echo 只到达各自的订阅流；list_sessions 计 4。
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn four_sessions_concurrent_independent() {
        let fx = spawn_host_fixture();
        let mut conns: Vec<(String, tokio::task::JoinHandle<()>, Collect)> = Vec::new();
        for _ in 0..4 {
            let id = fx.host.spawn(base_request()).expect("spawn");
            let stream = tokio::time::timeout(TIMEOUT, fx.host.subscribe(&id))
                .await
                .expect("subscribe within timeout")
                .expect("subscribe ok");
            let collect = Collect::default();
            let task = tokio::spawn(forward_stream(stream, collect.clone()));
            conns.push((id, task, collect));
        }
        assert_eq!(fx.host.list_sessions().len(), 4, "four sessions listed");
        for (i, (id, _task, _collect)) in conns.iter().enumerate() {
            fx.host
                .write(id, format!("echo s{i}-ok\n").into_bytes())
                .expect("write");
        }
        for (i, (_id, _task, collect)) in conns.iter().enumerate() {
            let marker = format!("s{i}-ok");
            let deadline = Instant::now() + TIMEOUT;
            let agg = loop {
                let snapshot = collect.snapshot();
                if find(&snapshot, marker.as_bytes()) {
                    break snapshot;
                }
                assert!(
                    Instant::now() < deadline,
                    "session {i} echo not seen; got: {:?}",
                    String::from_utf8_lossy(&snapshot)
                );
                tokio::time::sleep(Duration::from_millis(20)).await;
            };
            assert!(
                find(&agg, marker.as_bytes()),
                "got: {:?}",
                String::from_utf8_lossy(&agg)
            );
            for j in 0..4 {
                if j != i {
                    let other = format!("s{j}-ok");
                    assert!(
                        !find(&agg, other.as_bytes()),
                        "session {i} must not see session {j}'s echo"
                    );
                }
            }
        }
    }

    /// ⑤ SIGINT 对 `exec sleep 60` 的会话使其退出且 wait 码非零（unix）。
    /// `exec` 让 shell 进程替换为 sleep——信号命中的 pid 即前台进程本身。
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn sigint_terminates_sleeping_session_nonzero() {
        let fx = spawn_host_fixture();
        let mut req = base_request();
        req.command = Some("exec sleep 60".to_string());
        let id = fx.host.spawn(req).expect("spawn");
        // 等 shell 启动并完成 exec（启动远小于 500ms；过早 SIGINT 会被交互
        // shell 在提示符处丢弃，sleep 的 exec 后则命中目标进程本身）。
        tokio::time::sleep(Duration::from_millis(500)).await;
        fx.host.signal(&id, "SIGINT").expect("signal");
        let event = wait_event(
            "Exit after SIGINT",
            &fx.events,
            |e| matches!(e, PtyEvent::Exit(info) if info.id == id),
        );
        match event {
            PtyEvent::Exit(info) => {
                assert_ne!(info.code, 0, "SIGINT death must yield non-zero code");
            }
            other => panic!("expected Exit, got {other:?}"),
        }
    }

    /// ⑥ shutdown_all 后全部摘除：list 空、has_pty/is_alive 全 false。
    /// （无 server 可关——数据面已改进程内订阅，摘除以注册表观测为准。）
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn shutdown_all_removes_everything() {
        let fx = spawn_host_fixture();
        let mut ids = Vec::new();
        for _ in 0..3 {
            ids.push(fx.host.spawn(base_request()).expect("spawn"));
        }
        fx.host.shutdown_all();
        assert!(fx.host.list_sessions().is_empty(), "registry must be empty");
        for id in &ids {
            assert!(!fx.host.has_pty(id));
            assert!(!fx.host.is_alive(id));
        }
    }
}
