//! Task 6 测试：Supervisor 回收路径（closer 先断管道 + harvester 兜底）。
//!
//! join 阻塞语义对断言的影响（std 无 join 超时，Supervisor 为 harvester 模式：
//! `reap` 只把句柄入队并立即返回，常驻线程逐个**阻塞** join）：
//! - 「句柄已入队」不能靠立即读队列长度断言（harvester 会异步取走），改用
//!   `pending_len()`（尚未被 harvester 取走的数量）与 `joined_total()`
//!   （已完成 join 的数量）组合观测；
//! - 「永不退出的假句柄」会把 harvester 钉死在 join 上——测试③先钉住再灌队，
//!   入队/淘汰的计数因此完全确定；
//! - 假句柄线程 park 在 `mpsc::Receiver::recv` 上（发送端由测试保活）：
//!   不会像 `thread::park()` 那样被虚假唤醒提前退出，join 因此确定性地永不完成；
//!   丢掉发送端即释放线程（recv 返回 Err → 闭包结束）。

use std::collections::HashMap;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use ade_pty::session::{Session, SpawnRequest};
use ade_pty::supervisor::{Supervisor, REAPER_CAP};

const BOUND: Duration = Duration::from_secs(2);

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

/// 轮询等待条件成立（10ms 步进，2s 上限）——harvester 是异步线程，断言前先等它到位。
fn wait_until(what: &str, pred: impl Fn() -> bool) {
    let deadline = Instant::now() + BOUND;
    while !pred() {
        assert!(
            Instant::now() < deadline,
            "not satisfied within {BOUND:?}: {what}"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// 永不退出的假 reader 线程：park 在 recv 上，发送端由调用方保活。
/// 返回 `(句柄, 保活发送端)`；测试结束前丢掉发送端即释放线程。
fn parked_thread() -> (std::thread::JoinHandle<()>, mpsc::Sender<()>) {
    let (tx, rx) = mpsc::channel::<()>();
    let handle = std::thread::Builder::new()
        .name("fake-reader".to_string())
        .spawn(move || {
            let _ = rx.recv(); // 发送端保活期间永不返回
        })
        .expect("spawn fake reader thread");
    (handle, tx)
}

/// ①集成：真 spawn `/bin/sh`，kill 后 reap 正常——kill 2s 内返回退出码，exit
/// 通道给出同值（同源于 child 的 waitpid 状态），reader 句柄被 harvester join
/// （closer 断 master → reader EOF）。
#[cfg(unix)]
#[tokio::test]
async fn join_completes_after_close() {
    let (session, runtime) = Session::spawn(base_request()).expect("spawn session");
    let sup = Supervisor::new();

    let started = Instant::now();
    let code = session.kill(&sup, runtime.reader_handle);
    assert!(
        started.elapsed() < BOUND,
        "kill took {:?}",
        started.elapsed()
    );

    // kill 的返回码与 exit 通道一致：kill 在 exit 线程释放 child 锁后重读
    // 同一份缓存的 waitpid 状态，两者必须同值。
    let exit_code = tokio::time::timeout(BOUND, runtime.exit)
        .await
        .expect("exit within 2s")
        .expect("exit sender alive");
    assert_eq!(code, exit_code);

    // closer 已断 master → reader EOF → harvester join 完成。
    wait_until("reader joined by harvester", || sup.joined_total() == 1);
}

/// ②假句柄进 Reaper：`reap` 立即返回（绝不内联 join——一旦内联就会被永不退出
/// 的假 reader 卡死），句柄由 harvester 接管并阻塞 join。
#[test]
fn timeout_goes_to_reaper() {
    let sup = Supervisor::new();

    // 真实可结束线程：证明 harvester 确实在执行 join（joined_total 可观测）。
    sup.reap(|| {}, std::thread::spawn(|| {}));
    wait_until("fast thread joined", || sup.joined_total() == 1);
    assert_eq!(sup.pending_len(), 0);

    // 永不退出的假 reader + no-op closer：reap 必须立即返回。
    let (stuck, keep) = parked_thread();
    let started = Instant::now();
    sup.reap(|| {}, stuck);
    assert!(
        started.elapsed() < Duration::from_secs(1),
        "reap blocked on join"
    );

    // harvester 已把句柄取走、卡在 join 上：pending 清零，且 joined_total
    // 不再增长（线程永不退出 → join 确定性永不完成 → 不可能误计）。
    wait_until("stuck handle taken by harvester", || sup.pending_len() == 0);
    assert_eq!(
        sup.joined_total(),
        1,
        "stuck thread must not have been joined"
    );
    // keep（发送端）刻意活到测试结束：维持假线程存活，保持 join 阻塞形态。
    drop(keep);
}

/// ③上限 64：第 65 个句柄入队时淘汰**最旧**（pop_front），保留最新 64 个。
#[test]
fn reaper_evicts_oldest_over_cap() {
    assert_eq!(REAPER_CAP, 64);
    let sup = Supervisor::new();

    // 先钉死 harvester：永不退出句柄让它阻塞在 join 上——之后的入队不再被取走，
    // 队列长度与淘汰行为才完全确定（排除 harvester 异步取走的干扰）。
    let (_pin, release_pin) = parked_thread();
    sup.reap(|| {}, _pin);
    wait_until("harvester pinned", || sup.pending_len() == 0);

    // fake#1（可释放）+ 64 个永不退出假句柄：共 65 个入队 → 触发 cap 淘汰一个。
    let (fake1, release_fake1) = parked_thread();
    sup.reap(|| {}, fake1);
    let mut keep: Vec<mpsc::Sender<()>> = Vec::new();
    for _ in 0..64 {
        let (handle, tx) = parked_thread();
        keep.push(tx);
        sup.reap(|| {}, handle);
    }
    assert_eq!(sup.pending_len(), REAPER_CAP, "queue capped at 64");

    // 释放 fake#1：正确实现下它已被淘汰（detach），退出无人观测；
    // 若实现误淘汰最新（pop_back），fake#1 仍在队中，退出后下一步会被秒 join。
    drop(release_fake1);
    // 释放 pin：harvester 完成 join（joined_total=1），按 FIFO 取走队首并阻塞。
    // 正确实现队首是 fake#2（存活）→ 卡死在 join；误实现队首是 fake#1（已退出）
    // → 秒 join 再取 fake#2，joined_total 变 2、pending 变 62。
    drop(release_pin);
    wait_until("pin joined, harvester advanced to queue front", || {
        sup.joined_total() == 1
    });
    std::thread::sleep(Duration::from_millis(100)); // 给误实现留出连锁 join 的时间窗
    assert_eq!(
        sup.joined_total(),
        1,
        "exactly the pin was joined: front must be the oldest survivor"
    );
    assert_eq!(
        sup.pending_len(),
        REAPER_CAP - 1,
        "front (oldest survivor) is being joined by the harvester"
    );
    // keep 里的发送端活到测试结束：64 个假线程保持存活。
}
