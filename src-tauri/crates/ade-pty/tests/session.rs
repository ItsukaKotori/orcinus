//! Task 5 集成测试：会话 spawn、输出聚合、退出码、env 删除与背压无丢失。
//!
//! 五用例对应 brief Step 1；聚合统一「循环 recv + 5s 总时限」。全部 `#[cfg(unix)]`：
//! 会话核心的 CPR 接线只在 Windows 需要（unix 旁路），本任务测试仅在 unix 运行。

use std::collections::HashMap;
use std::time::{Duration, Instant};

use ade_pty::session::{Chunk, Session, SpawnRequest};

const AGG_TIMEOUT: Duration = Duration::from_secs(5);

fn base_request(cwd: &std::path::Path) -> SpawnRequest {
    SpawnRequest {
        cols: 80,
        rows: 24,
        cwd: Some(cwd.to_string_lossy().into_owned()),
        env: HashMap::new(),
        env_to_delete: Vec::new(),
        command: None,
        shell_override: Some("/bin/sh".to_string()),
    }
}

/// 从 connection 槽取出 outbound 接收端（Task 5 占位：spawn 时槽内即 rx）。
async fn take_outbound(session: &Session) -> tokio::sync::mpsc::Receiver<Chunk> {
    session
        .connection
        .lock()
        .await
        .take()
        .expect("connection slot pre-filled with outbound rx")
}

/// 循环 recv 聚合输出，直到 `pred` 命中；总时限 [`AGG_TIMEOUT`]。
async fn aggregate_until(
    rx: &mut tokio::sync::mpsc::Receiver<Chunk>,
    pred: impl Fn(&[u8]) -> bool,
) -> Vec<u8> {
    let deadline = Instant::now() + AGG_TIMEOUT;
    let mut agg: Vec<u8> = Vec::new();
    loop {
        if pred(&agg) {
            return agg;
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        match tokio::time::timeout(remaining, rx.recv()).await {
            Ok(Some(chunk)) => agg.extend_from_slice(chunk.as_slice()),
            Ok(None) => panic!(
                "outbound closed before predicate matched; got {} bytes",
                agg.len()
            ),
            Err(_) => panic!(
                "timed out after {AGG_TIMEOUT:?}; got {} bytes: {:?}",
                agg.len(),
                String::from_utf8_lossy(&agg)
            ),
        }
    }
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() {
        return Some(0);
    }
    if haystack.len() < needle.len() {
        return None;
    }
    haystack.windows(needle.len()).position(|w| w == needle)
}

/// 返回「回显命令所在行结束（其后的首个 `\n`）之后」的字节。
///
/// 交互式 shell 会把我们写入的命令原样回显（内核 ECHO 或 readline 自绘），命令文本
/// 本身可能包含断言目标（如 `${ADE_PTY_DEL:-missing}` 里的 `missing`），直接全文
/// contains 会被回显骗过；故只看回显行之后的内容。命令文本或行尾尚未收齐时返回
/// 空切片，谓词视为未命中，调用方继续 recv。
fn after_command_echo<'a>(agg: &'a [u8], command: &[u8]) -> &'a [u8] {
    let Some(start) = find(agg, command) else {
        return &[];
    };
    let Some(nl) = find(&agg[start..], b"\n") else {
        return &[];
    };
    &agg[start + nl + 1..]
}

#[cfg(unix)]
#[tokio::test]
async fn echo_flows_to_outbound() {
    let req = SpawnRequest {
        env: HashMap::from([("ADE_PTY_TEST".to_string(), "1".to_string())]),
        ..base_request(&std::env::temp_dir())
    };
    let (session, _runtime) = Session::spawn(req).expect("spawn session");
    let mut rx = take_outbound(&session).await;
    session.write(b"echo ok-$ADE_PTY_TEST\n").expect("write");
    // 回显文本是 `ok-$ADE_PTY_TEST`，不含字面量 `ok-1`，contains 无假阳性。
    let agg = aggregate_until(&mut rx, |bytes| find(bytes, b"ok-1").is_some()).await;
    assert!(find(&agg, b"ok-1").is_some(), "got: {:?}", String::from_utf8_lossy(&agg));
}

#[cfg(unix)]
#[tokio::test]
async fn exit_code_propagates() {
    let (session, runtime) =
        Session::spawn(base_request(&std::env::temp_dir())).expect("spawn session");
    let _rx = take_outbound(&session).await; // 保持通道开启，镜像真实 attach
    session.write(b"exit 7\n").expect("write");
    let code = tokio::time::timeout(AGG_TIMEOUT, runtime.exit)
        .await
        .expect("exit within 5s")
        .expect("exit sender alive");
    assert_eq!(code, 7);
}

#[cfg(unix)]
#[tokio::test]
async fn env_to_delete_removes_key() {
    // 键先注入测试进程 env（spawn 经 base env 继承），再经 env_to_delete 删除。
    std::env::set_var("ADE_PTY_DEL", "present");
    const CMD: &[u8] = b"echo ${ADE_PTY_DEL:-missing}";
    let req = SpawnRequest {
        env_to_delete: vec!["ADE_PTY_DEL".to_string()],
        ..base_request(&std::env::temp_dir())
    };
    let (session, _runtime) = Session::spawn(req).expect("spawn session");
    let mut rx = take_outbound(&session).await;
    session.write(b"echo ${ADE_PTY_DEL:-missing}\n").expect("write");
    let agg = aggregate_until(&mut rx, |bytes| {
        find(after_command_echo(bytes, CMD), b"missing").is_some()
    })
    .await;
    assert!(
        find(after_command_echo(&agg, CMD), b"missing").is_some(),
        "expected 'missing' after command echo, got: {:?}",
        String::from_utf8_lossy(&agg)
    );
}

#[cfg(unix)]
#[tokio::test]
async fn backpressure_pauses_without_loss() {
    // 裁定：输入流必须无换行——tty ONLCR 会把 \n 膨胀为 \r\n，按输入字节数断言会假红。
    // head|tr 产出恰好 4194304 个 'A'、无任何换行；连续 A 游程全长到达即为无丢失。
    const TARGET: usize = 4_194_304;
    let (session, _runtime) =
        Session::spawn(base_request(&std::env::temp_dir())).expect("spawn session");
    let mut rx = take_outbound(&session).await;
    session
        .write(b"head -c 4194304 /dev/zero | tr '\\0' A\n")
        .expect("write");
    // 先不 recv：给 reader 时间把 outbound 通道灌满（满即停读，块滞留通道内）。
    tokio::time::sleep(Duration::from_millis(500)).await;
    let deadline = Instant::now() + AGG_TIMEOUT;
    let mut received = 0usize;
    let (mut run, mut max_run) = (0usize, 0usize);
    loop {
        if max_run >= TARGET {
            break;
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        let chunk = match tokio::time::timeout(remaining, rx.recv()).await {
            Ok(Some(chunk)) => chunk,
            Ok(None) => panic!(
                "outbound closed; received {received} bytes, max A-run {max_run}"
            ),
            Err(_) => panic!(
                "timed out; received {received} bytes, max A-run {max_run} (target {TARGET})"
            ),
        };
        received += chunk.len();
        // 跨块累计连续 'A' 游程（块间不重置）。
        for &b in chunk.as_slice() {
            if b == b'A' {
                run += 1;
                if run > max_run {
                    max_run = run;
                }
            } else {
                run = 0;
            }
        }
    }
    assert!(
        max_run >= TARGET,
        "expected a contiguous run of {TARGET} 'A's, got {max_run} over {received} bytes"
    );
}

#[cfg(unix)]
#[tokio::test]
async fn command_written_after_spawn_runs() {
    const CMD: &[u8] = b"echo delivered";
    let req = SpawnRequest {
        command: Some("echo delivered".to_string()),
        ..base_request(&std::env::temp_dir())
    };
    let (session, _runtime) = Session::spawn(req).expect("spawn session");
    let mut rx = take_outbound(&session).await;
    // 回显行含 "delivered" 字面量，会被全文 contains 骗过；只认回显行之后的输出。
    let agg = aggregate_until(&mut rx, |bytes| {
        find(after_command_echo(bytes, CMD), b"delivered").is_some()
    })
    .await;
    assert!(
        find(after_command_echo(&agg, CMD), b"delivered").is_some(),
        "expected 'delivered' after command echo, got: {:?}",
        String::from_utf8_lossy(&agg)
    );
}
